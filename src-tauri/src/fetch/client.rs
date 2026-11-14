//! HTTP fetch for feed URLs: a plain GET for `add_feed`'s one-off
//! fetch, and a conditional GET (ETag/If-Modified-Since) for the
//! polling path (see docs/ARCHITECTURE.md §4.1, §6.1, §6.2). Both
//! share a single pooled client for connection reuse.

use std::sync::OnceLock;
use std::time::Duration;

const USER_AGENT: &str = concat!("Aequo/", env!("CARGO_PKG_VERSION"));

/// Applied by the client to every request's connect+read as a whole
/// (not just connection setup) - one unresponsive feed can't stall a
/// polling cycle for other feeds sharing this client (§4.2).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("failed to build shared reqwest client")
    })
}

pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    let response = shared_client().get(url).send().await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

/// Outcome of a conditional GET (see ARCHITECTURE.md §4.1, §6.2).
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
    let mut request = shared_client().get(url);
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
