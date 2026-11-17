//! Best-effort article-list extraction for public HTML pages without a feed.
//! Site layouts vary, so this intentionally extracts ordinary article links
//! and nearby machine-readable dates rather than trying to render JavaScript.
//! Tag/category links are excluded via path and attribute heuristics, and a
//! heading-like inner tag (`<h1>`-`<h3>`/`<strong>`) is preferred for the
//! title when present. Feed autodiscovery (following a `<link rel=
//! "alternate">` tag) is a deliberate non-goal of this fallback.

use chrono::{DateTime, NaiveDate};
use regex::Regex;
use sha2::{Digest, Sha256};
use url::Url;

use crate::db::NewItem;

pub struct WebpageListing {
    pub title: Option<String>,
    pub items: Vec<NewItem>,
}

pub fn parse(url: &str, bytes: &[u8], fetched_at: i64) -> Result<WebpageListing, String> {
    let html = String::from_utf8_lossy(bytes);
    let lower_html = html.to_ascii_lowercase();
    if !lower_html.contains("<html") && !lower_html.contains("<article") {
        return Err("URL did not return a supported feed or HTML page".into());
    }

    let base = Url::parse(url).map_err(|e| e.to_string())?;
    let title_re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap();
    let anchor_re =
        Regex::new(r#"(?is)<a\b([^>]*?)href\s*=\s*['"]([^'"]+)['"]([^>]*)>(.*?)</a\s*>"#).unwrap();
    let date_re = Regex::new(r#"(?is)<time\b[^>]*datetime\s*=\s*['"]([^'"]+)['"][^>]*>"#).unwrap();
    let visible_date_re = Regex::new(r"\b(20\d{2}[./-]\s*\d{1,2}[./-]\s*\d{1,2})\b").unwrap();
    // Taxonomy/archive path segments common across WordPress, Ghost, Hugo, Jekyll, Astro.
    let taxonomy_href_re =
        Regex::new(r"(?i)/(tags?|categor(?:y|ies)|topics?|labels?)/").unwrap();
    // rel="tag" is WordPress's the_tags() microformats2 convention; class="...tag.../
    // category.../topic.../label..." is the common theme convention.
    let taxonomy_attr_re = Regex::new(
        r#"(?i)\b(?:class|rel)\s*=\s*['"][^'"]*\b(?:tags?|categor\w*|topics?|labels?)\b[^'"]*['"]"#,
    )
    .unwrap();
    // Headline-like inner tag. The `regex` crate has no backreferences, so this
    // doesn't require the closing tag to match the opening one - acceptable for
    // best-effort HTML given the file's existing non-greedy `(.*?)` limits.
    let heading_re =
        Regex::new(r"(?is)<(?:h[1-3]|strong)\b[^>]*>(.*?)</(?:h[1-3]|strong)>").unwrap();
    let site_title = title_re
        .captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| clean_text(m.as_str()));
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for captures in anchor_re.captures_iter(&html) {
        let href = captures.get(2).unwrap().as_str().trim();
        let link = match base.join(href) {
            Ok(link) => link,
            Err(_) => continue,
        };
        if !matches!(link.scheme(), "http" | "https")
            || link.host_str() != base.host_str()
            || link.path() == base.path()
            || link.path() == "/"
        {
            continue;
        }
        let inner_html = captures.get(4).unwrap().as_str();
        let attrs = format!(
            "{} {}",
            captures.get(1).unwrap().as_str(),
            captures.get(3).unwrap().as_str()
        );

        // Exclude tag/category links. The path-based signal only fires when the
        // anchor also lacks article-like structure (no heading, no image), so a
        // real post published under e.g. `/topics/<slug>/` isn't misclassified.
        let has_heading_or_image = heading_re.is_match(inner_html)
            || inner_html.to_ascii_lowercase().contains("<img");
        let is_taxonomy_attr = taxonomy_attr_re.is_match(&attrs);
        let is_taxonomy_path = taxonomy_href_re.is_match(link.path()) && !has_heading_or_image;
        if is_taxonomy_attr || is_taxonomy_path {
            continue;
        }

        let raw_title = heading_re
            .captures(inner_html)
            .and_then(|h| h.get(1))
            .map(|m| m.as_str())
            .unwrap_or(inner_html);
        let title = strip_trailing_metadata(&clean_text(raw_title));
        if title.len() < 4 || title.len() > 400 {
            continue;
        }
        let normalized = link.as_str().trim_end_matches('/').to_string();
        if !seen.insert(normalized.clone()) {
            continue;
        }
        let id = Sha256::digest(normalized.as_bytes());

        // Many blog templates put <time datetime> immediately after the title
        // within the same card. Restrict the lookahead to the next 600 bytes.
        let end = captures.get(0).unwrap().end();
        let tail = String::from_utf8_lossy(&html.as_bytes()[end..html.len().min(end + 600)]);
        // Check inside the anchor first (same-card date), then fall back to the
        // after-</a> tail lookahead for templates that render the date outside it.
        let published_at = date_re
            .captures(inner_html)
            .and_then(|date| date.get(1))
            .and_then(|m| parse_date(m.as_str()))
            .or_else(|| {
                visible_date_re
                    .captures(inner_html)
                    .and_then(|date| date.get(1))
                    .and_then(|m| parse_date(m.as_str()))
            })
            .or_else(|| {
                date_re
                    .captures(&tail)
                    .and_then(|date| date.get(1))
                    .and_then(|m| parse_date(m.as_str()))
            })
            .or_else(|| {
                visible_date_re
                    .captures(&tail)
                    .and_then(|date| date.get(1))
                    .and_then(|m| parse_date(m.as_str()))
            })
            .unwrap_or(fetched_at);
        items.push(NewItem {
            guid_hash: format!("web:{}", hex(&id)),
            title: Some(title),
            link: Some(normalized),
            summary: None,
            full_content: None,
            published_at,
            fetched_at,
        });
        if items.len() >= 100 {
            break;
        }
    }
    if items.is_empty() {
        return Err("No article links found on this page".into());
    }
    Ok(WebpageListing {
        title: site_title.filter(|t| !t.is_empty()),
        items,
    })
}

fn parse_date(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|d| d.timestamp())
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp())
        })
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y. %-m. %-d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp())
        })
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y. %m. %d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp())
        })
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y/%m/%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp())
        })
}

