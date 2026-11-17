//! Fetch worker: polls a single feed with a conditional request and
//! applies the `304`/`200` response branches (see docs/ARCHITECTURE.md
//! §4.1, §6.2). Dispatching this across many feeds under bounded
//! concurrency is M4's job - this module only handles one feed at a
//! time.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::db::{self, FeedMeta, NewItem};

use super::backoff;
use super::client::{self, ConditionalFetch};
use super::enrichment;
use super::parser;

/// What happened when polling a feed.
#[derive(Debug, PartialEq)]
pub enum PollOutcome {
    /// Server returned `304 Not Modified`.
    NotModified,
    /// Server returned `200 OK`; `new_item_count` new rows were
    /// inserted (existing items are skipped by `db::upsert_items`'s
    /// dedup).
    Updated { new_item_count: usize },
    /// The fetch (network/timeout) or the parse failed; recorded to
    /// `last_error` and rescheduled via backoff (§7) rather than
    /// bubbling up as a hard `Err` - a misbehaving feed shouldn't look
    /// like an infrastructure failure to the caller.
    Failed { error: String },
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Candidate items from one fetch, plus the feed's declared language
/// (RSS `<language>` / Atom `xml:lang`; `None` for article-list pages).
type ParsedFetch = (Vec<NewItem>, Option<String>);

/// Parses a fetched body into candidate items - pure, no DB access, so
/// it can run before any lock is acquired.
fn parse_items(meta: &FeedMeta, bytes: &[u8], now: i64) -> Result<ParsedFetch, String> {
    if meta.format.as_deref() == Some("website") {
        super::webpage::parse(&meta.url, bytes, now).map(|listing| {
            let items = listing
                .items
                .into_iter()
                .filter(|item| item.published_at > meta.last_checked_at)
                .collect::<Vec<_>>();
            (items, None)
        })
    } else {
        parser::parse(bytes)
            .map(|parsed| {
                (
                    parser::to_new_items(&parsed, bytes, now),
                    parser::feed_language(&parsed),
                )
            })
            .map_err(|e| e.to_string())
    }
}

/// Persists already-parsed (and, on success, already-enriched) items
/// plus the feed's new etag/last_modified/next_fetch_at, or records a
/// parse failure (§7). Split from parsing/enrichment so those steps,
/// including enrichment's network `.await`s, can run before the DB
/// lock is acquired, not while holding it.
fn apply_parsed(
    conn: &Connection,
    meta: &FeedMeta,
    parsed_items: Result<ParsedFetch, String>,
    etag: Option<String>,
    last_modified: Option<String>,
    now: i64,
) -> Result<PollOutcome, String> {
    match parsed_items {
        Ok((items, language)) => {
            let new_item_count =
                db::upsert_items(conn, meta.id, &items).map_err(|e| e.to_string())?;
            if language.is_some() {
                db::set_feed_language(conn, meta.id, language.as_deref())
                    .map_err(|e| e.to_string())?;
            }

            db::update_feed_fetched(
                conn,
                meta.id,
                etag.as_deref(),
                last_modified.as_deref(),
                now + meta.interval_secs,
            )
            .map_err(|e| e.to_string())?;

            Ok(PollOutcome::Updated { new_item_count })
        }
        // Parsing failed even though the fetch succeeded: skip this
        // cycle only, retry on the next poll (§7). etag/last_modified
        // are deliberately left untouched here (see record_failure) so
        // a subsequent identical 200 can be re-attempted rather than
        // silently becoming a 304.
        Err(parse_err) => record_failure(conn, meta, &parse_err),
    }
}

fn apply_not_modified(conn: &Connection, meta: &FeedMeta) -> Result<PollOutcome, String> {
    let next_fetch_at = now_unix() + meta.interval_secs;
    db::update_feed_not_modified(conn, meta.id, next_fetch_at).map_err(|e| e.to_string())?;
    Ok(PollOutcome::NotModified)
}

/// Polls one feed: sends a conditional GET using its stored
/// etag/last_modified, then either just reschedules it (304), parses
/// the body, pre-fetches full content for items missing it (§ full
/// article pre-fetch), and stores new items (200), or records the
/// failure (§7) if the fetch itself errored.
#[cfg(test)]
pub async fn poll_feed(conn: &Connection, meta: &FeedMeta) -> Result<PollOutcome, String> {
    match client::fetch_conditional(
        &meta.url,
        meta.etag.as_deref(),
        meta.last_modified.as_deref(),
    )
    .await
    {
        Ok(ConditionalFetch::NotModified) => apply_not_modified(conn, meta),
        Ok(ConditionalFetch::Modified {
            bytes,
            etag,
            last_modified,
        }) => {
            let now = now_unix();
            let mut parsed_items = parse_items(meta, &bytes, now);
            if let Ok((items, _)) = &mut parsed_items {
                let has_content =
                    db::guid_hashes_with_full_content(conn, meta.id).unwrap_or_default();
                enrichment::enrich_missing_full_content(items, &has_content).await;
            }
            apply_parsed(conn, meta, parsed_items, etag, last_modified, now)
        }
        Err(err) => record_failure(conn, meta, &client::describe_error(&err)),
    }
}

/// Same as [`poll_feed`], for callers holding the connection behind a
/// `Mutex` (the Poll Scheduler's `Arc<Mutex<_>>`, or Tauri's managed
/// `State<'_, Mutex<_>>` in the `refresh_feed` command - both deref to
/// `&Mutex<Connection>`) rather than a plain `&Connection`: the network
/// fetch and the full-content pre-fetch both run with no lock held at
/// all, and the lock is only acquired briefly - once to read which
/// items already have content, once more to persist the final result -
/// never across an `.await` (a `MutexGuard` held across an `.await`
/// inside a task spawned on `JoinSet`/`tokio::spawn` wouldn't compile,
/// since the guard isn't `Send`).
pub async fn poll_feed_locked(
    conn: &Mutex<Connection>,
    meta: &FeedMeta,
) -> Result<PollOutcome, String> {
    let fetched = client::fetch_conditional(
        &meta.url,
        meta.etag.as_deref(),
        meta.last_modified.as_deref(),
    )
    .await;

    match fetched {
        Ok(ConditionalFetch::NotModified) => {
            let guard = conn
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            apply_not_modified(&guard, meta)
        }
        Ok(ConditionalFetch::Modified {
            bytes,
            etag,
            last_modified,
        }) => {
            let now = now_unix();
            let mut parsed_items = parse_items(meta, &bytes, now);
            if let Ok((items, _)) = &mut parsed_items {
                let has_content = {
                    let guard = conn
                        .lock()
                        .map_err(|_| "database lock poisoned".to_string())?;
                    db::guid_hashes_with_full_content(&guard, meta.id).unwrap_or_default()
                    // guard dropped here, before the next .await
                };
                enrichment::enrich_missing_full_content(items, &has_content).await;
            }
            let guard = conn
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            apply_parsed(&guard, meta, parsed_items, etag, last_modified, now)
        }
        Err(err) => {
            let guard = conn
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            record_failure(&guard, meta, &client::describe_error(&err))
        }
    }
}

/// Records a failed attempt: bumps `consecutive_failures`, writes
/// `last_error`, and reschedules via exponential backoff (capped at
/// the feed's own interval) instead of the normal interval (§7). `pub`
/// so the scheduler (M4) can record a network-fetch failure too, from
/// the point where it already holds the lock after its own await.
pub fn record_failure(
    conn: &Connection,
    meta: &FeedMeta,
    error: &str,
) -> Result<PollOutcome, String> {
    let consecutive_failures = meta.consecutive_failures + 1;
    let retry_after = if meta.format.as_deref() == Some("website") {
        // Page checks are intentionally low frequency; a failed page
        // request must not turn the daily schedule into rapid retries.
        meta.interval_secs
    } else {
        backoff::backoff_secs(consecutive_failures, meta.interval_secs)
    };
    let next_fetch_at = now_unix() + retry_after;

    db::update_feed_failed(conn, meta.id, error, next_fetch_at, consecutive_failures)
        .map_err(|e| e.to_string())?;

    Ok(PollOutcome::Failed {
        error: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const RSS2_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sample_rss2.xml"
    ));

    #[tokio::test]
    async fn poll_feed_200_stores_items_and_updates_meta() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/feed.xml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(RSS2_FIXTURE)
                    .insert_header("ETag", "\"abc123\"")
                    .insert_header("Last-Modified", "Mon, 01 Jan 2024 00:00:00 GMT"),
            )
            .mount(&mock_server)
            .await;

