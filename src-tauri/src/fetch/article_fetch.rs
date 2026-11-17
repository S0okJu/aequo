//! Shared fetch-and-extract logic for a single article's original URL:
//! redirect-following with SSRF guards re-checked on every hop, a 5MB
//! cap, `legible::parse` extraction with the `hydration` module as a
//! fallback for client-side-rendered pages, and a per-host rate limit.
//! Used by both `commands::article::load_article` (on-demand reads) and
//! `fetch::enrichment` (poll-time pre-fetching) - security- and
//! correctness-sensitive, so it lives in one place rather than being
//! duplicated between callers.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::{Client, StatusCode, Url, header};
use tokio::sync::Mutex as AsyncMutex;

use super::hydration;

const MAX_HTML_BYTES: usize = 5 * 1024 * 1024;
const MAX_REDIRECTS: usize = 6;
// Real per-host spacing in production; a short interval under tests so
// the (process-global, shared across every test in this binary) host
// throttle below doesn't make the suite slow or let one test's delay
// bleed into another's timing.
#[cfg(not(test))]
const HOST_REQUEST_INTERVAL: Duration = Duration::from_secs(10);
#[cfg(test)]
const HOST_REQUEST_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug)]
pub struct ExtractedArticle {
    pub final_url: String,
    pub title: String,
    pub content_html: String,
    pub text_content: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

fn host_locks() -> &'static Mutex<HashMap<String, Arc<AsyncMutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn last_host_requests() -> &'static Mutex<HashMap<String, Instant>> {
    static REQUESTS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn wait_for_host(host: &str) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    let lock = {
        let mut locks = host_locks()
            .lock()
            .map_err(|_| "article request lock poisoned".to_string())?;
        locks
            .entry(host.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    };
    let guard = lock.lock_owned().await;
    let delay = {
        let requests = last_host_requests()
            .lock()
            .map_err(|_| "article request lock poisoned".to_string())?;
        requests
            .get(host)
            .map(|last| HOST_REQUEST_INTERVAL.saturating_sub(last.elapsed()))
            .unwrap_or_default()
    };
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    last_host_requests()
        .lock()
        .map_err(|_| "article request lock poisoned".to_string())?
        .insert(host.to_string(), Instant::now());
    Ok(guard)
}

fn is_denied_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.octets()[0] == 0
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
                || (ip.octets()[0] == 198 && (18..=19).contains(&ip.octets()[1]))
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| is_denied_ip(IpAddr::V4(mapped)))
        }
    }
}

async fn validate_public_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err("Only public HTTP or HTTPS article links can be fetched.".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Article links with embedded credentials are not supported.".to_string());
    }

    let host = url
        .host_str()
        .ok_or_else(|| "Article URL has no host.".to_string())?;
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err("Local network addresses are not fetched for article previews.".to_string());
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_denied_ip(ip) {
            return Err(
                "Local network addresses are not fetched for article previews.".to_string(),
            );
        }
        return Ok(());
    }

    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Article URL has no port.".to_string())?;
    let resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "Could not resolve the article host.".to_string())?;
    let addresses: Vec<_> = resolved.collect();
    if addresses.is_empty() || addresses.iter().any(|address| is_denied_ip(address.ip())) {
        return Err("Article host resolves to a local or restricted network address.".to_string());
    }
    Ok(())
}

