//! Feed autodiscovery, tried before falling back to the best-effort HTML
//! scraper in `webpage.rs`. Two strategies, in priority order:
//! 1. `<link rel="alternate" type="application/rss+xml|atom+xml|json...">`
//!    tags in the already-fetched HTML - an explicit, intentional signal.
//! 2. A short list of conventional feed paths at the site's origin root
//!    (`/feed`, `/rss.xml`, ...), since many sites serve a real feed at one
//!    of these without advertising it via a `<link>` tag at all.
//!
//! A candidate only counts as "found" if it parses as a feed *and* has at
//! least one entry - a syntactically valid but empty feed (seen in the
//! wild, e.g. some sites' `/rss.xml`) should not win over the scraper
//! fallback, which might still find real content.

use regex::Regex;
use url::Url;

use super::{client, parser};

pub struct DiscoveredFeed {
    pub url: String,
    pub bytes: Vec<u8>,
}

const CONVENTIONAL_PATHS: &[&str] = &[
    "/feed",
    "/feed/",
    "/rss",
    "/rss.xml",
    "/atom.xml",
    "/index.xml",
];

const MAX_CANDIDATES: usize = 8;

pub async fn discover(base_url: &str, html_bytes: &[u8]) -> Option<DiscoveredFeed> {
    discover_with(client::shared_client(), base_url, html_bytes).await
}

async fn discover_with(
    client: &reqwest::Client,
    base_url: &str,
    html_bytes: &[u8],
) -> Option<DiscoveredFeed> {
    let base = Url::parse(base_url).ok()?;
    let html = String::from_utf8_lossy(html_bytes);
    let candidates = candidate_urls(&base, &html);
    if candidates.is_empty() {
        return None;
    }

    let mut set = tokio::task::JoinSet::new();
    for (idx, url) in candidates.into_iter().enumerate() {
        let client = client.clone();
        set.spawn(async move {
            let bytes = client::fetch_bytes_with(&client, url.as_str()).await.ok()?;
            let has_entries = parser::parse(&bytes)
                .map(|f| !f.entries.is_empty())
                .unwrap_or(false);
            has_entries.then(|| (idx, url.to_string(), bytes))
        });
    }

    let mut hits = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(Some(hit)) = res {
            hits.push(hit);
        }
    }
    // Priority order (link-tag candidates, then conventional paths) wins
    // regardless of which network response landed first.
    hits.sort_by_key(|(idx, _, _)| *idx);
    hits.into_iter()
        .next()
        .map(|(_, url, bytes)| DiscoveredFeed { url, bytes })
}

