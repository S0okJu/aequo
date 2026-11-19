//! On-demand article extraction and disk-backed caching.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;
use tokio::sync::watch;

use crate::db::{self, ArticleCache, Item};
use crate::fetch::article_fetch;
use crate::resource_policy::{ResourcePolicy, WorkKind};

const SUCCESS_CACHE_TTL: i64 = 60 * 60 * 24 * 30;
const FAILURE_CACHE_TTL: i64 = 60 * 60 * 6;
const MAX_READER_HTML_BYTES: usize = 5 * 1024 * 1024;
const BODY_LIMIT_MESSAGE: &str = "This body exceeds the 5 MiB display limit. The original remains on disk; open the original page to read it.";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleView {
    pub item_id: i64,
    pub original_url: Option<String>,
    pub final_url: Option<String>,
    pub title: Option<String>,
    pub content_html: Option<String>,
    pub rss_content_html: Option<String>,
    pub fetched_at: Option<i64>,
    pub cached: bool,
    pub error: Option<String>,
}

/// The app has one foreground reader. A single revision cell cancels older
/// reads without retaining request IDs, cancelled tasks or item bodies.
/// Revisions originate in a module shared across route/component mounts.
pub struct ArticleReads(watch::Sender<(u64, bool)>);

impl Default for ArticleReads {
    fn default() -> Self {
        Self(watch::channel((0, true)).0)
    }
}

impl ArticleReads {
    fn begin(&self, revision: u64) -> Result<watch::Receiver<(u64, bool)>, String> {
        if revision == 0 || revision > 9_007_199_254_740_991 {
            return Err("Invalid article request revision".into());
        }
        let accepted = self.0.send_if_modified(|current| {
            if revision <= current.0 {
                return false;
            }
            *current = (revision, false);
            true
        });
        if !accepted {
            return Err("Article request cancelled".into());
        }
        Ok(self.0.subscribe())
    }
    fn cancel(&self, revision: u64) {
        self.0.send_if_modified(|current| {
            if revision < current.0 || (revision == current.0 && current.1) {
                return false;
            }
            *current = (revision, true);
            true
        });
    }
}

async fn until_cancelled<T>(
    mut changes: watch::Receiver<(u64, bool)>,
    revision: u64,
    operation: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    if *changes.borrow_and_update() != (revision, false) {
        return Err("Article request cancelled".into());
    }
    tokio::select! {
        biased;
        _ = changes.changed() => Err("Article request cancelled".into()),
        result = operation => result,
    }
}

#[tauri::command]
pub fn cancel_article_read(
    reads: State<'_, ArticleReads>,
    request_revision: u64,
) -> Result<(), String> {
    if request_revision == 0 || request_revision > 9_007_199_254_740_991 {
        return Err("Invalid article request revision".into());
    }
    reads.cancel(request_revision);
    Ok(())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn to_view(
    item_id: i64,
    cache: Option<ArticleCache>,
    original_url: Option<String>,
    cached: bool,
) -> ArticleView {
    if let Some(cache) = cache {
        ArticleView {
            item_id,
            original_url: Some(cache.original_url),
            final_url: cache.final_url,
            title: cache.title,
            content_html: cache.content_html,
            rss_content_html: None,
            fetched_at: Some(cache.fetched_at),
            cached,
            error: cache.error,
        }
    } else {
        ArticleView {
            item_id,
            original_url,
            final_url: None,
            title: None,
            content_html: None,
            rss_content_html: None,
            fetched_at: None,
            cached: false,
            error: None,
        }
    }
}

fn item_for_article(conn: &rusqlite::Connection, item_id: i64) -> Result<(Item, bool), String> {
    db::get_reader_item(conn, item_id, MAX_READER_HTML_BYTES)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("post {item_id} was not found"))
}

#[tauri::command]
pub async fn load_article(
    db_state: State<'_, std::sync::Mutex<rusqlite::Connection>>,
    policy: State<'_, ResourcePolicy>,
    reads: State<'_, ArticleReads>,
    item_id: i64,
    force_refresh: bool,
    request_revision: u64,
) -> Result<ArticleView, String> {
    let changes = reads.begin(request_revision)?;
    until_cancelled(
        changes,
        request_revision,
        load_article_for(&db_state, &policy, item_id, force_refresh),
    )
    .await
}