/// `Ok(None)` means the server returned `304 Not Modified` - only
/// reachable when the caller passes a real `etag`/`last_modified` (the
/// poll-time enrichment path always passes `None, None` and never sees
/// this case).
pub async fn fetch_and_extract(
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<Option<ExtractedArticle>, String> {
    fetch_and_extract_impl(url, etag, last_modified, true).await
}

/// Test-only: bypasses the public-host/SSRF guard so tests can target a
/// local mock server. `cfg(test)` strips this entirely from a release
/// build; `fetch::enrichment`'s tests use this instead of the real
/// guarded path since they live in a different module.
#[cfg(test)]
pub(crate) async fn fetch_and_extract_for_tests(
    url: &str,
) -> Result<Option<ExtractedArticle>, String> {
    fetch_and_extract_impl(url, None, None, false).await
}

async fn fetch_and_extract_impl(
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
    enforce_public_host: bool,
) -> Result<Option<ExtractedArticle>, String> {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Could not prepare article request: {error}"))?;
    let mut current = Url::parse(url).map_err(|_| "The article URL is invalid.".to_string())?;
    let mut host_guards = HashMap::new();

    for redirect_count in 0..=MAX_REDIRECTS {
        if enforce_public_host {
            validate_public_url(&current).await?;
        }
        let host = current.host_str().unwrap_or_default().to_ascii_lowercase();
        if !host_guards.contains_key(&host) {
            host_guards.insert(host.clone(), wait_for_host(&host).await?);
        }

        let mut request = client
            .get(current.clone())
            .header(header::USER_AGENT, "Aequo/0.1 (personal RSS reader)")
            .header(
                header::ACCEPT,
                "text/html,application/xhtml+xml;q=0.9,*/*;q=0.1",
            );
        if redirect_count == 0 {
            if let Some(etag) = etag {
                request = request.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(last_modified) = last_modified {
                request = request.header(header::IF_MODIFIED_SINCE, last_modified);
            }
        }

        let mut response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                "The article page took too long to respond.".to_string()
            } else {
                format!("Could not load the article page: {error}")
            }
        })?;
        let status = response.status();

        if status == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        if status.is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err("The article page redirected too many times.".to_string());
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "The article page returned an invalid redirect.".to_string())?;
            current = current
                .join(location)
                .map_err(|_| "The article page returned an invalid redirect.".to_string())?;
            continue;
        }
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => "The site requires access that Aequo does not have. Open the original page instead.".to_string(),
                429 => "The site is limiting requests. Aequo will wait before trying this host again.".to_string(),
                code => format!("The article page returned HTTP {code}."),
            });
        }

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !content_type.is_empty()
            && !content_type.contains("text/html")
            && !content_type.contains("application/xhtml+xml")
        {
            return Err("The original link did not return an HTML page.".to_string());
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_HTML_BYTES as u64)
        {
            return Err("The article page is larger than Aequo's 5 MB reading limit.".to_string());
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Could not read the article page.".to_string())?
        {
            if bytes.len() + chunk.len() > MAX_HTML_BYTES {
                return Err(
                    "The article page is larger than Aequo's 5 MB reading limit.".to_string(),
                );
            }
            bytes.extend_from_slice(&chunk);
        }
        let html = String::from_utf8_lossy(&bytes);
        let readable = legible::parse(&html, Some(current.as_str()), None)
            .ok()
            .filter(|article| article.text_content.trim().len() >= 80);

        // Some sites (client-side-rendered Nuxt/Next SSR shells) never put
        // the real article body into the DOM at all - it only exists as an
        // escaped HTML string inside a hydration payload script. Try that
        // before giving up.
        let (title, content_html, text_content) = match readable {
            Some(article) => (article.title, article.content, article.text_content),
            None => {
                let hydrated = hydration::extract(&html).ok_or_else(|| {
                    "Aequo could not find a readable article in this page.".to_string()
                })?;
                (
                    hydrated.title.unwrap_or_default(),
                    hydrated.content_html,
                    hydrated.text_content,
                )
            }
        };

        return Ok(Some(ExtractedArticle {
            final_url: current.to_string(),
            title,
            content_html,
            text_content,
            etag: response
                .headers()
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            last_modified: response
                .headers()
                .get(header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
        }));
    }
    Err("The article page could not be loaded.".to_string())
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn extracts_a_readable_article() {
        let mock_server = MockServer::start().await;
        let html = r#"<html><head><title>Example</title></head><body>
<article><p>This is a long enough paragraph of real article text to clear the readability floor comfortably.</p></article>
</body></html>"#;
        Mock::given(method("GET"))
            .and(path("/post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html"))
            .mount(&mock_server)
            .await;

        let url = format!("{}/post", mock_server.uri());
        let result = fetch_and_extract_for_tests(&url).await.unwrap().unwrap();

        assert!(result.text_content.contains("long enough paragraph"));
    }

    #[tokio::test]
    async fn returns_an_error_on_404() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let url = format!("{}/missing", mock_server.uri());
        let result = fetch_and_extract_for_tests(&url).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_and_extract_rejects_loopback_hosts() {
        // The real, SSRF-guarded entry point must reject a mock server
        // running on loopback - this is the production safety that
        // `fetch_and_extract_for_tests` deliberately bypasses for the
        // tests above.
        let mock_server = MockServer::start().await;
        let url = format!("{}/post", mock_server.uri());
        let result = fetch_and_extract(&url, None, None).await;
        assert!(result.is_err());
    }

    #[test]
    fn is_denied_ip_blocks_private_loopback_and_link_local_ranges() {
        assert!(is_denied_ip("127.0.0.1".parse().unwrap()));
        assert!(is_denied_ip("10.0.0.5".parse().unwrap()));
        assert!(is_denied_ip("192.168.1.1".parse().unwrap()));
        assert!(is_denied_ip("169.254.169.254".parse().unwrap())); // cloud metadata endpoint
        assert!(is_denied_ip("::1".parse().unwrap()));
        assert!(!is_denied_ip("93.184.216.34".parse().unwrap())); // public IP
    }
}
