//! HTTP fetch for feed URLs: a plain GET for `add_feed`'s one-off
//! fetch, and a conditional GET (ETag/If-Modified-Since) for the
//! polling path (see docs/ARCHITECTURE.md §4.1, §6.1, §6.2). Both
//! share a single pooled client for connection reuse.

use std::sync::OnceLock;
use std::time::Duration;

/// Looks like an ordinary desktop browser rather than a bare app
/// name/version - some feed hosts (Medium, various corporate blogs)
/// WAF-block generic/unbranded bot user agents outright, even for
/// their own public RSS endpoints.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/131.0.0.0 Safari/537.36";

const ACCEPT: &str =
    "application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.8, */*;q=0.7";

/// Time to establish the TCP/TLS connection. Separate from
/// `REQUEST_TIMEOUT` so a host that's simply slow to *respond* (but
/// did connect) gets more room, while a dead/unreachable host still
/// fails fast instead of holding a polling slot for the full
/// request timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Applied by the client to every request's connect+read as a whole
/// (not just connection setup) - one unresponsive feed can't stall a
/// polling cycle for other feeds sharing this client (§4.2).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Limit decompressed feed/discovery input, including chunked responses.
pub const MAX_FEED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub enum FetchError {
    Http(reqwest::Error),
    ResponseTooLarge { limit: usize },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(error) => std::fmt::Display::fmt(error, f),
            Self::ResponseTooLarge { limit } => {
                write!(f, "feed response exceeds the {limit}-byte reading limit")
            }
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::ResponseTooLarge { .. } => None,
        }
    }
}

impl From<reqwest::Error> for FetchError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(error)
    }
}

async fn read_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, FetchError> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(FetchError::ResponseTooLarge { limit });
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        // Check actual (reqwest-decompressed) bytes even without Content-Length.
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(FetchError::ResponseTooLarge { limit });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(crate) fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::ACCEPT, ACCEPT.parse().unwrap());

        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("failed to build shared reqwest client")
    })
}

/// Describes a fetch error for `last_error`/logs: `reqwest::Error`'s
/// `Display` doesn't reliably surface "timed out" in its top-level
/// message, so callers that only have the string (not the structured
/// error) can't tell a timeout from any other connection failure.
pub fn describe_error(err: &FetchError) -> String {
    if matches!(err, FetchError::Http(error) if error.is_timeout()) {
        format!("request timed out: {err}")
    } else {
        err.to_string()
    }
}

/// HTML relative links must resolve against the final redirect destination.
pub async fn fetch_page(url: &str) -> Result<(String, Vec<u8>), FetchError> {
    let response = shared_client().get(url).send().await?.error_for_status()?;
    let final_url = response.url().to_string();
    Ok((final_url, read_limited(response, MAX_FEED_BYTES).await?))
}

#[cfg(test)]
pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, FetchError> {
    fetch_bytes_with(shared_client(), url).await
}

