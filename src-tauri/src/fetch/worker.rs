//! Fetch worker: polls a single feed with a conditional request and
//! applies the `304`/`200` response branches (see docs/ARCHITECTURE.md
//! §4.1, §6.2). Dispatching this across many feeds under bounded
//! concurrency is M4's job - this module only handles one feed at a
//! time.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::db::{self, FeedMeta, NewItem};
use crate::resource_policy::{ResourcePolicy, WorkKind, effective_due_at};

use super::backoff;
use super::client::{self, ConditionalFetch};
use super::enrichment;
use super::parser;

/// What happened when polling a feed.
#[derive(Debug, PartialEq)]
pub enum PollOutcome {
    /// No request was issued: the live schedule/backoff deferred it.
    Deferred { until: i64 },
    /// Server returned `304 Not Modified`.
    NotModified,
    /// Existing website rows were corrected/hidden; reload even without new items.
    Reconciled { new_item_count: usize },
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
type ParsedFetch = (Vec<NewItem>, Option<String>, Vec<String>);

/// Parses a fetched body into candidate items - pure, no DB access, so
/// it can run before any lock is acquired.
fn parse_items(meta: &FeedMeta, bytes: &[u8], now: i64) -> Result<ParsedFetch, String> {
    if meta.format.as_deref() == Some("website") {
        super::webpage::parse(&meta.url, bytes, now)
            .map(|listing| (listing.items, None, listing.rejected_links))
    } else {
        parser::parse(bytes)
            .map(|parsed| {
                (
                    parser::to_new_items(&parsed, bytes, now),
                    parser::feed_language(&parsed),
                    Vec::new(),
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
        Ok((items, language, rejected)) => {
            let tx = rusqlite::Transaction::new_unchecked(
                conn,
                rusqlite::TransactionBehavior::Immediate,
            )
            .map_err(|e| e.to_string())?;
            let conn = &*tx;
            let (new_item_count, changed) = if meta.format.as_deref() == Some("website") {
                let changes = db::reconcile_website_in_transaction(
                    conn,
                    meta.id,
                    &items,
                    &rejected,
                    super::webpage::PARSER_VERSION,
                )
                .map_err(|e| e.to_string())?;
                (changes.inserted, changes.changed)
            } else {
                (
                    db::upsert_items_in_transaction(conn, meta.id, &items)
                        .map_err(|e| e.to_string())?,
                    false,
                )
            };
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

            tx.commit().map_err(|e| e.to_string())?;
            Ok(if changed {
                PollOutcome::Reconciled { new_item_count }
            } else {
                PollOutcome::Updated { new_item_count }
            })
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
    let repair = meta.format.as_deref() == Some("website")
        && db::website_needs_repair(conn, meta.id, super::webpage::PARSER_VERSION)
            .map_err(|e| e.to_string())?;
    match client::fetch_conditional(
        &meta.url,
        if repair { None } else { meta.etag.as_deref() },
        if repair {
            None
        } else {
            meta.last_modified.as_deref()
        },
    )
    .await
    {
        Ok(ConditionalFetch::NotModified) => apply_not_modified(conn, meta),
        Ok(ConditionalFetch::Modified {
            final_url,
            bytes,
            etag,
            last_modified,
        }) => {
            let now = now_unix();
            let mut source_meta = meta.clone();
            source_meta.url = final_url;
            let mut parsed_items = parse_items(&source_meta, &bytes, now);
            if let Ok((items, _, _)) = &mut parsed_items {
                db::collection::filter_items(conn, meta.id, items).map_err(|e| e.to_string())?;
                let has_content =
                    db::guid_hashes_with_full_content(conn, meta.id).unwrap_or_default();
                enrichment::enrich_missing_full_content(
                    items,
                    &has_content,
                    &ResourcePolicy::default(),
                )
                .await;
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
    policy: &ResourcePolicy,
    manual: bool,
) -> Result<PollOutcome, String> {
    let _permit = policy.acquire(WorkKind::Feed, false).await;
    // Re-read after waiting: another poll or a policy change may have moved
    // this feed's schedule. Never overwrite the user's per-feed interval.
    let meta = {
        let guard = conn
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db::get_feed_meta(&guard, meta.id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("feed {} not found", meta.id))?
    };
    let due_at = if manual {
        meta.next_fetch_at
    } else {
        effective_due_at(&meta, policy.settings().min_auto_refresh_secs)
    };
    if due_at > now_unix() && (!manual || meta.consecutive_failures > 0) {
        return Ok(PollOutcome::Deferred { until: due_at });
    }
    let repair = meta.format.as_deref() == Some("website")
        && (manual || {
            let guard = conn
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            db::website_needs_repair(&guard, meta.id, super::webpage::PARSER_VERSION)
                .map_err(|e| e.to_string())?
        });
    let fetched = client::fetch_conditional(
        &meta.url,
        if repair { None } else { meta.etag.as_deref() },
        if repair {
            None
        } else {
            meta.last_modified.as_deref()
        },
    )
    .await;

    match fetched {
        Ok(ConditionalFetch::NotModified) => {
            let guard = conn
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            apply_not_modified(&guard, &meta)
        }
        Ok(ConditionalFetch::Modified {
            final_url,
            bytes,
            etag,
            last_modified,
        }) => {
            let now = now_unix();
            let mut source_meta = meta.clone();
            source_meta.url = final_url;
            let mut parsed_items = parse_items(&source_meta, &bytes, now);
            if let Ok((items, _, _)) = &mut parsed_items {
                let has_content = {
                    let guard = conn
                        .lock()
                        .map_err(|_| "database lock poisoned".to_string())?;
                    db::collection::filter_items(&guard, meta.id, items)
                        .map_err(|e| e.to_string())?;
                    db::guid_hashes_with_full_content(&guard, meta.id).map_err(|e| e.to_string())?
                    // guard dropped here, before the next .await
                };
                enrichment::enrich_missing_full_content(items, &has_content, policy).await;
            }
            let guard = conn
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            apply_parsed(&guard, &meta, parsed_items, etag, last_modified, now)
        }
        Err(err) => {
            let guard = conn
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            record_failure(&guard, &meta, &client::describe_error(&err))
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

    #[tokio::test]
    async fn live_auto_floor_defers_requests_and_manual_updates_still_honor_error_backoff() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&server)
            .await;
        let conn = Mutex::new(crate::db::init_in_memory().unwrap());
        let now = now_unix();
        let id = {
            let c = conn.lock().unwrap();
            let id = db::insert_feed(&c, &server.uri(), None, None, 0).unwrap();
            db::update_feed_not_modified(&c, id, now + 900).unwrap();
            id
        };
        let policy = ResourcePolicy::default();
        policy.apply(crate::db::resources::ResourceSnapshot {
            revision: 1,
            settings: crate::db::resources::ResourceSettings::preset(
                crate::db::resources::ResourceMode::Light,
            )
            .unwrap(),
        });
        let meta = db::get_feed_meta(&conn.lock().unwrap(), id)
            .unwrap()
            .unwrap();
        let auto = poll_feed_locked(&conn, &meta, &policy, false)
            .await
            .unwrap();
        assert_eq!(auto, PollOutcome::Deferred { until: now + 1800 });
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
        assert_eq!(
            poll_feed_locked(&conn, &meta, &policy, true).await.unwrap(),
            PollOutcome::NotModified
        );
        db::update_feed_failed(&conn.lock().unwrap(), id, "retry later", now + 300, 1).unwrap();
        assert_eq!(
            poll_feed_locked(&conn, &meta, &policy, true).await.unwrap(),
            PollOutcome::Deferred { until: now + 300 }
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

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
    async fn default_poll_stores_metadata_without_article_requests() {
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
            .expect(0)
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/second-post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(article_html, "text/html"))
            .expect(0)
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
            assert!(full.full_content.is_none());
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

#[cfg(test)]
mod website_repair_tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    #[tokio::test]
    async fn legacy_website_forces_body_repairs_dates_and_discovers_backdated_posts() {
        let server = MockServer::start().await;
        let body = r#"<html><nav><a href='/docs'>Documentation</a></nav><main>
            <article><h5><a href='/blog/old/'>Old article</a></h5><time datetime='2020-01-02'>January</time></article>
            <article><h5><a href='/blog/backdated/'>Backdated article</a></h5><time datetime='2019-01-02'>January</time></article>
            </main></html>"#;
        Mock::given(method("GET"))
            .and(path("/blog/"))
            .and(|r: &wiremock::Request| !r.headers.contains_key("if-none-match"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(body)
                    .insert_header("ETag", "\"v2\""),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/blog/"))
            .and(header("If-None-Match", "\"v2\""))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&server)
            .await;
        let conn = db::init_in_memory().unwrap();
        let url = format!("{}/blog/", server.uri());
        let id = db::insert_feed(&conn, &url, None, Some("website"), now_unix()).unwrap();
        let old = db::NewItem {
            guid_hash: "web:legacy-key".into(),
            title: Some("Old article".into()),
            link: Some(format!("{}/blog/old", server.uri())),
            summary: None,
            full_content: Some("Keep original body".into()),
            published_at: now_unix(),
            published_at_kind: "timestamp",
            fetched_at: now_unix(),
        };
        db::upsert_items(&conn, id, &[old]).unwrap();
        conn.execute("UPDATE items SET is_read=1,read_at=42", [])
            .unwrap();
        db::update_feed_fetched(&conn, id, Some("\"v1\""), None, now_unix() + 86400).unwrap();
        let meta = db::get_feed_meta(&conn, id).unwrap().unwrap();
        assert_eq!(
            poll_feed(&conn, &meta).await.unwrap(),
            PollOutcome::Reconciled { new_item_count: 1 }
        );
        let rows = db::list_items(&conn, &db::ItemFilter::default(), 10, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, 1);
        assert!(rows[0].is_read);
        assert_eq!(rows[0].read_at, Some(42));
        assert_eq!(rows[0].published_at_kind, "date");
        assert!(rows[1].published_at < meta.last_checked_at);
        let meta = db::get_feed_meta(&conn, id).unwrap().unwrap();
        assert_eq!(
            poll_feed(&conn, &meta).await.unwrap(),
            PollOutcome::NotModified
        );
        // New parser version marker prevents future automatic unconditional requests.
        assert!(
            !db::website_needs_repair(&conn, id, super::super::webpage::PARSER_VERSION).unwrap()
        );
    }
}

#[cfg(test)]
mod collection_storage_tests {
    use super::*;
    use crate::commands::add_feed::store_feed;

    #[test]
    fn failed_metadata_write_rolls_back_articles_then_retry_persists_every_new_item() {
        for format in ["rss", "website"] {
            let conn = db::init_in_memory().unwrap();
            let source = crate::db::collection::tests::items(50);
            let id = store_feed(
                &conn,
                "https://example.com/feed",
                None,
                format,
                None,
                &source,
            )
            .unwrap();
            let meta = db::get_feed_meta(&conn, id).unwrap().unwrap();
            let incoming = crate::db::collection::tests::items(65);
            conn.execute_batch("CREATE TRIGGER reject_metadata BEFORE UPDATE OF etag ON feeds BEGIN SELECT RAISE(ABORT,'disk failure'); END").unwrap();
            let run = || {
                apply_parsed(
                    &conn,
                    &meta,
                    Ok((incoming.clone(), None, vec![])),
                    Some("new-etag".into()),
                    None,
                    100,
                )
            };
            assert!(run().is_err());
            assert_eq!(db::count_unread(&conn, id).unwrap(), 10);
            assert!(
                db::get_feed_meta(&conn, id)
                    .unwrap()
                    .unwrap()
                    .etag
                    .is_none()
            );
            conn.execute_batch("DROP TRIGGER reject_metadata").unwrap();
            let result = run().unwrap();
            assert!(matches!(
                result,
                PollOutcome::Updated { new_item_count: 15 }
                    | PollOutcome::Reconciled { new_item_count: 15 }
            ));
            assert_eq!(db::count_unread(&conn, id).unwrap(), 25);
            assert!(matches!(
                run().unwrap(),
                PollOutcome::Updated { new_item_count: 0 }
                    | PollOutcome::Reconciled { new_item_count: 0 }
            ));
            apply_not_modified(&conn, &meta).unwrap();
            assert_eq!(db::count_unread(&conn, id).unwrap(), 25);
            assert_eq!(db::collection::exclusions(&conn, id).unwrap().len(), 40);
        }
    }
}

#[cfg(test)]
mod collection_concurrency_tests {
    use super::*;
    use crate::commands::add_feed::store_feed;

    #[test]
    fn two_connections_refreshing_the_same_feed_keep_one_copy_and_initial_exclusions() {
        let path = std::env::temp_dir().join(format!(
            "aequo-collection-race-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let conn = db::init(&path).unwrap();
        let id = store_feed(
            &conn,
            "https://example.com/feed",
            None,
            "rss",
            None,
            &crate::db::collection::tests::items(50),
        )
        .unwrap();
        let meta = db::get_feed_meta(&conn, id).unwrap().unwrap();
        drop(conn);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                let meta = meta.clone();
                std::thread::spawn(move || {
                    let conn = db::init(&path).unwrap();
                    barrier.wait();
                    apply_parsed(
                        &conn,
                        &meta,
                        Ok((crate::db::collection::tests::items(65), None, vec![])),
                        Some("etag".into()),
                        None,
                        100,
                    )
                    .unwrap()
                })
            })
            .collect();
        let mut counts: Vec<_> = handles
            .into_iter()
            .map(|h| match h.join().unwrap() {
                PollOutcome::Updated { new_item_count } => new_item_count,
                outcome => panic!("unexpected {outcome:?}"),
            })
            .collect();
        counts.sort();
        assert_eq!(counts, vec![0, 15]);
        let conn = db::init(&path).unwrap();
        assert_eq!(db::count_unread(&conn, id).unwrap(), 25);
        assert_eq!(db::collection::exclusions(&conn, id).unwrap().len(), 40);
        drop(conn);
        std::fs::remove_file(path).unwrap();
    }
}
