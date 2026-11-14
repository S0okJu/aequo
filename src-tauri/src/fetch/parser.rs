//! `feed-rs` wrapper: parses RSS 2.0/0.9x, Atom, and JSON Feed input into
//! feed-rs's shared internal model (see docs/ARCHITECTURE.md §4.4). Format
//! is auto-detected by feed-rs from the content itself - callers don't
//! need to know which format a feed is in before parsing it.

use feed_rs::model::{Entry, Feed, FeedType};
use feed_rs::parser;
use sha2::{Digest, Sha256};

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

/// Dedup key for an entry: a hash of its guid/id if present, otherwise a
/// hash of its normalized primary link (see docs/ARCHITECTURE.md §4.4).
/// feed-rs always populates `Entry::id` from the feed's guid/atom-id, so
/// an empty id here means the feed genuinely provided neither.
pub fn guid_hash(entry: &Entry) -> String {
    let basis = if !entry.id.trim().is_empty() {
        entry.id.trim().to_string()
    } else {
        normalize_link(entry.links.first().map(|l| l.href.as_str()).unwrap_or(""))
    };
    hash_hex(&basis)
}

/// Lowercases the scheme and host and strips the fragment and a single
/// trailing slash, leaving path/query case untouched (paths can be
/// case-sensitive on the server, so we don't touch them).
fn normalize_link(link: &str) -> String {
    let trimmed = link.trim();
    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let lowered = match without_fragment.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = match rest.find('/') {
                Some(idx) => (&rest[..idx], &rest[idx..]),
                None => (rest, ""),
            };
            format!(
                "{}://{}{}",
                scheme.to_lowercase(),
                authority.to_lowercase(),
                path
            )
        }
        None => without_fragment.to_string(),
    };
    lowered.strip_suffix('/').unwrap_or(&lowered).to_string()
}

fn hash_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
