//! Best-effort extraction of article text that is only present inside a
//! client-side-rendered page's hydration payload - the JSON blob a
//! Nuxt/Next.js app embeds in `<script id="__NUXT_DATA__">` /
//! `<script id="__NEXT_DATA__">` so the client can re-render without a
//! second network round trip. On pages like this, `legible::parse`'s DOM
//! walk finds an empty SSR shell: the readable body never touched the
//! DOM at all, it only exists as an HTML string inside that one script.
//!
//! Nuxt's devalue format is still syntactically plain JSON - an array
//! where some sub-arrays are tagged with a type name and plain numbers
//! elsewhere are *references* (indices) into that same top-level array.
//! We don't reconstruct devalue's reference graph in general - we only
//! need to locate the one string that holds the real article body, which
//! `serde_json::Value` parses fine without understanding references.
//!
//! Known gaps, left unhandled deliberately: content stored as Markdown
//! or untagged plain-text paragraphs, or nested inside its own
//! JSON-encoded string value.

use regex::Regex;
use serde_json::Value;

pub struct HydratedArticle {
    pub title: Option<String>,
    pub content_html: String,
    pub text_content: String,
}

const MIN_TAG_HITS: usize = 3;
const MIN_TEXT_LEN: usize = 80;
/// Caps recursion in our own walkers *and* gates whether we call
/// `serde_json::from_str` at all: serde_json's `Value` deserializer
/// recurses on the call stack once per nesting level with no depth
/// limit of its own, so a few KB of `[[[[...]]]]` from an attacker-
/// influenced site is a stack-overflow vector before our code runs.
const MAX_JSON_DEPTH: usize = 128;
const MAX_TITLE_LEN: usize = 200;

pub fn extract(html: &str) -> Option<HydratedArticle> {
    let raw = find_hydration_json(html)?;
    if !nesting_depth_within_limit(raw, MAX_JSON_DEPTH) {
        return None;
    }
    let root: Value = serde_json::from_str(raw).ok()?;

    let body = match best_candidate(&root) {
        Some((s, _)) => s.to_string(),
        None => best_fragment_array(&root)?,
    };
    let text_content = strip_tags(&body);
    if text_content.trim().len() < MIN_TEXT_LEN {
        return None;
    }

    let title = find_title(&root, &body).or_else(|| extract_title_tag(html));
    Some(HydratedArticle {
        title,
        content_html: body,
        text_content,
    })
}

fn find_hydration_json(html: &str) -> Option<&str> {
    let script_re = Regex::new(
        r#"(?is)<script\b[^>]*\bid\s*=\s*['"](?:__NUXT_DATA__|__NEXT_DATA__)['"][^>]*>(.*?)</script>"#,
    )
    .unwrap();
    script_re.captures(html)?.get(1).map(|m| m.as_str())
}

/// Linear scan for the max `[`/`{` nesting depth, ignoring brackets
/// inside quoted strings (tracking `\"` escapes). Run *before*
/// `serde_json::from_str` - see `MAX_JSON_DEPTH`'s doc comment. ASCII
/// delimiter bytes never appear as part of a multi-byte UTF-8 sequence,
/// so scanning raw bytes (not chars) is safe even with non-ASCII text.
fn nesting_depth_within_limit(text: &str, limit: usize) -> bool {
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > limit {
                    return false;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    true
}

fn walk_strings<'a>(value: &'a Value, depth: usize, visit: &mut impl FnMut(&'a str)) {
    if depth > MAX_JSON_DEPTH {
        return;
    }
    match value {
        Value::String(s) => visit(s),
        Value::Array(items) => {
            for item in items {
                walk_strings(item, depth + 1, visit);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                walk_strings(item, depth + 1, visit);
            }
        }
        _ => {}
    }
}

fn walk_arrays<'a>(value: &'a Value, depth: usize, visit: &mut impl FnMut(&'a Vec<Value>)) {
    if depth > MAX_JSON_DEPTH {
        return;
    }
    match value {
        Value::Array(items) => {
            visit(items);
            for item in items {
                walk_arrays(item, depth + 1, visit);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                walk_arrays(item, depth + 1, visit);
            }
        }
        _ => {}
    }
}

/// Picks the JSON string most likely to be the real article body.
///
/// Ranked by *stripped text length*, not raw tag-hit count: a "related
/// articles" blurb list or a comments payload can rack up a high tag
/// count from many short fragments, but the real article body is
/// reliably the one with the most actual readable text once tags are
/// stripped - the same signal `legible`'s own text-length floor already
/// leans on.
fn best_candidate(root: &Value) -> Option<(&str, usize)> {
    let mut best: Option<(&str, usize)> = None;
    walk_strings(root, 0, &mut |s| {
        if s.len() < 40 {
            return; // cheap pre-filter before the tag_hits regex call
        }
        if tag_hits(s) < MIN_TAG_HITS {
            return;
        }
        let stripped_len = strip_tags(s).trim().len();
        if best.is_none_or(|(_, len)| stripped_len > len) {
            best = Some((s, stripped_len));
        }
    });
    best
}

/// Fallback when no single string clears the floor: some sites hydrate
/// content as separate per-block HTML fragments in one array rather
/// than one concatenated string, so no individual element clears
/// `MIN_TAG_HITS` on its own. Only tried when `best_candidate` finds
/// nothing, so a real single-string match is never second-guessed.
fn best_fragment_array(root: &Value) -> Option<String> {
    let mut best: Option<String> = None;
    walk_arrays(root, 0, &mut |items| {
        let fragments: Vec<&str> = items
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| s.trim_start().starts_with('<') && tag_hits(s) >= 1)
            .collect();
        if fragments.len() < 3 {
            return;
        }
        let joined = fragments.join("\n");
        if strip_tags(&joined).trim().len() < MIN_TEXT_LEN {
            return;
        }
        let joined_len = strip_tags(&joined).trim().len();
        let best_len = best.as_deref().map(|b| strip_tags(b).trim().len()).unwrap_or(0);
        if joined_len > best_len {
            best = Some(joined);
        }
    });
    best
}

