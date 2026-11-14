//! `feed-rs` wrapper: parses RSS 2.0/0.9x, Atom, and JSON Feed input into
//! feed-rs's shared internal model (see docs/ARCHITECTURE.md §4.4). Format
//! is auto-detected by feed-rs from the content itself - callers don't
//! need to know which format a feed is in before parsing it.

use feed_rs::model::{Feed, FeedType};
use feed_rs::parser;

pub use feed_rs::parser::ParseFeedError;

/// Parses RSS/Atom/JSON Feed bytes into feed-rs's shared `Feed` model.
pub fn parse(bytes: &[u8]) -> Result<Feed, ParseFeedError> {
    parser::parse(bytes)
}

/// Maps feed-rs's detected format to the label stored in `feeds.format`.
pub fn format_label(feed: &Feed) -> &'static str {
    match feed.feed_type {
        FeedType::Atom => "atom",
        FeedType::JSON => "json_feed",
        FeedType::RSS0 | FeedType::RSS1 | FeedType::RSS2 => "rss",
    }
}