        let url = format!("{}/feed.xml", mock_server.uri());
        let conn = db::init_in_memory().unwrap();
        let feed_id = db::insert_feed(&conn, &url, None, None, 0).unwrap();
        let meta = db::get_feed_meta(&conn, feed_id).unwrap().unwrap();

        let outcome = poll_feed(&conn, &meta).await.unwrap();
        assert_eq!(outcome, PollOutcome::Updated { new_item_count: 2 });

        let updated = db::get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(updated.etag, Some("\"abc123\"".to_string()));
        assert_eq!(
            updated.last_modified,
            Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string())
        );
        assert!(updated.next_fetch_at > 0);
    }

    #[tokio::test]
    async fn poll_feed_200_pre_fetches_full_content_for_new_items() {
        let mock_server = MockServer::start().await;
        let feed_body = RSS2_FIXTURE.replace("https://item.invalid", &mock_server.uri());
        Mock::given(method("GET"))
            .and(path("/feed.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(feed_body))
            .mount(&mock_server)
            .await;
        let article_html = r#"<html><head><title>Example</title></head><body>
<article><p>This is a long enough paragraph of real article text to clear the readability floor comfortably.</p></article>
</body></html>"#;
        Mock::given(method("GET"))
            .and(path("/first-post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(article_html, "text/html"))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/second-post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(article_html, "text/html"))
            .mount(&mock_server)
            .await;

        let url = format!("{}/feed.xml", mock_server.uri());
        let conn = db::init_in_memory().unwrap();
        let feed_id = db::insert_feed(&conn, &url, None, None, 0).unwrap();
        let meta = db::get_feed_meta(&conn, feed_id).unwrap().unwrap();

        let outcome = poll_feed(&conn, &meta).await.unwrap();
        assert_eq!(outcome, PollOutcome::Updated { new_item_count: 2 });

        // `db::list_items` deliberately nulls `full_content` for list
        // views (payload-size optimization) - fetch each item by id to
        // see the real stored value.
        let list = db::list_items(&conn, &db::ItemFilter::default(), 10, None).unwrap();
        assert_eq!(list.len(), 2);
        for summary in &list {
            let full = db::get_item_by_id(&conn, summary.id).unwrap().unwrap();
            assert!(
                full.full_content
                    .as_deref()
                    .is_some_and(|c| c.contains("long enough paragraph")),
                "expected item {} to have pre-fetched full content, got {full:?}",
                summary.id
            );
        }
    }

    #[tokio::test]
    async fn poll_feed_304_only_reschedules() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/feed.xml"))
            .and(header("If-None-Match", "\"etag-1\""))
            .respond_with(ResponseTemplate::new(304))
            .mount(&mock_server)
            .await;

        let url = format!("{}/feed.xml", mock_server.uri());
        let conn = db::init_in_memory().unwrap();
        let feed_id = db::insert_feed(&conn, &url, None, None, 0).unwrap();
        db::update_feed_fetched(&conn, feed_id, Some("\"etag-1\""), None, 0).unwrap();
        let meta = db::get_feed_meta(&conn, feed_id).unwrap().unwrap();

        let outcome = poll_feed(&conn, &meta).await.unwrap();
        assert_eq!(outcome, PollOutcome::NotModified);

        let updated = db::get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(updated.etag, Some("\"etag-1\"".to_string())); // untouched by a 304
        assert!(updated.next_fetch_at > 0); // still rescheduled

        assert_eq!(db::count_unread(&conn, feed_id).unwrap(), 0); // no body was ever parsed
    }

    #[tokio::test]
    async fn poll_feed_parse_error_skips_cycle_and_retries_next_time() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/feed.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string("this is not a feed"))
            .mount(&mock_server)
            .await;

        let url = format!("{}/feed.xml", mock_server.uri());
        let conn = db::init_in_memory().unwrap();
        let feed_id = db::insert_feed(&conn, &url, None, None, 0).unwrap();
        let meta = db::get_feed_meta(&conn, feed_id).unwrap().unwrap();

        let outcome = poll_feed(&conn, &meta).await.unwrap();
        match outcome {
            PollOutcome::Failed { error } => assert!(!error.is_empty()),
            other => panic!("expected Failed, got {other:?}"),
        }

        let updated = db::get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(updated.consecutive_failures, 1);
        // Rescheduled, not stuck at the old next_fetch_at - "retry on
        // next poll" rather than this cycle silently never ending.
        assert!(updated.next_fetch_at > 0);
        assert_eq!(updated.etag, None); // nothing new to adopt from an unparseable body
        assert_eq!(db::count_unread(&conn, feed_id).unwrap(), 0); // "skip only that cycle" - no items stored

        // Recovery on a later successful poll is already covered by
        // db::repository::tests::success_paths_clear_prior_failure_state
        // (update_feed_fetched/update_feed_not_modified both reset
        // consecutive_failures/last_error).
    }
}
