//! Tauri command handlers exposed to the frontend via `invoke` (see
//! docs/ARCHITECTURE.md §3.1, §6).

use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::db;
use crate::fetch;
use crate::fetch::raw_store::{FeedRawContent, RawContentStore};

pub mod add_feed;
pub mod ai;
mod ai_io;
mod ai_stream;
mod ai_transcript;
pub mod annotations;
pub mod article;
pub mod refresh_feed;
pub mod resources;
pub mod storage;

/// A feed plus its unread count (ARCHITECTURE.md §8: "피드별 안읽음
/// 카운트는 SQLite에서 COUNT(*) WHERE is_read = 0으로 집계") - the
/// shape the feed-list sidebar actually needs, not a bare `Feed` row.
#[derive(Debug, Serialize)]
pub struct FeedSummary {
    #[serde(flatten)]
    pub feed: db::Feed,
    #[serde(rename = "unreadCount")]
    pub unread_count: i64,
}

#[tauri::command]
pub fn list_feeds(db_state: State<'_, Mutex<Connection>>) -> Result<Vec<FeedSummary>, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let feeds = db::list_feeds(&conn).map_err(|e| e.to_string())?;

    feeds
        .into_iter()
        .map(|feed| {
            let unread_count = db::count_unread(&conn, feed.id).map_err(|e| e.to_string())?;
            Ok(FeedSummary { feed, unread_count })
        })
        .collect()
}

/// Small batches of requested icons; never transfer every subscription at once.
#[tauri::command]
pub fn get_feed_icons(
    db_state: State<'_, Mutex<Connection>>,
    feed_ids: Vec<i64>,
) -> Result<std::collections::HashMap<i64, String>, String> {
    if feed_ids.len() > 32 {
        return Err("At most 32 feed icons may be requested at once".into());
    }
    use base64::Engine;
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let icons = db::list_feed_icons(&conn, &feed_ids).map_err(|e| e.to_string())?;
    Ok(icons
        .into_iter()
        .map(|(feed_id, mime, data)| {
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            (feed_id, format!("data:{mime};base64,{encoded}"))
        })
        .collect())
}

#[tauri::command]
pub fn list_items(
    db_state: State<'_, Mutex<Connection>>,
    filter: db::ItemFilter,
    limit: i64,
    cursor_published_at: Option<i64>,
    cursor_id: Option<i64>,
    newer: Option<bool>,
) -> Result<Vec<db::ItemSummary>, String> {
    if !(1..=100).contains(&limit) {
        return Err("Page size must be between 1 and 100".into());
    }
    if cursor_published_at.is_some() != cursor_id.is_some() {
        return Err("Both cursor values are required".into());
    }
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let cursor = cursor_published_at
        .zip(cursor_id)
        .map(|(published_at, id)| db::ItemCursor { published_at, id });
    if newer.unwrap_or(false) {
        let cursor = cursor.ok_or_else(|| "A cursor is required for a newer page".to_string())?;
        db::list_newer_items(&conn, &filter, limit, cursor).map_err(|e| e.to_string())
    } else {
        db::list_items(&conn, &filter, limit, cursor).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn get_item_summary(
    db_state: State<'_, Mutex<Connection>>,
    item_id: i64,
) -> Result<Option<db::ItemSummary>, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::get_item_summary_by_id(&conn, item_id).map_err(|error| error.to_string())
}

/// The raw document behind a subscription as of its last successful
/// poll - `None` if it hasn't completed one yet since this feature
/// shipped (or ever, for a feed that only ever 404s/times out).
///
/// A `format = 'website'` feed never had a real feed document to begin
/// with, so instead of the cached scraped HTML (confusing next to "View
/// RSS feed"), this synthesizes one from its already-stored items -
/// clearly marked as auto-generated so it's never mistaken for
/// something the site actually serves.
#[tauri::command]
pub fn get_feed_raw_content(
    db_state: State<'_, Mutex<Connection>>,
    raw_store: State<'_, RawContentStore>,
    feed_id: i64,
) -> Result<Option<FeedRawContent>, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    feed_raw_content(&conn, &raw_store, feed_id)
}

