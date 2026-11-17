//! HTTP client (reqwest) and feed-rs parsing, including conditional
//! requests via ETag/Last-Modified (see docs/ARCHITECTURE.md §4.1, §4.4).

mod backoff;
pub mod article_fetch;
pub mod client;
pub mod discovery;
pub mod enrichment;
pub mod hydration;
pub mod parser;
mod raw_content;
pub mod worker;
pub mod webpage;
