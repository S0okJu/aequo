//! Tauri command handlers exposed to the frontend via `invoke` (see
//! docs/ARCHITECTURE.md §3.1, §6).

use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::db;

pub mod add_feed;
pub mod refresh_feed;

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

#[tauri::command]
pub fn list_items(
    db_state: State<'_, Mutex<Connection>>,
    limit: i64,
    offset: i64,
    unread_only: bool,
    feed_id: Option<i64>,
) -> Result<Vec<db::Item>, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::list_items(&conn, limit, offset, unread_only, feed_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mark_read(db_state: State<'_, Mutex<Connection>>, item_id: i64) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::mark_read(&conn, item_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mark_feed_read(db_state: State<'_, Mutex<Connection>>, feed_id: i64) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::mark_feed_read(&conn, feed_id).map_err(|e| e.to_string())
}