async fn load_article_for(
    db_state: &std::sync::Mutex<rusqlite::Connection>,
    policy: &ResourcePolicy,
    item_id: i64,
    force_refresh: bool,
) -> Result<ArticleView, String> {
    // Admission precedes body/cache reads, so queued work does not retain
    // full RSS HTML or duplicate cached article strings while waiting.
    let _permit = policy.acquire(WorkKind::Article, false).await;
    let ((mut item, source_oversized), cached) = {
        let conn = db_state
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let item = item_for_article(&conn, item_id)?;
        let cached = if item.0.full_content.is_some() {
            None
        } else {
            db::get_reader_article_cache(&conn, item_id, MAX_READER_HTML_BYTES)
                .map_err(|error| error.to_string())?
        };
        (item, cached)
    };
    let (cached, cache_oversized) = cached
        .map(|(entry, oversized)| (Some(entry), oversized))
        .unwrap_or((None, false));
    let fallback = item.summary.take();
    let item_id = item.id;
    let original_url = item.link.clone();
    let result = load_article_view(db_state, item, cached, cache_oversized, force_refresh).await;
    let mut view = match result {
        Ok(view) => view,
        Err(error) if fallback.is_some() => {
            let mut view = to_view(item_id, None, original_url, false);
            view.error = Some(error);
            view
        }
        Err(error) => return Err(error),
    };
    enforce_view_limit(&mut view);
    if view.content_html.is_none() && view.rss_content_html.is_none() {
        view.rss_content_html = fallback;
    }
    if source_oversized && view.error.is_none() {
        view.error = Some(BODY_LIMIT_MESSAGE.into());
    }
    enforce_view_limit(&mut view);
    Ok(view)
}

fn enforce_view_limit(view: &mut ArticleView) {
    if view
        .content_html
        .as_ref()
        .is_some_and(|body| body.len() > MAX_READER_HTML_BYTES)
    {
        view.content_html = None;
        view.error = Some(BODY_LIMIT_MESSAGE.into());
    }
    if view
        .rss_content_html
        .as_ref()
        .is_some_and(|body| body.len() > MAX_READER_HTML_BYTES)
    {
        view.rss_content_html = None;
        view.error = Some(BODY_LIMIT_MESSAGE.into());
    }
}