/// Counts readable block-tag hits. The `\b` after the tag name is what
/// keeps this safe against false positives like `<path>`/`<picture>` -
/// there's no word boundary between `p` and the following word
/// character in either, so `p\b` never matches inside them.
fn tag_hits(s: &str) -> usize {
    let re =
        Regex::new(r"(?is)</?(p|h[1-6]|ul|ol|li|blockquote|pre|img|figure)\b").unwrap();
    re.find_iter(s).count()
}

fn strip_tags(value: &str) -> String {
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

/// Looks for the article's own title *inside* the hydration payload,
/// since `<title>` on an SSR shell is usually site-wide boilerplate.
fn find_title(root: &Value, content: &str) -> Option<String> {
    match root {
        Value::Array(items) => find_title_devalue(items, content),
        Value::Object(_) => find_title_sibling(root, content, 0),
        _ => None,
    }
    .and_then(validate_title)
}

/// Nuxt devalue shape: the metadata object is
/// `{"id":.., "title": <idx>, .., "content": <idx>, ..}` where the
/// values are indices into the same root array, not inline values.
fn find_title_devalue(items: &[Value], content: &str) -> Option<String> {
    let content_idx = items.iter().position(|v| v.as_str() == Some(content))?;
    for item in items {
        let Value::Object(map) = item else { continue };
        let matches_content = map
            .get("content")
            .and_then(Value::as_u64)
            .is_some_and(|idx| idx as usize == content_idx);
        if !matches_content {
            continue;
        }
        if let Some(title_idx) = map.get("title").and_then(Value::as_u64)
            && let Some(Value::String(title)) = items.get(title_idx as usize) {
                return Some(title.clone());
            }
    }
    None
}

/// Next.js shape: no reference indirection, so look for the object that
/// directly contains `content` as a field value and has a sibling
/// `"title"` string field.
fn find_title_sibling(value: &Value, content: &str, depth: usize) -> Option<String> {
    if depth > MAX_JSON_DEPTH {
        return None;
    }
    match value {
        Value::Object(map) => {
            if map.values().any(|v| v.as_str() == Some(content))
                && let Some(title) = map.get("title").and_then(Value::as_str) {
                    return Some(title.to_string());
                }
            map.values()
                .find_map(|v| find_title_sibling(v, content, depth + 1))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|v| find_title_sibling(v, content, depth + 1)),
        _ => None,
    }
}

fn validate_title(title: String) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_TITLE_LEN || tag_hits(trimmed) > 0 {
        return None;
    }
    Some(trimmed.to_string())
}

