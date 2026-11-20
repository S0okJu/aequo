//! Article-list extraction using DOM card boundaries, never adjacent byte ranges.
use crate::db::{NewItem, url_identity};
use chrono::{DateTime, NaiveDate};
use dom_query::{Document, NodeRef, Selection};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::OnceLock;
use url::Url;

pub const PARSER_VERSION: i64 = 1;
const HEADINGS: &str = "h1,h2,h3,h4,h5,h6,strong";
const CARDS: &str = "article,.article-excerpt,.post-card,.blog-card,.post-item,.entry,.h-entry,[itemtype$='BlogPosting']";
const META: &str = "[itemprop='datePublished'],.article-excerpt-meta,.post-meta,.entry-meta,.published,.post-date,.date";
// `dd` too: a `<dl><dt>등록일</dt><dd>2026.11.19</dd></dl>` metadata list (seen on
// d2.naver.com/helloworld) carries its date in an unclassed `<dd>`, not a
// `<span>`/`<time>`. A non-date `<dd>` (e.g. a view count) alongside it is
// harmless here - `visible_date`'s regex still has to match before it's
// treated as one.
const DATE_HINT: &str = "time,.post-date,.date,dd";

pub struct WebpageListing {
    pub title: Option<String>,
    pub items: Vec<NewItem>,
    /// Positively identified navigation/metadata URLs. Absence is never rejection.
    pub rejected_links: Vec<String>,
}

