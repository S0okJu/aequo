//! Recovers `<content:encoded>` bodies that feed-rs's model silently
//! drops.
//!
//! Some RSS feeds (observed on `thenewstack.io/blog/feed/`, almost
//! certainly from a WordPress plugin that reuses the tag for an
//! unrelated "dek"/standfirst field) emit *two* `content:encoded`
//! elements inside one `<item>`: the first holds the real article
//! body, the second - later in the item - holds a short one-line
//! summary. feed-rs's parser (`src/parser/rss2/mod.rs`, upstream)
//! just assigns `entry.content` on every match in document order, so
//! the last element wins and the real body is gone by the time we
//! see the parsed `Feed`.
//!
//! This module re-scans the raw bytes independently of feed-rs and,
//! for each `<item>`, returns the *longest* `content:encoded` value
//! found - recovering the real body regardless of which occurrence
//! came last. `parser::to_new_items` then takes the longer of this
//! and whatever feed-rs itself produced, so a well-formed feed (single
//! `content:encoded` per item) is completely unaffected.

use quick_xml::events::Event;
use quick_xml::name::{QName, ResolveResult};
use quick_xml::reader::NsReader;

const CONTENT_NS: &[u8] = b"http://purl.org/rss/1.0/modules/content/";

fn is_item(name: QName) -> bool {
    name.local_name().as_ref() == b"item"
}

fn is_content_encoded(resolved: &ResolveResult, name: QName) -> bool {
    name.local_name().as_ref() == b"encoded"
        && matches!(resolved, ResolveResult::Bound(ns) if ns.as_ref() == CONTENT_NS)
}

/// For each `<item>` encountered, in document order, returns the
/// longest `content:encoded` text found directly inside it (`None` if
/// it had none). feed-rs emits exactly one `Entry` per `<item>` in
/// the same order and never drops one, so the result is positionally
/// aligned with `Feed::entries` - callers should still check the
/// lengths match before zipping, as cheap insurance against a future
/// feed-rs behaviour change.
///
/// Best-effort: any XML error simply ends the scan early, returning
/// whatever items were already collected (and thus - via the length
/// check above - no recovered content at all, same as before this
/// module existed).
pub fn longest_content_encoded_per_item(bytes: &[u8]) -> Vec<Option<String>> {
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().trim_text(false);

    let mut results = Vec::new();
    let mut depth: u32 = 0;
    let mut item_depth: Option<u32> = None;
    let mut best_for_item: Option<String> = None;
    let mut encoded_depth: Option<u32> = None;
    let mut captured = String::new();

    while let Ok((resolved, event)) = reader.read_resolved_event() {
        match event {
            Event::Eof => break,
            Event::Start(e) => {
                depth += 1;
                let name = e.name();
                if item_depth.is_none() && is_item(name) {
                    item_depth = Some(depth);
                    best_for_item = None;
                } else if item_depth.is_some()
                    && encoded_depth.is_none()
                    && is_content_encoded(&resolved, name)
                {
                    encoded_depth = Some(depth);
                    captured.clear();
                }
            }
            Event::Empty(e) if item_depth.is_none() && is_item(e.name()) => {
                results.push(None);
            }
            Event::Text(t) if encoded_depth.is_some() => {
                if let Ok(text) = t.decode() {
                    captured.push_str(&text);
                }
            }
            Event::CData(t) if encoded_depth.is_some() => {
                if let Ok(text) = t.decode() {
                    captured.push_str(&text);
                }
            }
            Event::End(_) => {
                if encoded_depth == Some(depth) {
                    encoded_depth = None;
                    if captured.len() > best_for_item.as_ref().map_or(0, String::len) {
                        best_for_item = Some(std::mem::take(&mut captured));
                    } else {
                        captured.clear();
                    }
                }
                if item_depth == Some(depth) {
                    results.push(best_for_item.take());
                    item_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_the_longer_of_two_duplicate_content_encoded_elements() {
        // Mirrors the actual structure seen on thenewstack.io/blog/feed/:
        // a full-body content:encoded followed by a short one-liner
        // "dek" reusing the same tag, later in the same item.
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/">
<channel>
<item>
  <title>A</title>
  <content:encoded><![CDATA[<p>This is the real, long article body.</p>]]></content:encoded>
  <slash:comments>0</slash:comments>
  <content:encoded><![CDATA[Short dek.]]></content:encoded>
</item>
</channel>
</rss>"#;
        let result = longest_content_encoded_per_item(xml.as_bytes());
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            Some("<p>This is the real, long article body.</p>".to_string())
        );
    }

    #[test]
    fn single_content_encoded_is_returned_as_is() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/">
<channel>
<item>
  <title>A</title>
  <content:encoded><![CDATA[<p>Only body.</p>]]></content:encoded>
</item>
<item>
  <title>B</title>
  <description>No full text here, just a summary.</description>
</item>
</channel>
</rss>"#;
        let result = longest_content_encoded_per_item(xml.as_bytes());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], Some("<p>Only body.</p>".to_string()));
        assert_eq!(result[1], None);
    }

    #[test]
    fn unrelated_elements_named_encoded_are_ignored() {
        // A same-named "encoded" element in a different (or no)
        // namespace must not be mistaken for content:encoded.
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:other="https://example.com/ns">
<channel>
<item>
  <title>A</title>
  <other:encoded>not the content module</other:encoded>
</item>
</channel>
</rss>"#;
        let result = longest_content_encoded_per_item(xml.as_bytes());
        assert_eq!(result, vec![None]);
    }
}