async fn load_article_view(
    db_state: &std::sync::Mutex<rusqlite::Connection>,
    item: Item,
    mut cached: Option<ArticleCache>,
    cache_oversized: bool,
    force_refresh: bool,
) -> Result<ArticleView, String> {
    let item_id = item.id;
    if let Some(rss_content) = item.full_content {
        return Ok(ArticleView {
            item_id,
            original_url: item.link,
            final_url: None,
            title: item.title,
            content_html: None,
            rss_content_html: Some(rss_content),
            fetched_at: None,
            cached: false,
            error: None,
        });
    }
    let Some(original_url) = item.link.clone() else {
        return Ok(to_view(item_id, cached, None, true));
    };

    let now = now_unix();
    if !force_refresh
        && let Some(entry) = cached
            .as_ref()
            .filter(|entry| entry.original_url == original_url)
    {
        let ttl = if entry.error.is_some() {
            FAILURE_CACHE_TTL
        } else {
            SUCCESS_CACHE_TTL
        };
        if now.saturating_sub(entry.fetched_at) < ttl {
            return Ok(to_view(item_id, cached.take(), Some(original_url), true));
        }
    }

    let etag = cached.as_ref().and_then(|entry| entry.etag.clone());
    let last_modified = cached
        .as_ref()
        .and_then(|entry| entry.last_modified.clone());
    match article_fetch::fetch_and_extract(&original_url, etag.as_deref(), last_modified.as_deref())
        .await
    {
        Ok(None) => {
            if let Some(mut previous) = cached {
                previous.fetched_at = now;
                previous.error = None;
                let conn = db_state
                    .lock()
                    .map_err(|_| "database lock poisoned".to_string())?;
                db::touch_article_cache(&conn, item_id, now).map_err(|error| error.to_string())?;
                Ok(to_view(item_id, Some(previous), Some(original_url), true))
            } else {
                Err("The article server returned not-modified without a local copy.".to_string())
            }
        }
        Ok(Some(page)) => {
            let entry = ArticleCache {
                item_id,
                original_url: original_url.clone(),
                final_url: Some(page.final_url),
                title: Some(page.title),
                content_html: Some(page.content_html),
                text_content: Some(page.text_content),
                etag: page.etag,
                last_modified: page.last_modified,
                fetched_at: now,
                error: None,
            };
            let conn = db_state
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            db::save_article_cache(&conn, &entry).map_err(|error| error.to_string())?;
            Ok(to_view(item_id, Some(entry), Some(original_url), false))
        }
        Err(message) => {
            if let Some(mut previous) = cached.filter(|entry| {
                entry.original_url == original_url
                    && (entry.content_html.is_some() || cache_oversized)
            }) {
                previous.error = Some(if cache_oversized {
                    format!("Could not refresh the page. {message} {BODY_LIMIT_MESSAGE}")
                } else {
                    format!(
                        "Could not refresh the page. Showing the previously saved full text. {message}"
                    )
                });
                return Ok(to_view(item_id, Some(previous), Some(original_url), true));
            }
            let entry = ArticleCache {
                item_id,
                original_url: original_url.clone(),
                final_url: None,
                title: None,
                content_html: None,
                text_content: None,
                etag: None,
                last_modified: None,
                fetched_at: now,
                error: Some(message),
            };
            let conn = db_state
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            db::save_article_cache(&conn, &entry).map_err(|error| error.to_string())?;
            Ok(to_view(item_id, Some(entry), Some(original_url), false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::resources::{ResourceMode, ResourceSettings, ResourceSnapshot};
    use std::sync::Mutex;

    #[tokio::test]
    async fn prefetch_off_reading_uses_embedded_or_cached_bodies_and_keeps_retry_available() {
        let conn = db::init_in_memory().unwrap();
        let feed = db::insert_feed(&conn, "https://example.com/feed", None, None, 0).unwrap();
        let mut items = crate::db::collection::tests::items(3);
        items[0].full_content = Some("<p>Embedded article</p>".into());
        // A blocked address deterministically exercises the reader's failure path.
        items[1].link = Some("http://127.0.0.1/article".into());
        db::upsert_items(&conn, feed, &items).unwrap();
        let rows = db::list_items(&conn, &db::ItemFilter::default(), 10, None).unwrap();
        let id = |key: &str| rows.iter().find(|r| r.guid_hash == key).unwrap().id;
        let cached = ArticleCache {
            item_id: id("id-2"),
            original_url: items[2].link.clone().unwrap(),
            final_url: None,
            title: Some("Cached article".into()),
            content_html: Some("<p>Cached full text</p>".into()),
            text_content: None,
            etag: None,
            last_modified: None,
            fetched_at: now_unix(),
            error: None,
        };
        db::save_article_cache(&conn, &cached).unwrap();
        let state = Mutex::new(conn);
        let policy = ResourcePolicy::default();
        assert!(!policy.settings().prefetch_full_text);
        let embedded = load_article_for(&state, &policy, id("id-0"), false)
            .await
            .unwrap();
        assert_eq!(
            embedded.rss_content_html.as_deref(),
            Some("<p>Embedded article</p>")
        );
        let first = load_article_for(&state, &policy, id("id-2"), false)
            .await
            .unwrap();
        let second = load_article_for(&state, &policy, id("id-2"), false)
            .await
            .unwrap();
        assert!(first.cached && second.cached);
        assert_eq!(second.content_html, cached.content_html);
        let failed = load_article_for(&state, &policy, id("id-1"), false)
            .await
            .unwrap();
        assert!(failed.error.is_some());
        let retry = load_article_for(&state, &policy, id("id-1"), true)
            .await
            .unwrap();
        assert!(!retry.cached && retry.error.is_some());
        let stored = db::get_item_by_id(&state.lock().unwrap(), id("id-1"))
            .unwrap()
            .unwrap();
        assert_eq!(stored.summary, items[1].summary);
        assert_eq!(policy.status().running_work, 0);
    }

    #[tokio::test]
    async fn cancellation_precedes_registration_and_stale_cancels_cannot_stop_new_work() {
        let reads = ArticleReads::default();
        reads.cancel(10);
        assert!(reads.begin(10).is_err());
        assert!(reads.begin(9).is_err());
        let latest = reads.begin(11).unwrap();
        reads.cancel(10);
        assert_eq!(
            until_cancelled(latest, 11, async { Ok(42) }).await.unwrap(),
            42
        );
        assert!(reads.begin(11).is_err());
        assert!(reads.begin(0).is_err());
    }

    #[tokio::test]
    async fn newer_selection_drops_in_flight_work_and_releases_global_article_permit() {
        let reads = ArticleReads::default();
        let policy = ResourcePolicy::default();
        let (started, ready) = tokio::sync::oneshot::channel();
        let changes = reads.begin(1).unwrap();
        let worker_policy = policy.clone();
        let work = tokio::spawn(async move {
            until_cancelled(changes, 1, async {
                let _permit = worker_policy.acquire(WorkKind::Article, false).await;
                let _ = started.send(());
                std::future::pending::<Result<(), String>>().await
            })
            .await
        });
        ready.await.unwrap();
        assert_eq!(policy.status().running_work, 1);
        let _next = reads.begin(2).unwrap();
        assert_eq!(
            work.await.unwrap().unwrap_err(),
            "Article request cancelled"
        );
        assert_eq!(policy.status().running_work, 0);
    }

    #[tokio::test]
    async fn cancelled_queued_read_never_accesses_missing_item_body() {
        let conn = Mutex::new(db::init_in_memory().unwrap());
        let policy = ResourcePolicy::default();
        policy.apply(ResourceSnapshot {
            revision: 1,
            settings: ResourceSettings::preset(ResourceMode::Light).unwrap(),
        });
        let held = policy.acquire(WorkKind::Article, false).await;
        let reads = ArticleReads::default();
        let changes = reads.begin(1).unwrap();
        let future = until_cancelled(changes, 1, load_article_for(&conn, &policy, -1, false));
        tokio::pin!(future);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut future)
                .await
                .is_err()
        );
        reads.cancel(1);
        assert_eq!(future.await.unwrap_err(), "Article request cancelled");
        assert_eq!(policy.status().running_work, 1);
        drop(held);
        assert_eq!(policy.status().running_work, 0);
    }

    #[test]
    fn reader_projection_bounds_utf8_html_omits_plaintext_and_preserves_disk_sources() {
        let conn = db::init_in_memory().unwrap();
        let feed = db::insert_feed(&conn, "https://reader.invalid/rss", None, None, 0).unwrap();
        let boundary = format!("{}x", "한".repeat(21)); // 64 UTF-8 bytes.
        let oversized = format!("{boundary}한");
        db::upsert_items(
            &conn,
            feed,
            &[
                db::NewItem {
                    published_at_kind: "timestamp",
                    guid_hash: "boundary".into(),
                    title: None,
                    link: None,
                    summary: Some(boundary.clone()),
                    full_content: None,
                    published_at: 1,
                    fetched_at: 1,
                },
                db::NewItem {
                    published_at_kind: "timestamp",
                    guid_hash: "oversized".into(),
                    title: None,
                    link: None,
                    summary: Some(oversized.clone()),
                    full_content: None,
                    published_at: 2,
                    fetched_at: 1,
                },
            ],
        )
        .unwrap();
        let ids = db::list_items(&conn, &db::ItemFilter::default(), 50, None).unwrap();
        let (small, is_large) = db::get_reader_item(&conn, ids[1].id, 64).unwrap().unwrap();
        assert_eq!(small.summary, Some(boundary));
        assert!(!is_large);
        let (large, is_large) = db::get_reader_item(&conn, ids[0].id, 64).unwrap().unwrap();
        assert!(large.summary.is_none());
        assert!(is_large);
        assert_eq!(
            db::get_item_by_id(&conn, ids[0].id)
                .unwrap()
                .unwrap()
                .summary,
            Some(oversized.clone())
        );
        let entry = ArticleCache {
            item_id: ids[0].id,
            original_url: "https://reader.invalid/article".into(),
            final_url: None,
            title: None,
            content_html: Some(oversized.clone()),
            text_content: Some("searchable original text".into()),
            etag: Some("etag".into()),
            last_modified: None,
            fetched_at: 1,
            error: None,
        };
        db::save_article_cache(&conn, &entry).unwrap();
        let (reader, cache_oversized) = db::get_reader_article_cache(&conn, entry.item_id, 64)
            .unwrap()
            .unwrap();
        assert!(cache_oversized);
        assert!(reader.content_html.is_none());
        assert!(reader.text_content.is_none());
        assert!(reader.etag.is_none());
        assert!(reader.error.is_some());
        db::touch_article_cache(&conn, entry.item_id, 50).unwrap();
        let stored = db::get_article_cache(&conn, entry.item_id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.text_content, entry.text_content);
        assert_eq!(stored.content_html, Some(oversized));
        assert_eq!(stored.fetched_at, 50);
    }

    #[tokio::test]
    async fn failed_forced_refresh_never_overwrites_an_oversized_original_cache() {
        let conn = db::init_in_memory().unwrap();
        let feed = db::insert_feed(&conn, "https://reader.invalid/rss", None, None, 0).unwrap();
        let url = "http://127.0.0.1/denied";
        db::upsert_items(
            &conn,
            feed,
            &[db::NewItem {
                published_at_kind: "timestamp",
                guid_hash: "preserved-cache".into(),
                title: None,
                link: Some(url.into()),
                summary: Some("<p>RSS fallback</p>".into()),
                full_content: None,
                published_at: 1,
                fetched_at: 1,
            }],
        )
        .unwrap();
        let item = db::list_items(&conn, &db::ItemFilter::default(), 50, None).unwrap()[0].id;
        let entry = ArticleCache {
            item_id: item,
            original_url: url.into(),
            final_url: None,
            title: None,
            content_html: Some("x".repeat(MAX_READER_HTML_BYTES + 1)),
            text_content: Some("original search text".into()),
            etag: Some("etag".into()),
            last_modified: None,
            fetched_at: 1,
            error: None,
        };
        db::save_article_cache(&conn, &entry).unwrap();
        let state = Mutex::new(conn);
        let view = load_article_for(&state, &ResourcePolicy::default(), item, true)
            .await
            .unwrap();
        assert!(view.content_html.is_none());
        assert_eq!(
            view.rss_content_html.as_deref(),
            Some("<p>RSS fallback</p>")
        );
        assert!(view.error.is_some());
        let stored = db::get_article_cache(&state.lock().unwrap(), item)
            .unwrap()
            .unwrap();
        assert_eq!(stored.content_html, entry.content_html);
        assert_eq!(stored.text_content, entry.text_content);
        assert_eq!(stored.etag, entry.etag);
    }

    #[test]
    fn extracted_html_limit_never_truncates_original_markup_into_an_invalid_partial_body() {
        let mut view = to_view(1, None, Some("https://reader.invalid".into()), false);
        view.content_html = Some("x".repeat(MAX_READER_HTML_BYTES));
        enforce_view_limit(&mut view);
        assert!(view.content_html.is_some());
        view.content_html.as_mut().unwrap().push('한');
        enforce_view_limit(&mut view);
        assert!(view.content_html.is_none());
        assert!(view.error.unwrap().contains("5 MiB"));
    }

    #[tokio::test]
    async fn reader_receives_original_summary_even_with_preview_cap_and_prefetch_disabled() {
        let conn = db::init_in_memory().unwrap();
        let feed = db::insert_feed(&conn, "https://reader.invalid/rss", None, None, 0).unwrap();
        let original = format!("<p>{}</p>", "Full RSS body. ".repeat(1000));
        db::upsert_items(
            &conn,
            feed,
            &[db::NewItem {
                published_at_kind: "timestamp",
                guid_hash: "reader-test".into(),
                title: Some("Title".into()),
                link: None,
                summary: Some(original.clone()),
                full_content: None,
                published_at: 1,
                fetched_at: 1,
            }],
        )
        .unwrap();
        let list = db::list_items(&conn, &db::ItemFilter::default(), 50, None).unwrap();
        assert_eq!(list[0].preview.as_ref().unwrap().chars().count(), 500);
        let policy = ResourcePolicy::default();
        policy.apply(ResourceSnapshot {
            revision: 1,
            settings: ResourceSettings::preset(ResourceMode::Light).unwrap(),
        });
        let view = load_article_for(&Mutex::new(conn), &policy, list[0].id, false)
            .await
            .unwrap();
        assert_eq!(view.rss_content_html, Some(original));
        assert!(view.error.is_none());
    }
}
