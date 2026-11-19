//! Comparable web URL identity; navigation URLs keep their original slash/query.
use url::Url;

pub fn key(value: &str) -> Option<String> {
    let mut url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_self_variants_without_merging_query_articles() {
        assert_eq!(
            key("https://example.com/blog/#top"),
            key("https://example.com/blog")
        );
        assert_ne!(
            key("https://example.com/post?id=1"),
            key("https://example.com/post?id=2")
        );
        assert_eq!(key("mailto:hello@example.com"), None);
        assert_eq!(
            key("https://example.com/%E3%81%82/"),
            Some("https://example.com/%E3%81%82".into())
        );
    }
}
