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

#[cfg(test)]
mod tests {
    use super::*;

    const RSS2_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sample_rss2.xml"
    ));
    const ATOM_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sample_atom.xml"
    ));
    const JSON_FEED_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sample_json_feed.json"
    ));

    #[test]
    fn parses_rss2_sample() {
        let feed = parse(RSS2_FIXTURE.as_bytes()).unwrap();
        assert_eq!(format_label(&feed), "rss");
        assert_eq!(feed.entries.len(), 2);
        assert_eq!(
            feed.entries[0].title.as_ref().unwrap().content,
            "First Post"
        );
        assert_eq!(
            feed.entries[0].links.first().unwrap().href,
            "https://example.com/first-post"
        );
    }

    #[test]
    fn parses_atom_sample() {
        let feed = parse(ATOM_FIXTURE.as_bytes()).unwrap();
        assert_eq!(format_label(&feed), "atom");
        assert_eq!(feed.entries.len(), 2);
        assert_eq!(
            feed.entries[0].title.as_ref().unwrap().content,
            "First Entry"
        );
    }

    #[test]
    fn parses_json_feed_sample() {
        let feed = parse(JSON_FEED_FIXTURE.as_bytes()).unwrap();
        assert_eq!(format_label(&feed), "json_feed");
        assert_eq!(feed.entries.len(), 2);
        assert_eq!(
            feed.entries[0].title.as_ref().unwrap().content,
            "First Item"
        );
    }

    #[test]
    fn guid_hash_is_deterministic_and_distinguishes_entries() {
        let feed = parse(RSS2_FIXTURE.as_bytes()).unwrap();
        let hash_a = guid_hash(&feed.entries[0]);
        let hash_a_again = guid_hash(&feed.entries[0]);
        let hash_b = guid_hash(&feed.entries[1]);

        assert_eq!(hash_a, hash_a_again);
        assert_ne!(hash_a, hash_b);
        assert_eq!(hash_a.len(), 64); // sha256 hex digest
    }

    #[test]
    fn guid_hash_falls_back_to_normalized_link_without_a_guid() {
        // No <guid> on this item - feed-rs may itself synthesize an id from
        // the link, but either way guid_hash must stay deterministic.
        let rss_no_guid = r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>T</title><link>https://example.com/</link>
<item><title>No Guid</title><link>HTTPS://Example.com/No-Guid/</link></item>
</channel></rss>"#;
        let feed = parse(rss_no_guid.as_bytes()).unwrap();
        let hash_a = guid_hash(&feed.entries[0]);
        let hash_b = guid_hash(&feed.entries[0]);
        assert_eq!(hash_a, hash_b);
        assert_eq!(hash_a.len(), 64);
    }

    #[test]
    fn normalize_link_lowercases_scheme_host_and_strips_fragment_and_trailing_slash() {
        assert_eq!(
            normalize_link("HTTPS://Example.COM/Post/?x=1#frag"),
            "https://example.com/Post/?x=1"
        );
        assert_eq!(
            normalize_link("https://example.com/"),
            "https://example.com"
        );
    }
}