fn clean_text(value: &str) -> String {
    let tag_re = Regex::new(r"(?is)<[^>]+>").unwrap();
    tag_re
        .replace_all(value, " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_trailing_metadata(title: &str) -> String {
    // Some cards render "<title> <date> <author>" with no dedicated heading
    // tag; once flattened by clean_text(), trim a trailing visible date and
    // anything after it so it doesn't get glued onto the headline.
    let trailing_date_re =
        Regex::new(r"(?i)\s+20\d{2}[./-]\s*\d{1,2}[./-]\s*\d{1,2}\b.*$").unwrap();
    trailing_date_re.replace(title, "").trim().to_string()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_tag_cloud_links_and_extracts_real_posts() {
        // Mirrors the actual structure seen on tech.kakaopay.com: post cards
        // (heading + <time> inside the same anchor) followed by a tag cloud
        // of plain `class="tag"` anchors.
        let html = r#"<html><head><title>Kakao Pay Tech</title></head><body>
<a href="/post/ifkakao26-ai-change-organization-preparation/">
  <div class="post-image"><img alt="if(kakao)26"></div>
  <strong>if(kakao)26에서 확인한 AI의 변화, 조직은 무엇을 준비해야 할까?</strong>
  <div class="extra"><time>2026. 11. 6</time> <span>카카오페이</span></div>
</a>
<a href="/post/second-post-example/">
  <div class="post-image"><img alt="second"></div>
  <strong>Second Example Post Title</strong>
  <div class="extra"><time>2026. 10. 1</time> <span>Author</span></div>
</a>
<a class="tag" href="/tag/dev-ops/">DevOps</a>
<a class="tag" href="/tag/i-os/">iOS</a>
<a class="tag" href="/tag/kotlin/">Kotlin</a>
</body></html>"#;

        let result = parse("https://tech.kakaopay.com/", html.as_bytes(), 1_700_000_000).unwrap();

        assert_eq!(result.title, Some("Kakao Pay Tech".to_string()));
        assert_eq!(
            result.items.len(),
            2,
            "tag cloud links must not be treated as articles"
        );
        assert!(result
            .items
            .iter()
            .all(|i| !i.link.as_ref().unwrap().contains("/tag/")));

        assert_eq!(
            result.items[0].title,
            Some("if(kakao)26에서 확인한 AI의 변화, 조직은 무엇을 준비해야 할까?".to_string())
        );
        assert_eq!(
            result.items[0].published_at,
            parse_date("2026. 11. 6").unwrap()
        );

        assert_eq!(
            result.items[1].title,
            Some("Second Example Post Title".to_string())
        );
        assert_eq!(
            result.items[1].published_at,
            parse_date("2026. 10. 1").unwrap()
        );
    }

    #[test]
    fn wordpress_style_rel_tag_attribute_is_excluded() {
        // rel="tag" is the microformats2 convention WordPress's the_tags()
        // emits; this must be excluded even without a `/tag/` path segment.
        let html = r#"<html><body>
<a href="/2024/05/wordpress-post">
  <h2>A Real WordPress Post Title</h2>
  <time datetime="2024-05-01">May 1</time>
</a>
<a href="/topic/rust" rel="tag">Rust</a>
</body></html>"#;

        let result = parse("https://example.com/", html.as_bytes(), 1_700_000_000).unwrap();

        assert_eq!(result.items.len(), 1);
        assert_eq!(
            result.items[0].title,
            Some("A Real WordPress Post Title".to_string())
        );
        assert_eq!(
            result.items[0].published_at,
            parse_date("2024-05-01").unwrap()
        );
    }

    #[test]
    fn taxonomy_path_without_article_structure_is_excluded() {
        // A flat pill-style anchor under /topics/ with no heading or image is
        // treated as a taxonomy link.
        let html = r#"<html><body>
<a href="/2024/05/real-post"><h3>Real Post Title</h3></a>
<a href="/topics/devops">DevOps</a>
</body></html>"#;

        let result = parse("https://example.com/", html.as_bytes(), 1_700_000_000).unwrap();

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].title, Some("Real Post Title".to_string()));
    }

    #[test]
    fn article_card_under_topics_path_is_not_excluded() {
        // Known tradeoff: some sites legitimately publish articles under
        // /topics/<slug>. The path heuristic alone would misfire here, so it
        // only fires when the anchor also lacks heading/image structure. A
        // real article card (heading + image) under /topics/ survives.
        let html = r#"<html><body>
<a href="/topics/why-rust-is-great">
  <img src="/img.jpg" alt="rust">
  <h2>Why Rust Is Great For Systems Programming</h2>
</a>
</body></html>"#;

        let result = parse("https://example.com/", html.as_bytes(), 1_700_000_000).unwrap();

        assert_eq!(result.items.len(), 1);
        assert_eq!(
            result.items[0].title,
            Some("Why Rust Is Great For Systems Programming".to_string())
        );
    }

    #[test]
    fn simple_blog_links_without_tags_or_headings_still_extract() {
        // Regression: the original generic fallback (plain <a> text as
        // title, no <time> anywhere, so published_at falls back to
        // fetched_at) must keep working for sites with no taxonomy markup.
        let html = r#"<html><head><title>My Simple Blog</title></head><body>
<a href="/2024/01/hello-world">Hello World: My First Post</a>
<a href="/2024/01/second-post">Second Post About Things</a>
</body></html>"#;

        let fetched_at = 1_700_000_000;
        let result = parse("https://example.com/", html.as_bytes(), fetched_at).unwrap();

        assert_eq!(result.title, Some("My Simple Blog".to_string()));
        assert_eq!(result.items.len(), 2);
        assert_eq!(
            result.items[0].title,
            Some("Hello World: My First Post".to_string())
        );
        assert_eq!(
            result.items[0].link,
            Some("https://example.com/2024/01/hello-world".to_string())
        );
        assert_eq!(result.items[0].published_at, fetched_at);
        assert_eq!(
            result.items[1].title,
            Some("Second Post About Things".to_string())
        );
        assert_eq!(result.items[1].published_at, fetched_at);
    }
}
