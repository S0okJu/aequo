//! Minimal HTTP fetch for a feed URL (see docs/ARCHITECTURE.md §6.1).
//! Conditional requests (ETag/If-Modified-Since) and a shared/pooled
//! client for the polling path land in M3 - this is only used for the
//! one-off `add_feed` fetch, where a fresh client per call is fine.

use std::time::Duration;

const USER_AGENT: &str = concat!("Aequo/", env!("CARGO_PKG_VERSION"));
const TIMEOUT_SECS: u64 = 10;

pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build()?;
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}
