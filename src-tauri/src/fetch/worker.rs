//! Fetch worker: polls a single feed with a conditional request and
//! applies the `304`/`200` response branches (see docs/ARCHITECTURE.md
//! §4.1, §6.2). Dispatching this across many feeds under bounded
//! concurrency is M4's job - this module only handles one feed at a
//! time.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::db::{self, FeedMeta};

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
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Polls one feed: sends a conditional GET using its stored
/// etag/last_modified, then either just reschedules it (304) or
/// parses the body, stores new items, and updates its
/// etag/last_modified (200).
pub async fn poll_feed(conn: &Connection, meta: &FeedMeta) -> Result<PollOutcome, String> {
    let fetched = client::fetch_conditional(
        &meta.url,
        meta.etag.as_deref(),
        meta.last_modified.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    let next_fetch_at = now_unix() + meta.interval_secs;

    match fetched {
        ConditionalFetch::NotModified => {
            db::update_feed_not_modified(conn, meta.id, next_fetch_at)
                .map_err(|e| e.to_string())?;
            Ok(PollOutcome::NotModified)
        }
        ConditionalFetch::Modified {
            bytes,
            etag,
            last_modified,
        } => {
            let parsed = parser::parse(&bytes).map_err(|e| e.to_string())?;
            let items = parser::to_new_items(&parsed, now_unix());
            let new_item_count =
                db::upsert_items(conn, meta.id, &items).map_err(|e| e.to_string())?;

            db::update_feed_fetched(
                conn,
                meta.id,
                etag.as_deref(),
                last_modified.as_deref(),
                next_fetch_at,
            )
            .map_err(|e| e.to_string())?;

            Ok(PollOutcome::Updated { new_item_count })
        }
    }
}
