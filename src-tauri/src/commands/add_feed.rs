//! The `add_feed` Tauri command (see docs/ARCHITECTURE.md §6.1).

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use feed_rs::model::Feed;
use rusqlite::Connection;
use tauri::State;

use crate::db::{self, NewItem};
use crate::fetch;

/// Fetches and parses a feed URL. Split out from the `#[tauri::command]`
/// wrapper so it can be exercised directly in tests (incl. against real
/// network URLs) without needing a running Tauri app / managed state.
pub async fn fetch_and_parse(url: &str) -> Result<Feed, String> {
    let bytes = fetch::client::fetch_bytes(url)
        .await
        .map_err(|e| e.to_string())?;
    fetch::parser::parse(&bytes).map_err(|e| e.to_string())
}

fn feed_title(parsed: &Feed) -> Option<String> {
    parsed.title.as_ref().map(|t| t.content.clone())
}

fn to_new_items(parsed: &Feed, fetched_at: i64) -> Vec<NewItem> {
    parsed
        .entries
        .iter()
        .map(|entry| NewItem {
            guid_hash: fetch::parser::guid_hash(entry),
            title: entry.title.as_ref().map(|t| t.content.clone()),
            link: entry.links.first().map(|l| l.href.clone()),
            summary: entry.summary.as_ref().map(|t| t.content.clone()),
            published_at: entry
                .published
                .or(entry.updated)
                .map(|dt| dt.timestamp())
                .unwrap_or(fetched_at),
            fetched_at,
        })
        .collect()
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Stores a freshly parsed feed: inserts the `feeds` row, then bulk-
/// inserts its items (new ones only - see `db::upsert_items`). Returns
/// the new feed's id.
pub fn store_feed(conn: &Connection, url: &str, parsed: &Feed) -> Result<i64, String> {
    let now = now_unix();
    let format = fetch::parser::format_label(parsed);
    let title = feed_title(parsed);

    let feed_id = db::insert_feed(conn, url, title.as_deref(), Some(format), now)
        .map_err(|e| e.to_string())?;

    let items = to_new_items(parsed, now);
    db::upsert_items(conn, feed_id, &items).map_err(|e| e.to_string())?;

    Ok(feed_id)
}

#[tauri::command]
pub async fn add_feed(db_state: State<'_, Mutex<Connection>>, url: String) -> Result<i64, String> {
    let parsed = fetch_and_parse(&url).await?;

    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    store_feed(&conn, &url, &parsed)
}
