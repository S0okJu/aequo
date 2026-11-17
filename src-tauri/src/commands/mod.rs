//! Tauri command handlers exposed to the frontend via `invoke` (see
//! docs/ARCHITECTURE.md §3.1, §6).

use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::db;

pub mod add_feed;
pub mod ai;
mod ai_transcript;
pub mod annotations;
pub mod article;
pub mod refresh_feed;
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

#[tauri::command]
pub fn list_items(
    db_state: State<'_, Mutex<Connection>>,
    limit: i64,
    cursor_published_at: Option<i64>,
    cursor_id: Option<i64>,
    unread_only: bool,
    feed_id: Option<i64>,
    folder_id: Option<i64>,
    saved_only: bool,
) -> Result<Vec<db::Item>, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::list_items(
        &conn,
        limit,
        cursor_published_at,
        cursor_id,
        unread_only,
        feed_id,
        folder_id,
        saved_only,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mark_read(db_state: State<'_, Mutex<Connection>>, item_id: i64) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::mark_read(&conn, item_id, now_unix()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mark_feed_read(db_state: State<'_, Mutex<Connection>>, feed_id: i64) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::mark_feed_read(&conn, feed_id, now_unix()).map_err(|e| e.to_string())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[tauri::command]
pub fn delete_feed(db_state: State<'_, Mutex<Connection>>, feed_id: i64) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let deleted = db::delete_feed(&conn, feed_id).map_err(|e| e.to_string())?;
    if deleted {
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
