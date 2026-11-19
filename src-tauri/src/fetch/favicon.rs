//! Feed icons (docs/UI_SIMPLIFICATION_PLAN.md §3.12). The webview's CSP
//! only allows `data:` images, and loading icons straight from each site
//! would leak reading activity anyway - so the backend fetches each feed's
//! site icon once, stores the bytes, and the frontend renders them as
//! `data:` URIs.
//!
//! Lookup order for a feed: the site's own `<link rel="icon">` /
//! `apple-touch-icon` (from the site root of the feed URL), then
//! `/favicon.ico`. Responses are capped at [`MAX_ICON_BYTES`] and the image
//! type is decided by sniffing the bytes, never by the server's header.

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use super::client::shared_client;

pub const MAX_ICON_BYTES: usize = 64 * 1024;
/// Re-fetch a stored icon after this long (sites rebrand).
pub const REFRESH_AFTER_SECS: i64 = 30 * 86_400;
/// Retry a feed whose lookup found nothing after this long.
pub const RETRY_FAILED_AFTER_SECS: i64 = 7 * 86_400;

pub struct FeedIcon {
    pub mime: &'static str,
    pub data: Vec<u8>,
    pub source_url: String,
}

/// Finds and downloads an icon for the site a feed belongs to.
pub async fn fetch_for_feed(feed_url: &str) -> Option<FeedIcon> {
    let feed_url = Url::parse(feed_url).ok()?;
    let site_root = feed_url.join("/").ok()?;

    let mut candidates = Vec::new();
    if let Some(html) = fetch_limited(site_root.as_str(), 512 * 1024).await {
        candidates.extend(icon_links(&site_root, &String::from_utf8_lossy(&html)));
    }
    if let Ok(ico) = site_root.join("/favicon.ico") {
        candidates.push(ico);
    }

    for candidate in candidates {
        if let Some(bytes) = fetch_limited(candidate.as_str(), MAX_ICON_BYTES).await
            && let Some(mime) = sniff_image(&bytes)
        {
            return Some(FeedIcon {
                mime,
                data: bytes,
                source_url: candidate.to_string(),
            });
        }
    }
    None
}

/// GETs `url`, giving up (None) on a non-success status or a body larger
/// than `limit` bytes - read in chunks so an oversized body is never
/// buffered whole.
async fn fetch_limited(url: &str, limit: usize) -> Option<Vec<u8>> {
    let mut response = shared_client().get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|len| len as usize > limit)
    {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > limit {
            return None;
        }
    }
    Some(body)
}

static LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<link\b([^>]*)>").unwrap());
static REL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\brel\s*=\s*['"]([^'"]*)['"]"#).unwrap());
static HREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bhref\s*=\s*['"]([^'"]*)['"]"#).unwrap());

/// `<link rel="icon">`-style hrefs in the page, resolved against `base`:
/// plain `icon`/`shortcut icon` first (usually small), then
/// `apple-touch-icon` (larger, but common on sites without the former).
/// Attributes are matched independently, as in `discovery.rs`, since the
/// `regex` crate has no backreferences.
fn icon_links(base: &Url, html: &str) -> Vec<Url> {
    let mut icons = Vec::new();
    let mut touch_icons = Vec::new();
    for tag in LINK_RE.captures_iter(html) {
        let attrs = tag.get(1).map_or("", |m| m.as_str());
        let Some(rel) = REL_RE.captures(attrs).map(|c| c[1].to_ascii_lowercase()) else {
            continue;
        };
        let Some(url) = HREF_RE
            .captures(attrs)
            .and_then(|c| base.join(c[1].trim()).ok())
        else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        let tokens: Vec<&str> = rel.split_whitespace().collect();
        if tokens.contains(&"icon") {
            icons.push(url);
        } else if tokens
            .iter()
            .any(|token| token.starts_with("apple-touch-icon"))
        {
            touch_icons.push(url);
        }
    }
    icons.extend(touch_icons);
    icons
}

/// The image type from the content's magic bytes. SVG is accepted when the
/// document starts with an `<svg` root (optionally after an XML prolog);
/// the frontend only ever renders it through `<img src="data:…">`, where
/// scripts and external loads don't run.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if is_svg(bytes) {
        Some("image/svg+xml")
    } else {
        None
    }
}

fn is_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    let mut rest = head.trim_start_matches('\u{feff}').trim_start();
    if rest.starts_with("<?xml") {
        match rest.find("?>") {
            Some(end) => rest = rest[end + 2..].trim_start(),
            None => return false,
        }
    }
    // Comments or a doctype may sit between the prolog and the root.
    while rest.starts_with("<!--") || rest.starts_with("<!doctype") {
        let close = if rest.starts_with("<!--") { "-->" } else { ">" };
        match rest.find(close) {
            Some(end) => rest = rest[end + close.len()..].trim_start(),
            None => return false,
        }
    }
    rest.starts_with("<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_common_icon_formats() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff_image(&[0, 0, 1, 0, 1, 0]), Some("image/x-icon"));
        assert_eq!(sniff_image(b"GIF89a..."), Some("image/gif"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
    }

    #[test]
    fn accepts_svg_roots_and_rejects_html_and_text() {
        assert_eq!(
            sniff_image(b"<?xml version=\"1.0\"?>\n<!-- logo -->\n<svg xmlns=\"\"></svg>"),
            Some("image/svg+xml")
        );
        assert_eq!(
            sniff_image(b"  <SVG viewBox=\"0 0 1 1\"/>"),
            Some("image/svg+xml")
        );
        assert_eq!(sniff_image(b"<!doctype html><html><svg></svg>"), None);
        assert_eq!(sniff_image(b"Not Found"), None);
    }

    #[test]
    fn finds_icon_links_in_preference_order() {
        let base = Url::parse("https://blog.example/").unwrap();
        let html = r#"<head>
            <link href="/apple.png" rel="apple-touch-icon" sizes="180x180">
            <link rel="stylesheet" href="/site.css">
            <link rel="shortcut icon" href="https://cdn.example/fav.ico">
            <link rel='icon' type='image/svg+xml' href='icon.svg'>
            <link rel="icon" href="javascript:alert(1)">
        </head>"#;
        let links: Vec<String> = icon_links(&base, html)
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            links,
            [
                "https://cdn.example/fav.ico",
                "https://blog.example/icon.svg",
                "https://blog.example/apple.png",
            ]
        );
    }
}