fn selection(node: NodeRef<'_>) -> Selection<'_> {
    Selection::from(node)
}
fn text(node: NodeRef<'_>) -> String {
    node.text().split_whitespace().collect::<Vec<_>>().join(" ")
}
fn is_heading(node: NodeRef<'_>) -> bool {
    selection(node).is(HEADINGS)
}
fn is_metadata(node: NodeRef<'_>) -> bool {
    let s = selection(node);
    s.is("[rel~='author'],[rel~='tag'],.author,.byline,.tag,.category,.article-excerpt-meta,.post-meta,.entry-meta")
}
fn excluded(node: NodeRef<'_>) -> bool {
    std::iter::once(node)
        .chain(node.ancestors_it(None))
        .any(|n| {
            let s = selection(n);
            s.is("nav,footer,aside,script,style,template,[role='navigation'],[role='contentinfo']")
            || is_metadata(n)
            // A header inside a card can contain its real headline.
            || (s.is("header") && !n.ancestors_it(None).any(|a| selection(a).is(CARDS)))
        })
}
fn card_for(node: NodeRef<'_>) -> Option<NodeRef<'_>> {
    node.ancestors_it(None)
        .find(|n| selection(*n).is(CARDS))
        .or_else(|| {
            // Unnamed repeated card wrappers (including a whole-card anchor).
            std::iter::once(node)
                .chain(node.ancestors_it(Some(4)))
                .find(|n| {
                    let s = selection(*n);
                    !s.is("body,html,main,section")
                        && !s.select(HEADINGS).is_empty()
                        && !s.select(DATE_HINT).is_empty()
                        && s.select("h1,h2,h3,h4,h5,h6,strong").nodes().len() == 1
                })
        })
}
fn heading_for(node: NodeRef<'_>) -> Option<NodeRef<'_>> {
    selection(node)
        .select(HEADINGS)
        .nodes()
        .first()
        .copied()
        .or_else(|| node.ancestors_it(Some(3)).find(|n| is_heading(*n)))
}

/// Returns (unix seconds, timestamp/date precision). Ambiguous dates stay unknown.
fn date_value(value: &str, day_first: Option<bool>) -> Option<(i64, &'static str)> {
    let value = value.trim();
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Some((date.timestamp(), "timestamp"));
    }
    static SPACE: OnceLock<Regex> = OnceLock::new();
    let compact = SPACE
        .get_or_init(|| Regex::new(r"\s+").unwrap())
        .replace_all(value, " ")
        .to_string();
    for format in [
        "%Y-%m-%d",
        "%Y/%m/%d",
        "%Y. %m. %d",
        "%Y.%m.%d",
        "%b %d, %Y",
        "%B %d, %Y",
    ] {
        if let Ok(date) = NaiveDate::parse_from_str(&compact, format) {
            return Some((date.and_hms_opt(0, 0, 0)?.and_utc().timestamp(), "date"));
        }
    }
    let parts: Vec<_> = value.split('/').collect();
    if parts.len() == 3 && parts[2].len() == 4 {
        let a: u32 = parts[0].parse().ok()?;
        let b: u32 = parts[1].parse().ok()?;
        let order = if a > 12 {
            Some(true)
        } else if b > 12 {
            Some(false)
        } else if a == b {
            Some(true)
        } else {
            day_first
        }?;
        let (day, month) = if order { (a, b) } else { (b, a) };
        let date = NaiveDate::from_ymd_opt(parts[2].parse().ok()?, month, day)?;
        return Some((date.and_hms_opt(0, 0, 0)?.and_utc().timestamp(), "date"));
    }
    None
}
fn visible_date(value: &str) -> Option<String> {
    static VISIBLE: OnceLock<Regex> = OnceLock::new();
    let re = VISIBLE.get_or_init(||Regex::new(r"(?i)\b(?:20\d{2}[-/]\d{1,2}[-/]\d{1,2}|20\d{2}\.\s*\d{1,2}\.\s*\d{1,2}|\d{1,2}/\d{1,2}/20\d{2}|(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:tember)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)\s+\d{1,2},\s+20\d{2})\b").unwrap());
    re.find(value).map(|m| m.as_str().to_string())
}
fn is_updated(node: NodeRef<'_>, card: NodeRef<'_>) -> bool {
    std::iter::once(node)
        .chain(node.ancestors_it(None).take_while(|a| a.id != card.id))
        .any(|n| selection(n).is(".updated,.modified,[itemprop='dateModified']"))
}
fn date_texts(card: NodeRef<'_>) -> Vec<String> {
    let s = selection(card);
    let explicit = s.select(META);
    if !explicit.is_empty() {
        return explicit
            .nodes()
            .iter()
            .filter(|n| !is_updated(**n, card))
            .map(|n| text(*n))
            .collect();
    }
    // Kyverno featured card uses a direct span; do not scan headline/summary text.
    s.select("span,time,dd")
        .nodes()
        .iter()
        .filter(|n| {
            !is_updated(**n, card)
                && !n
                    .ancestors_it(None)
                    .take_while(|a| a.id != card.id)
                    .any(|a| is_heading(a) || selection(a).is("p"))
        })
        .map(|n| text(*n))
        .collect()
}
fn card_date(card: NodeRef<'_>, day_first: Option<bool>) -> Option<(i64, &'static str)> {
    let s = selection(card);
    for el in s.select("time[datetime],[itemprop='datePublished']").iter() {
        if el.nodes().first().is_some_and(|n| is_updated(*n, card)) {
            continue;
        }
        let raw = el
            .attr("datetime")
            .or_else(|| el.attr("content"))
            .unwrap_or_else(|| el.text());
        if let Some(date) = date_value(&raw, day_first) {
            return Some(date);
        }
    }
    date_texts(card)
        .iter()
        .filter_map(|t| visible_date(t))
        .find_map(|d| date_value(&d, day_first))
}

pub fn parse(url: &str, bytes: &[u8], fetched_at: i64) -> Result<WebpageListing, String> {
    let html = String::from_utf8_lossy(bytes);
    let lower = html.to_ascii_lowercase();
    if !lower.contains("<html") && !lower.contains("<article") {
        return Err("URL did not return a supported feed or HTML page".into());
    }
    let base = Url::parse(url).map_err(|e| e.to_string())?;
    let base_key = url_identity::key(url);
    let doc = Document::from(html.as_ref());
    let site_title = doc
        .select("title")
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let anchors = doc.select("a[href]");
    let has_cards = anchors
        .nodes()
        .iter()
        .any(|a| !excluded(*a) && heading_for(*a).is_some());
    // Infer locale only from explicit date metadata, never a title's numbers.
    let mut orders = HashSet::new();
    for card in doc.select(CARDS).nodes() {
        for raw in date_texts(*card).iter().filter_map(|s| visible_date(s)) {
            let p: Vec<_> = raw.split('/').collect();
            if p.len() == 3 && p[2].len() == 4 {
                let a = p[0].parse::<u32>().unwrap_or(0);
                let b = p[1].parse::<u32>().unwrap_or(0);
                if a > 12 && b <= 12 {
                    orders.insert(true);
                } else if b > 12 && a <= 12 {
                    orders.insert(false);
                }
            }
        }
    }
    let day_first = if orders.len() == 1 {
        orders.iter().next().copied()
    } else {
        None
    };
    let taxonomy = Regex::new(r"(?i)/(tags?|categor(?:y|ies)|topics?|labels?|authors?)/").unwrap();
    let mut seen = HashSet::new();
    let mut rejected = HashSet::new();
    let mut items = Vec::new();
    for node in anchors.nodes() {
        let href = node.attr("href").unwrap();
        let Ok(mut link) = base.join(&href) else {
            continue;
        };
        if link.host_str() != base.host_str() || !matches!(link.scheme(), "http" | "https") {
            continue;
        }
        link.set_fragment(None);
        let Some(key) = url_identity::key(link.as_str()) else {
            continue;
        };
        let heading = heading_for(*node);
        let structured = heading.is_some() || !selection(*node).select("img").is_empty();
        if excluded(*node)
            || Some(&key) == base_key.as_ref()
            || link.path() == "/"
            || (taxonomy.is_match(link.path()) && !structured)
        {
            rejected.insert(key);
            continue;
        }
        // If real cards exist, a bare link isn't an article. It is not positive
        // rejection evidence: older articles can disappear from a listing.
        if has_cards && heading.is_none() {
            continue;
        }
        let title = heading.map(text).unwrap_or_else(|| text(*node));
        let title = if heading.is_some() {
            title
        } else {
            strip_trailing_metadata(&title)
        };
        if title.chars().count() < 4 || title.chars().count() > 400 || !seen.insert(key.clone()) {
            continue;
        }
        if items.len() >= 100 {
            continue;
        }
        let card = card_for(*node).or(if structured { Some(*node) } else { None });
        let date = card.and_then(|c| card_date(c, day_first));
        let (published_at, published_at_kind) = date.unwrap_or((fetched_at, "discovered"));
        items.push(NewItem {
            guid_hash: format!("web:{}", hex(&Sha256::digest(key.as_bytes()))),
            title: Some(title),
            link: Some(link.to_string()),
            summary: None,
            full_content: None,
            published_at,
            published_at_kind,
            fetched_at,
        });
        // Continue scanning positively rejected links after the article cap.
    }
    // A navigation duplicate of an accepted article must never hide it.
    for key in &seen {
        rejected.remove(key);
    }
    if items.is_empty() {
        return Err("No article links found on this page".into());
    }
    Ok(WebpageListing {
        title: (!site_title.is_empty()).then_some(site_title),
        items,
        rejected_links: rejected.into_iter().collect(),
    })
}
#[cfg(test)]
fn parse_date(value: &str) -> Option<i64> {
    date_value(value, None).map(|d| d.0)
}
fn strip_trailing_metadata(title: &str) -> String {
    static TRAILING: OnceLock<Regex> = OnceLock::new();
    TRAILING
        .get_or_init(|| Regex::new(r"(?i)\s+20\d{2}[./-]\s*\d{1,2}[./-]\s*\d{1,2}\b.*$").unwrap())
        .replace(title, "")
        .trim()
        .to_string()
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
        assert!(
            result
                .items
                .iter()
                .all(|i| !i.link.as_ref().unwrap().contains("/tag/"))
        );

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

#[cfg(test)]
mod listing_regressions {
    use super::*;
    #[test]
    fn real_listings_keep_only_articles_and_restore_dates() {
        let kyverno = parse(
            "https://kyverno.io/blog/",
            include_bytes!("../../tests/fixtures/kyverno_blog.html"),
            1_800_000_000,
        )
        .unwrap();
        assert_eq!(kyverno.items.len(), 43);
        assert!(
            kyverno
                .items
                .iter()
                .all(|i| i.published_at_kind != "discovered")
        );
        assert_eq!(
            kyverno.items[0].title.as_deref(),
            Some("Announcing Kyverno 1.20!")
        );
        assert_eq!(
            kyverno.items[0].published_at,
            parse_date("2026-11-12").unwrap()
        );
        assert!(
            kyverno
                .rejected_links
                .contains(&"https://kyverno.io/blog".into())
        );
        let kata = parse(
            "https://katacontainers.io/blog/",
            include_bytes!("../../tests/fixtures/kata_blog.html"),
            1_800_000_000,
        )
        .unwrap();
        assert_eq!(kata.items.len(), 68);
        assert!(kata.items.iter().all(|i| i.published_at_kind == "date"));
        assert_eq!(
            kata.items[0].published_at,
            parse_date("2026-11-03").unwrap()
        );
        assert_eq!(
            kata.items.last().unwrap().published_at,
            parse_date("2019-02-20").unwrap()
        );
        let weekly = kata
            .items
            .iter()
            .find(|i| i.title.as_deref().unwrap().starts_with("4/18,"))
            .unwrap();
        assert_eq!(weekly.published_at, parse_date("2019-04-22").unwrap());
    }
    #[test]
    fn a_dl_dt_dd_metadata_list_still_yields_real_dates() {
        // d2.naver.com/helloworld puts each post's date in a plain,
        // unclassed `<dd>` next to a view count - no `<time>`, `.date`, or
        // similar class anywhere. Regression for DATE_HINT/date_texts not
        // looking at `dd` at all.
        let d2 = parse(
            "https://d2.naver.com/helloworld",
            include_bytes!("../../tests/fixtures/d2_naver_helloworld.html"),
            1_800_000_000,
        )
        .unwrap();
        let dated: Vec<_> = d2
            .items
            .iter()
            .filter(|i| i.published_at_kind == "date")
            .collect();
        // The header's own "naver D2" home-link is a known, harmless
        // false positive (an <h1>-wrapped site logo, not an article), so
        // this allows for exactly one undated outlier rather than requiring
        // every single item to have a real date.
        assert!(
            dated.len() >= d2.items.len() - 1,
            "expected almost every item to have a real date, got {}/{}",
            dated.len(),
            d2.items.len()
        );
        let first_dated = d2
            .items
            .iter()
            .find(|i| i.published_at_kind == "date")
            .unwrap();
        assert_eq!(first_dated.published_at, parse_date("2026.11.19").unwrap());
    }
    #[test]
    fn card_boundaries_headers_aliases_queries_and_ambiguous_dates() {
        let html = r#"<html><header><a href='/docs'>Documentation</a></header><main>
        <article><header><h5><a href='/blog/first/'>First Article</a></h5></header><p>Posted 2020-01-01 in a quote</p></article>
        <article><h2><a href='/blog/second'>Second Article</a></h2><time datetime='2025-04-01'>April</time></article>
        <article><h2><a href='/blog/third'>Third Article</a></h2><div class='post-meta'>03/04/2025</div></article>
        <article><h2><a href='/blog/third#more'>Third Article</a></h2></article>
        <article><h2><a href='/blog/post?id=1'>Query First Article</a></h2></article>
        <article><h2><a href='/blog/post?id=2'>Query Second Article</a></h2></article>
        </main><footer><a href='/blog'>Blog</a></footer></html>"#;
        let listing = parse("https://example.com/blog/", html.as_bytes(), 999).unwrap();
        assert_eq!(listing.items.len(), 5);
        assert_eq!(listing.items[0].published_at_kind, "discovered");
        assert_eq!(
            listing.items[1].published_at,
            parse_date("2025-04-01").unwrap()
        );
        assert_eq!(listing.items[2].published_at_kind, "discovered");
        assert_eq!(
            listing.items[0].link.as_deref(),
            Some("https://example.com/blog/first/")
        );
        assert_ne!(listing.items[3].guid_hash, listing.items[4].guid_hash);
        for base in ["https://example.com/blog", "https://example.com/blog/"] {
            assert!(
                !parse(base, html.as_bytes(), 999)
                    .unwrap()
                    .items
                    .iter()
                    .any(|i| i.title.as_deref() == Some("Blog"))
            );
        }
    }
}

#[cfg(test)]
mod limits_and_metadata_tests {
    use super::*;
    #[test]
    fn article_limit_excludes_metadata_and_still_identifies_footer_noise() {
        let mut html = String::from("<html><main>");
        for i in 0..110 {
            html.push_str(&format!("<article><h5><a href='/post/{i}'>Article {i} dated 2026-01-01</a></h5><span class='updated'><time datetime='2026-10-11'>Oct 11</time></span><p>Summary from 2026-10-10</p><a rel='author' href='/author/a'>Author</a></article>"));
        }
        html.push_str("</main><footer><a href='/docs/'>Documentation</a></footer></html>");
        let listing = parse("https://example.com/blog/", html.as_bytes(), 999).unwrap();
        assert_eq!(listing.items.len(), 100);
        assert!(
            listing
                .items
                .iter()
                .all(|i| i.published_at_kind == "discovered")
        );
        assert!(
            listing.items[0]
                .title
                .as_deref()
                .unwrap()
                .ends_with("2026-01-01")
        );
        assert!(
            listing
                .rejected_links
                .contains(&"https://example.com/docs".into())
        );
    }
}