/// Split out from the `#[tauri::command]` wrapper so it can be exercised
/// directly in tests without needing a running Tauri app / managed state.
fn feed_raw_content(
    conn: &Connection,
    raw_store: &RawContentStore,
    feed_id: i64,
) -> Result<Option<FeedRawContent>, String> {
    let feed = db::get_feed_by_id(conn, feed_id).map_err(|e| e.to_string())?;
    if let Some(feed) = feed.filter(|f| f.format.as_deref() == Some("website")) {
        let items = db::list_items(
            conn,
            &db::ItemFilter {
                feed_id: Some(feed_id),
                ..Default::default()
            },
            100,
            None,
        )
        .map_err(|e| e.to_string())?;
        if items.is_empty() {
            return Ok(None);
        }
        let fetched_at = items.iter().map(|item| item.fetched_at).max().unwrap_or(0);
        let synthetic: Vec<_> = items
            .iter()
            .map(|item| fetch::synthetic_feed::SyntheticItem {
                guid: &item.guid_hash,
                title: item.title.as_deref(),
                link: item.link.as_deref(),
                summary: item.preview.as_deref(),
                published_at: item.published_at,
            })
            .collect();
        let title = feed
            .custom_title
            .as_deref()
            .or(feed.title.as_deref())
            .unwrap_or(&feed.url);
        return Ok(Some(FeedRawContent {
            content: fetch::synthetic_feed::build(title, &feed.url, &synthetic),
            fetched_at,
        }));
    }
    raw_store.read(feed_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn mark_read(db_state: State<'_, Mutex<Connection>>, item_id: i64) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::mark_read(&conn, item_id, now_unix()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn count_unread_items(
    db_state: State<'_, Mutex<Connection>>,
    filter: db::ItemFilter,
) -> Result<i64, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::count_unread_items(&conn, &filter).map_err(|error| error.to_string())
}

/// Renames a feed for display; an empty name restores the feed's own title.
#[tauri::command]
pub fn rename_feed(
    db_state: State<'_, Mutex<Connection>>,
    feed_id: i64,
    title: Option<String>,
) -> Result<(), String> {
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    if title.as_ref().is_some_and(|t| t.chars().count() > 200) {
        return Err("name is too long".to_string());
    }
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    if db::set_feed_custom_title(&conn, feed_id, title.as_deref()).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err(format!("feed {feed_id} was not found"))
    }
}

/// Marks every unread item in a view as read; returns the affected ids for
/// undo. `fetched_before` is when the frontend loaded the view.
#[tauri::command]
pub fn mark_items_read(
    db_state: State<'_, Mutex<Connection>>,
    filter: db::ItemFilter,
    fetched_before: i64,
) -> Result<Vec<i64>, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::mark_items_read(&conn, &filter, fetched_before, now_unix()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mark_items_unread(
    db_state: State<'_, Mutex<Connection>>,
    item_ids: Vec<i64>,
) -> Result<usize, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::mark_items_unread(&conn, &item_ids).map_err(|e| e.to_string())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[tauri::command]
pub fn delete_feed(
    app: AppHandle,
    db_state: State<'_, Mutex<Connection>>,
    raw_store: State<'_, RawContentStore>,
    feed_id: i64,
) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let deleted = db::delete_feed(&conn, feed_id).map_err(|e| e.to_string())?;
    if deleted {
        raw_store.delete(feed_id);
        let _ = app.emit(
            "feed-icon-updated",
            crate::icons::FeedIconUpdatedPayload { feed_id },
        );
        Ok(())
    } else {
        Err(format!("feed {feed_id} not found"))
    }
}

#[tauri::command]
pub fn list_feed_folders(
    db_state: State<'_, Mutex<Connection>>,
) -> Result<Vec<db::FeedFolder>, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::list_feed_folders(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_feed_folder(
    db_state: State<'_, Mutex<Connection>>,
    name: String,
) -> Result<i64, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("folder name cannot be empty".into());
    }
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::create_feed_folder(&conn, name, now_unix()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_feed_folder(
    db_state: State<'_, Mutex<Connection>>,
    folder_id: i64,
    name: String,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("folder name cannot be empty".into());
    }
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    if db::rename_feed_folder(&conn, folder_id, name).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err(format!("folder {folder_id} not found"))
    }
}

#[tauri::command]
pub fn delete_feed_folder(
    db_state: State<'_, Mutex<Connection>>,
    folder_id: i64,
) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    if db::delete_feed_folder(&conn, folder_id).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err(format!("folder {folder_id} not found"))
    }
}

