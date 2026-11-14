//! Fetch worker: polls a single feed with a conditional request and
//! applies the `304`/`200` response branches (see docs/ARCHITECTURE.md
//! §4.1, §6.2). Dispatching this across many feeds under bounded
//! concurrency is M4's job - this module only handles one feed at a
//! time.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::db::{self, FeedMeta};

use super::backoff;
use super::client::{self, ConditionalFetch};
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

/// Polls one feed: sends a conditional GET using its stored
/// etag/last_modified, then either just reschedules it (304), parses
/// the body and stores new items (200), or records the failure (§7)
/// if the fetch itself errored.
pub async fn poll_feed(conn: &Connection, meta: &FeedMeta) -> Result<PollOutcome, String> {
    match client::fetch_conditional(
        &meta.url,
        meta.etag.as_deref(),
        meta.last_modified.as_deref(),
    )
    .await
    {
        Ok(fetched) => apply_fetch(conn, meta, fetched),
        Err(err) => record_failure(conn, meta, &client::describe_error(&err)),
    }
}

/// Applies an already-fetched response to the DB. Split out from
/// [`poll_feed`] so the Poll Scheduler (M4) can run the network fetch
/// without holding any DB lock across the await, then call this
/// synchronously once it has the lock - a `MutexGuard` held across an
/// `.await` inside a task spawned on `JoinSet`/`tokio::spawn` wouldn't
/// compile (the guard isn't `Send`).
pub fn apply_fetch(
    conn: &Connection,
    meta: &FeedMeta,
    fetched: ConditionalFetch,
) -> Result<PollOutcome, String> {
    match fetched {
        ConditionalFetch::NotModified => {
            let next_fetch_at = now_unix() + meta.interval_secs;
            db::update_feed_not_modified(conn, meta.id, next_fetch_at)
                .map_err(|e| e.to_string())?;
            Ok(PollOutcome::NotModified)
        }
        ConditionalFetch::Modified {
            bytes,
            etag,
            last_modified,
        } => match parser::parse(&bytes) {
            Ok(parsed) => {
                let now = now_unix();
                let items = parser::to_new_items(&parsed, now);
                let new_item_count =
                    db::upsert_items(conn, meta.id, &items).map_err(|e| e.to_string())?;

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
            // Parsing failed even though the fetch succeeded: skip
            // this cycle only, retry on the next poll (§7). etag/
            // last_modified are deliberately left untouched here (see
            // record_failure) so a subsequent identical 200 can be
            // re-attempted rather than silently becoming a 304.
            Err(parse_err) => record_failure(conn, meta, &parse_err.to_string()),
        },
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
    let next_fetch_at =
        now_unix() + backoff::backoff_secs(consecutive_failures, meta.interval_secs);

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
}