fn extract_title_tag(html: &str) -> Option<String> {
    let title_re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap();
    let raw = title_re.captures(html)?.get(1)?.as_str();
    let title = strip_tags(raw);
    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_devalue_content_among_short_unrelated_strings() {
        let html = r#"<html><head><title>tech.kakao.com</title></head><body>
<script type="application/json" data-nuxt-data="nuxt-app" data-ssr="true" id="__NUXT_DATA__">
[["ShallowReactive",1],{"data":2},{"post":3},
 {"id":4,"title":5,"content":6},
 846,
 "AI 에이전트를 위한 멀티모달 데이터 처리",
 "<p>카카오의 자체 멀티모달 모델은 이미지와 텍스트를 함께 처리합니다. 이 글에서는 그 구조를 설명합니다.</p><h2>배경</h2><p>최근 멀티모달 연구가 활발히 진행되고 있으며, 이번 포스트에서는 실제 서비스에 적용한 사례를 공유합니다.</p>",
 "kr", "2026-01-01", true, 42
]
</script>
</body></html>"#;
        let result = extract(html).expect("should find hydration content");
        assert!(result.content_html.contains("<h2>배경</h2>"));
        assert!(result.text_content.len() >= 80);
        assert_eq!(
            result.title,
            Some("AI 에이전트를 위한 멀티모달 데이터 처리".to_string())
        );
    }

    #[test]
    fn finds_next_data_content_in_plain_object_and_sibling_title() {
        let html = r#"<html><head><title>example.com</title></head><body>
<script id="__NEXT_DATA__" type="application/json">
{"props":{"pageProps":{"post":{"id":123,"title":"The Real Post Title","content":"<p>This is the first paragraph of the real article body, long enough to clear the floor easily.</p><h2>Section</h2><p>And a second paragraph with more real content to be sure.</p>","comments":["<p>nice post</p>","<p>thanks!</p>"]}}},"page":"/posts/[id]"}
</script>
</body></html>"#;
        let result = extract(html).expect("should find hydration content");
        assert!(result.content_html.starts_with("<p>This is the first paragraph"));
        assert_eq!(result.title, Some("The Real Post Title".to_string()));
    }

    #[test]
    fn returns_none_without_a_hydration_script_tag() {
        let html = "<html><head><title>Just a page</title></head><body><p>No hydration data here.</p></body></html>";
        assert!(extract(html).is_none());
    }

    #[test]
    fn returns_none_when_best_candidate_is_too_short() {
        let html = r#"<html><body>
<script id="__NEXT_DATA__" type="application/json">
{"post":{"title":"Tiny","content":"<p>Hi</p><h2>Yo</h2><p>Bye now</p><p>Ok</p><p>No</p>"}}
</script>
</body></html>"#;
        assert!(extract(html).is_none());
    }

    #[test]
    fn prefers_longest_stripped_text_over_higher_raw_tag_count() {
        // 10 tiny <p> pairs: 20 raw tag hits, ~35 chars of real text.
        let comments = "<p>hi</p><p>ok</p><p>cool</p><p>nice</p><p>wow</p><p>lol</p><p>great</p><p>yes</p><p>no</p><p>meh</p>";
        // 3 tags (6 hits) but far more real text - must win on length, not tag count.
        let article = "<p>This is a much longer paragraph that represents the real article body and should win because it has far more actual readable text content than the short comment list, even though it has fewer total tag hits overall in comparison.</p><h2>Conclusion</h2><p>A second paragraph to make sure this clears the floor comfortably as well.</p>";
        let html = format!(
            r#"<html><body><script id="__NEXT_DATA__" type="application/json">{{"comments":"{comments}","article":"{article}"}}</script></body></html>"#
        );
        let result = extract(&html).expect("should find hydration content");
        assert!(result.content_html.starts_with("<p>This is a much longer paragraph"));
    }

    #[test]
    fn concatenates_per_block_html_fragments_when_no_single_string_qualifies() {
        let html = r#"<html><body>
<script id="__NEXT_DATA__" type="application/json">
{"post":{"blocks":[
  "<p>This is the first fragment of a block-based article body with real content.</p>",
  "<h2>Middle heading for the article</h2>",
  "<p>This is the second fragment, also with a decent amount of real readable text.</p>"
]}}
</script>
</body></html>"#;
        let result = extract(html).expect("should assemble fragment array");
        assert!(result.content_html.contains("Middle heading"));
        assert!(result.text_content.len() >= 80);
    }

    #[test]
    fn returns_none_instead_of_overflowing_on_pathological_nesting() {
        let depth = 5_000;
        let nested = "[".repeat(depth) + "\"x\"" + &"]".repeat(depth);
        let html = format!(
            r#"<html><body><script id="__NEXT_DATA__" type="application/json">{nested}</script></body></html>"#
        );
        assert!(extract(&html).is_none());
    }
}