#[tauri::command]
pub fn update_feed_organization(
    db_state: State<'_, Mutex<Connection>>,
    feed_id: i64,
    folder_id: Option<i64>,
    hidden_from_all: bool,
) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    if db::update_feed_organization(&conn, feed_id, folder_id, hidden_from_all)
        .map_err(|e| e.to_string())?
    {
        Ok(())
    } else {
        Err(format!("feed {feed_id} not found"))
    }
}

#[tauri::command]
pub fn reorder_feeds_in_folder(
    db_state: State<'_, Mutex<Connection>>,
    folder_id: Option<i64>,
    feed_ids: Vec<i64>,
) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::reorder_feeds_in_folder(&conn, folder_id, &feed_ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_feed_pinned(
    db_state: State<'_, Mutex<Connection>>,
    feed_id: i64,
    pinned: bool,
) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    if db::set_feed_pinned(&conn, feed_id, pinned).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err(format!("feed {feed_id} not found"))
    }
}

#[tauri::command]
pub fn set_feed_refresh_interval(
    db_state: State<'_, Mutex<Connection>>,
    schedule_changed: State<'_, std::sync::Arc<tokio::sync::Notify>>,
    feed_id: i64,
    interval_secs: i64,
) -> Result<(), String> {
    if ![300, 900, 1800, 3600, 21600, 43200, 86400].contains(&interval_secs) {
        return Err("unsupported refresh interval".into());
    }
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if db::update_feed_interval(&conn, feed_id, interval_secs, now + interval_secs)
        .map_err(|e| e.to_string())?
    {
        schedule_changed.notify_one();
        Ok(())
    } else {
        Err(format!("feed {feed_id} not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewItem;
    use crate::fetch::raw_store::temp_store;

    fn item(guid: &str, title: &str, published_at: i64) -> NewItem {
        NewItem {
            guid_hash: guid.to_string(),
            title: Some(title.to_string()),
            link: Some(format!("https://example.com/{guid}")),
            summary: Some("A summary".to_string()),
            full_content: None,
            published_at,
            published_at_kind: "date",
            fetched_at: published_at,
        }
    }

    #[test]
    fn a_website_feed_gets_a_synthesized_auto_generated_rss_feed_instead_of_its_cached_html() {
        let conn = db::init_in_memory().unwrap();
        let feed_id = db::insert_feed(
            &conn,
            "https://example.com/",
            Some("Example"),
            Some("website"),
            1000,
        )
        .unwrap();
        db::upsert_items(
            &conn,
            feed_id,
            &[
                item("a", "First Post", 2000),
                item("b", "Second Post", 1000),
            ],
        )
        .unwrap();
        let store = temp_store();
        // The scraped HTML is cached too (worker.rs always caches it for
        // website-repair), but it must not be what this returns.
        store
            .write(feed_id, b"<html><body>irrelevant</body></html>")
            .unwrap();

        let raw = feed_raw_content(&conn, &store, feed_id).unwrap().unwrap();
        assert!(raw.content.contains("Auto-generated by Aequo"));
        assert!(raw.content.contains("<title>First Post</title>"));
        assert!(raw.content.contains("<title>Second Post</title>"));
        assert!(!raw.content.contains("irrelevant"));
        assert_eq!(raw.fetched_at, 2000);

        let parsed = fetch::parser::parse(raw.content.as_bytes()).unwrap();
        assert_eq!(parsed.entries.len(), 2);
    }

    #[test]
    fn a_website_feed_with_no_items_yet_has_no_source_to_show() {
        let conn = db::init_in_memory().unwrap();
        let feed_id =
            db::insert_feed(&conn, "https://example.com/", None, Some("website"), 1000).unwrap();
        let store = temp_store();
        assert_eq!(feed_raw_content(&conn, &store, feed_id).unwrap(), None);
    }

    #[test]
    fn a_real_feed_still_returns_its_cached_source_unchanged() {
        let conn = db::init_in_memory().unwrap();
        let feed_id = db::insert_feed(
            &conn,
            "https://example.com/feed.xml",
            None,
            Some("rss"),
            1000,
        )
        .unwrap();
        let store = temp_store();
        store
            .write(
                feed_id,
                b"<rss><channel><title>Real</title></channel></rss>",
            )
            .unwrap();

        let raw = feed_raw_content(&conn, &store, feed_id).unwrap().unwrap();
        assert_eq!(
            raw.content,
            "<rss><channel><title>Real</title></channel></rss>"
        );
    }
}
