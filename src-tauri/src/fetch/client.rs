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
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
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
pub fn describe_error(err: &reqwest::Error) -> String {
    if err.is_timeout() {
        format!("request timed out: {err}")
    } else {
        err.to_string()
    }
}

pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    fetch_bytes_with(shared_client(), url).await
}

/// Same as [`fetch_bytes`], but against an explicit client - lets
/// discovery's probes and tests use a different client than production's
/// shared one (e.g. a mock-server-pointed client in tests).
pub(crate) async fn fetch_bytes_with(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>, reqwest::Error> {
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

/// Outcome of a conditional GET (see ARCHITECTURE.md §4.1, §6.2).
#[derive(Debug)]
pub enum ConditionalFetch {
    /// Server returned `304 Not Modified` - no body was sent.
    NotModified,
    /// Server returned `200 OK` with a body, plus whatever
    /// ETag/Last-Modified it sent back (to persist for next time).
    Modified {
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
) -> Result<ConditionalFetch, reqwest::Error> {
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
) -> Result<ConditionalFetch, reqwest::Error> {
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
    let bytes = response.bytes().await?.to_vec();

    Ok(ConditionalFetch::Modified {
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
        assert!(err.is_timeout());
    }
}
