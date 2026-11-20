//! HTTP client (reqwest) and feed-rs parsing, including conditional
//! requests via ETag/Last-Modified (see docs/ARCHITECTURE.md §4.1, §4.4).

pub mod article_fetch;
mod backoff;
pub mod client;
pub mod discovery;
pub mod enrichment;
pub mod favicon;
pub mod hydration;
pub mod parser;
mod raw_content;
pub mod raw_store;
pub mod synthetic_feed;
pub mod webpage;
pub mod worker;