/// `<link>` tags whose `rel` contains "alternate" and whose `type` is a
/// known feed mime type, in document order. The `regex` crate has no
/// backreferences, so attribute order within the tag doesn't matter - each
/// attribute is matched independently against the tag's full attribute text.
fn extract_link_candidates(base: &Url, html: &str) -> Vec<Url> {
    let link_re = Regex::new(r"(?is)<link\b([^>]*)>").unwrap();
    let rel_re = Regex::new(r#"(?i)\brel\s*=\s*['"]([^'"]*)['"]"#).unwrap();
    let type_re = Regex::new(r#"(?i)\btype\s*=\s*['"]([^'"]*)['"]"#).unwrap();
    let href_re = Regex::new(r#"(?i)\bhref\s*=\s*['"]([^'"]*)['"]"#).unwrap();

    let mut out = Vec::new();
    for tag in link_re.captures_iter(html) {
        let attrs = tag.get(1).unwrap().as_str();

        let is_alternate = rel_re
            .captures(attrs)
            .map(|c| c[1].to_ascii_lowercase())
            .is_some_and(|rel| rel.split_whitespace().any(|token| token == "alternate"));
        if !is_alternate {
            continue;
        }

        let is_feed_type = type_re
            .captures(attrs)
            .map(|c| c[1].to_ascii_lowercase())
            .is_some_and(|t| {
                matches!(
                    t.as_str(),
                    "application/rss+xml"
                        | "application/atom+xml"
                        | "application/json"
                        | "application/feed+json"
                )
            });
        if !is_feed_type {
            continue;
        }

        if let Some(href) = href_re.captures(attrs).map(|c| c[1].to_string())
            && let Ok(url) = base.join(&href)
        {
            out.push(url);
        }
    }
    out
}

/// Resolved against the site's origin root, not the given page's path -
/// e.g. `https://example.com/blog?page=1` -> `https://example.com/feed`,
/// not `https://example.com/blog/feed`. This is the overwhelmingly common
/// convention across blog platforms (WordPress, Ghost, Hugo, Jekyll, and
/// Nuxt-based blogs).
fn conventional_candidates(base: &Url) -> Vec<Url> {
    let mut root = base.clone();
    root.set_path("/");
    root.set_query(None);
    CONVENTIONAL_PATHS
        .iter()
        .filter_map(|p| root.join(p).ok())
        .collect()
}

fn candidate_urls(base: &Url, html: &str) -> Vec<Url> {
    let base_key = base.as_str().trim_end_matches('/').to_string();
    let mut seen = std::collections::HashSet::new();
    seen.insert(base_key);

    let mut out = Vec::new();
    for url in extract_link_candidates(base, html)
        .into_iter()
        .chain(conventional_candidates(base))
    {
        let key = url.as_str().trim_end_matches('/').to_string();
        if seen.insert(key) {
            out.push(url);
        }
        if out.len() >= MAX_CANDIDATES {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[test]
    fn extract_link_candidates_finds_feed_links_and_ignores_stylesheet() {
        let base = Url::parse("https://example.com/blog").unwrap();
        let html = r#"
            <link rel="stylesheet" href="/style.css">
            <link rel="alternate" type="application/rss+xml" href="/custom-feed.xml">
            <link rel="alternate" type="application/atom+xml" href="https://cdn.example.com/atom">
        "#;

        let candidates = extract_link_candidates(&base, html);

        assert_eq!(
            candidates,
            vec![
                Url::parse("https://example.com/custom-feed.xml").unwrap(),
                Url::parse("https://cdn.example.com/atom").unwrap(),
            ]
        );
    }

    #[test]
    fn conventional_candidates_resolve_against_origin_root() {
        let base = Url::parse("https://example.com/blog?page=1").unwrap();

        let candidates = conventional_candidates(&base);

        assert!(candidates.contains(&Url::parse("https://example.com/feed").unwrap()));
        assert!(!candidates.iter().any(|u| u.as_str().contains("/blog/feed")));
    }

    #[test]
    fn candidate_urls_dedupes_and_excludes_base_url() {
        let base = Url::parse("https://example.com/blog").unwrap();
        let html = r#"<link rel="alternate" type="application/rss+xml" href="/feed">"#;

        let candidates = candidate_urls(&base, html);

        assert_eq!(
            candidates.iter().filter(|u| u.path() == "/feed").count(),
            1,
            "the <link>-discovered /feed and the conventional /feed candidate must dedupe"
        );
        assert!(!candidates.iter().any(|u| u == &base));
    }

    fn rss_with_items(count: usize) -> String {
        let items = (0..count)
            .map(|i| {
                format!("<item><title>Post {i}</title><link>https://example.com/{i}</link></item>")
            })
            .collect::<String>();
        format!(
            r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Example</title>{items}</channel></rss>"#
        )
    }

    #[tokio::test]
    async fn prefers_link_tag_feed_over_conventional_path() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/custom-feed.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(rss_with_items(1)))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/feed"))
            .respond_with(ResponseTemplate::new(200).set_body_string(rss_with_items(1)))
            .mount(&mock_server)
            .await;

        let base = mock_server.uri();
        let html = format!(
            r#"<link rel="alternate" type="application/rss+xml" href="{base}/custom-feed.xml">"#
        );

        let result = discover_with(&reqwest::Client::new(), &base, html.as_bytes())
            .await
            .unwrap();

        assert_eq!(result.url, format!("{base}/custom-feed.xml"));
    }

    #[tokio::test]
    async fn falls_back_to_conventional_path_when_no_link_tag() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/feed"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/rss.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(rss_with_items(1)))
            .mount(&mock_server)
            .await;

        let base = mock_server.uri();
        let result = discover_with(&reqwest::Client::new(), &base, b"<html></html>")
            .await
            .unwrap();

        assert_eq!(result.url, format!("{base}/rss.xml"));
    }

    #[tokio::test]
    async fn skips_empty_feed_and_returns_none() {
        // Mirrors tech.kakaopay.com/rss.xml: a syntactically valid feed
        // with zero items must not count as "found".
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rss.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(rss_with_items(0)))
            .mount(&mock_server)
            .await;

        let base = mock_server.uri();
        let result = discover_with(&reqwest::Client::new(), &base, b"<html></html>").await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn returns_none_when_all_candidates_404() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let base = mock_server.uri();
        let result = discover_with(&reqwest::Client::new(), &base, b"<html></html>").await;

        assert!(result.is_none());
    }
}