/// Same as [`fetch_bytes`], but against an explicit client - lets
/// discovery's probes and tests use a different client than production's
/// shared one (e.g. a mock-server-pointed client in tests).
pub(crate) async fn fetch_bytes_with(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>, FetchError> {
    let response = client.get(url).send().await?.error_for_status()?;
    read_limited(response, MAX_FEED_BYTES).await
}

/// Outcome of a conditional GET (see ARCHITECTURE.md §4.1, §6.2).
#[derive(Debug)]
pub enum ConditionalFetch {
    /// Server returned `304 Not Modified` - no body was sent.
    NotModified,
    /// Server returned `200 OK` with a body, plus whatever
    /// ETag/Last-Modified it sent back (to persist for next time).
    Modified {
        final_url: String,
        bytes: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

/// GETs `url`, sending `If-None-Match`/`If-Modified-Since` when the
/// caller has a prior etag/last_modified for this feed.
pub async fn fetch_conditional(
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<ConditionalFetch, FetchError> {
    fetch_conditional_with(shared_client(), url, etag, last_modified).await
}

/// Same as [`fetch_conditional`], but against an explicit client -
/// lets tests use a short timeout against a mock server instead of
/// production's `REQUEST_TIMEOUT`.
async fn fetch_conditional_with(
    client: &reqwest::Client,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<ConditionalFetch, FetchError> {
    let mut request = client.get(url);
    if let Some(etag) = etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    if let Some(last_modified) = last_modified {
        request = request.header(reqwest::header::IF_MODIFIED_SINCE, last_modified);
    }

    let response = request.send().await?;

    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(ConditionalFetch::NotModified);
    }

    let response = response.error_for_status()?;
    let etag = header_str(&response, reqwest::header::ETAG);
    let last_modified = header_str(&response, reqwest::header::LAST_MODIFIED);
    let final_url = response.url().to_string();
    let bytes = read_limited(response, MAX_FEED_BYTES).await?;

    Ok(ConditionalFetch::Modified {
        final_url,
        bytes,
        etag,
        last_modified,
    })
}

fn header_str(response: &reqwest::Response, name: reqwest::header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn fetch_conditional_sends_both_validators() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/conditional.xml"))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&mock_server)
            .await;

        let url = format!("{}/conditional.xml", mock_server.uri());
        let outcome = fetch_conditional_with(
            &reqwest::Client::new(),
            &url,
            Some("\"etag-1\""),
            Some("Mon, 01 Jan 2024 00:00:00 GMT"),
        )
        .await
        .unwrap();

        assert!(matches!(outcome, ConditionalFetch::NotModified));
        let requests = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].headers.get("if-none-match").unwrap(),
            "\"etag-1\""
        );
        assert_eq!(
            requests[0].headers.get("if-modified-since").unwrap(),
            "Mon, 01 Jan 2024 00:00:00 GMT"
        );
    }

    /// Verifies the timeout mechanism itself (a client built with a
    /// short timeout errors out against a slow server), rather than
    /// production's 10s value - that would make this test take 10+
    /// real seconds for no extra confidence.
    #[tokio::test]
    async fn fetch_conditional_errors_on_timeout() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/slow.xml"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(300)))
            .mount(&mock_server)
            .await;

        let short_timeout_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();

        let url = format!("{}/slow.xml", mock_server.uri());
        let result = fetch_conditional_with(&short_timeout_client, &url, None, None).await;

        let err = result.expect_err("expected a timeout error");
        assert!(matches!(err, FetchError::Http(error) if error.is_timeout()));
    }

    #[tokio::test]
    async fn feed_fetch_rejects_oversized_plain_and_conditional_responses() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; MAX_FEED_BYTES + 1]))
            .mount(&server)
            .await;
        assert!(matches!(
            fetch_bytes(&server.uri()).await,
            Err(FetchError::ResponseTooLarge {
                limit: MAX_FEED_BYTES
            })
        ));
        assert!(matches!(
            fetch_conditional(&server.uri(), None, None).await,
            Err(FetchError::ResponseTooLarge {
                limit: MAX_FEED_BYTES
            })
        ));
    }

    #[tokio::test]
    async fn reading_limit_accepts_boundary_and_rejects_unknown_length_overflow() {
        use std::io::{Read, Write};
        for (length, accepted) in [(32, true), (33, false)] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                assert!(socket.read(&mut request).unwrap() > 0);
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                    .unwrap();
                socket.write_all(&vec![b'x'; length]).unwrap();
            });
            let response = reqwest::get(format!("http://{address}")).await.unwrap();
            assert_eq!(response.content_length(), None);
            let result = read_limited(response, 32).await;
            if accepted {
                assert_eq!(result.unwrap(), vec![b'x'; 32]);
            } else {
                assert!(matches!(
                    result,
                    Err(FetchError::ResponseTooLarge { limit: 32 })
                ));
            }
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn reading_limit_applies_after_gzip_decompression() {
        // gzip of 64 'x' bytes: transfer is below 32 bytes, decoded input exceeds it.
        let compressed = vec![
            31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 171, 168, 160, 12, 0, 0, 18, 172, 210, 58, 64, 0, 0,
            0,
        ];
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Encoding", "gzip")
                    .set_body_bytes(compressed),
            )
            .mount(&server)
            .await;
        let response = reqwest::get(server.uri()).await.unwrap();
        assert!(matches!(
            read_limited(response, 32).await,
            Err(FetchError::ResponseTooLarge { limit: 32 })
        ));
    }
}

#[cfg(test)]
mod redirect_tests {
    use super::*;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    #[tokio::test]
    async fn html_fetches_report_final_base_after_redirects() {
        let server = MockServer::start().await;
        Mock::given(path("/alias"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}/blog/", server.uri())),
            )
            .mount(&server)
            .await;
        Mock::given(path("/blog/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>ok</html>"))
            .mount(&server)
            .await;
        let url = format!("{}/alias", server.uri());
        let (base, body) = fetch_page(&url).await.unwrap();
        assert_eq!(base, format!("{}/blog/", server.uri()));
        assert_eq!(body, b"<html>ok</html>");
        match fetch_conditional(&url, None, None).await.unwrap() {
            ConditionalFetch::Modified { final_url, .. } => assert_eq!(final_url, base),
            _ => panic!("expected HTML body"),
        }
    }
}
