//! Pre-fetches full article content for items missing it, as part of
//! the normal poll/add cycle rather than waiting for a user to open the
//! post. Pure in-memory + network - the caller is responsible for the
//! one DB read (`db::guid_hashes_with_full_content`) that decides which
//! candidates to skip, since that needs a connection this module never
//! touches (keeping it lock-free makes it safe to `.await` freely).
//!
//! Bounded on three axes so one feed's backlog can't stall a poll
//! cycle: at most [`MAX_ITEMS_PER_CYCLE`] candidates (newest
//! `published_at` first - a feed with a larger backlog catches up over
//! several cycles), at most [`ENRICH_CONCURRENCY`] fetches in flight at
//! once, and each fetch capped at [`ENRICH_TIMEOUT`]. Failures and
//! timeouts just leave `full_content` as `None` - `load_article`'s
//! on-demand path or a later cycle retries it, this never fails the
//! surrounding poll.

use std::collections::HashSet;
use std::time::Duration;

use crate::db::NewItem;
use crate::resource_policy::{ResourcePolicy, WorkKind};

use super::article_fetch;

const MAX_ITEMS_PER_CYCLE: usize = 8;
const ENRICH_CONCURRENCY: usize = 3;
const ENRICH_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn enrich_missing_full_content(
    items: &mut [NewItem],
    has_content: &HashSet<String>,
    policy: &ResourcePolicy,
) {
    if !policy.settings().prefetch_full_text || !policy.optional_allowed() {
        return;
    }
    let mut candidates: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.full_content.is_none()
                && item.link.is_some()
                && !has_content.contains(&item.guid_hash)
        })
        .map(|(i, _)| i)
        .collect();
    candidates.sort_by_key(|&i| std::cmp::Reverse(items[i].published_at));
    candidates.truncate(MAX_ITEMS_PER_CYCLE);

    let mut set = tokio::task::JoinSet::new();
    for idx in candidates {
        while set.len() >= ENRICH_CONCURRENCY {
            if let Some(joined) = set.join_next().await {
                apply_result(items, joined);
            }
        }
        let link = items[idx].link.clone().unwrap();
        let policy = policy.clone();
        set.spawn(async move {
            let Some(_permit) = policy.acquire(WorkKind::Article, true).await else {
                return (idx, Ok(Ok(None)));
            };
            let result = tokio::time::timeout(ENRICH_TIMEOUT, fetch_one(&link)).await;
            (idx, result)
        });
    }
    while let Some(joined) = set.join_next().await {
        apply_result(items, joined);
    }
}

/// The real, SSRF-guarded fetch in production; swapped for a localhost-
/// permitting variant under tests (`fetch_and_extract_for_tests`) since
/// wiremock's mock server always runs on loopback, which the real path
/// correctly refuses to fetch.
#[cfg(not(test))]
async fn fetch_one(link: &str) -> Result<Option<article_fetch::ExtractedArticle>, String> {
    article_fetch::fetch_and_extract(link, None, None).await
}

#[cfg(test)]
async fn fetch_one(link: &str) -> Result<Option<article_fetch::ExtractedArticle>, String> {
    article_fetch::fetch_and_extract_for_tests(link).await
}

type EnrichOutcome = (
    usize,
    Result<Result<Option<article_fetch::ExtractedArticle>, String>, tokio::time::error::Elapsed>,
);

fn apply_result(items: &mut [NewItem], joined: Result<EnrichOutcome, tokio::task::JoinError>) {
    if let Ok((idx, Ok(Ok(Some(article))))) = joined {
        items[idx].full_content = Some(article.content_html);
    }
    // Any other case (timeout, fetch error, 404, panic) leaves
    // full_content untouched - already None, nothing to undo.
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn item(guid_hash: &str, link: Option<String>, published_at: i64) -> NewItem {
        NewItem {
            published_at_kind: "timestamp",
            guid_hash: guid_hash.to_string(),
            title: Some("Title".to_string()),
            link,
            summary: None,
            full_content: None,
            published_at,
            fetched_at: published_at,
        }
    }

    fn article_html() -> String {
        r#"<html><head><title>Example</title></head><body>
<article><p>This is a long enough paragraph of real article text to clear the readability floor comfortably.</p></article>
</body></html>"#
            .to_string()
    }

    #[tokio::test]
    async fn populates_full_content_on_success() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(article_html(), "text/html"))
            .mount(&mock_server)
            .await;

        let mut items = vec![item(
            "guid-1",
            Some(format!("{}/post", mock_server.uri())),
            1_700_000_000,
        )];
        enrich_missing_full_content(
            &mut items,
            &HashSet::new(),
            &ResourcePolicy::with_prefetch_for_tests(),
        )
        .await;

        assert!(
            items[0]
                .full_content
                .as_ref()
                .unwrap()
                .contains("long enough paragraph")
        );
    }

    #[tokio::test]
    async fn skips_items_already_marked_as_having_content() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(article_html(), "text/html"))
            .expect(0)
            .mount(&mock_server)
            .await;

        let mut items = vec![item(
            "guid-1",
            Some(format!("{}/post", mock_server.uri())),
            1_700_000_000,
        )];
        let mut has_content = HashSet::new();
        has_content.insert("guid-1".to_string());

        enrich_missing_full_content(
            &mut items,
            &has_content,
            &ResourcePolicy::with_prefetch_for_tests(),
        )
        .await;

        assert!(items[0].full_content.is_none());
    }

    #[tokio::test]
    async fn skips_items_without_a_link() {
        let mut items = vec![item("guid-1", None, 1_700_000_000)];
        enrich_missing_full_content(
            &mut items,
            &HashSet::new(),
            &ResourcePolicy::with_prefetch_for_tests(),
        )
        .await;
        assert!(items[0].full_content.is_none());
    }

    #[tokio::test]
    async fn failed_fetch_leaves_full_content_none() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let mut items = vec![item(
            "guid-1",
            Some(format!("{}/missing", mock_server.uri())),
            1_700_000_000,
        )];
        enrich_missing_full_content(
            &mut items,
            &HashSet::new(),
            &ResourcePolicy::with_prefetch_for_tests(),
        )
        .await;

        assert!(items[0].full_content.is_none());
    }

    #[tokio::test]
    async fn only_fetches_the_newest_items_up_to_the_per_cycle_cap() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(article_html(), "text/html"))
            .mount(&mock_server)
            .await;

        let mut items: Vec<NewItem> = (0..MAX_ITEMS_PER_CYCLE + 2)
            .map(|i| {
                item(
                    &format!("guid-{i}"),
                    Some(format!("{}/post-{i}", mock_server.uri())),
                    1_700_000_000 + i as i64,
                )
            })
            .collect();

        enrich_missing_full_content(
            &mut items,
            &HashSet::new(),
            &ResourcePolicy::with_prefetch_for_tests(),
        )
        .await;

        let enriched_count = items.iter().filter(|i| i.full_content.is_some()).count();
        assert_eq!(enriched_count, MAX_ITEMS_PER_CYCLE);
        // The newest-published items (highest index) are the ones kept.
        for (i, item) in items.iter().enumerate().skip(2) {
            assert!(
                item.full_content.is_some(),
                "item {i} should have been enriched"
            );
        }
        assert!(items[0].full_content.is_none());
        assert!(items[1].full_content.is_none());
    }
}
