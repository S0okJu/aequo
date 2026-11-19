use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::db::{self, CleanupPreview, CleanupResult, RetentionSettings};

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[tauri::command]
pub fn get_retention_settings(
    db_state: State<'_, Mutex<Connection>>,
) -> Result<RetentionSettings, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::get_retention_settings(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_retention_settings(
    db_state: State<'_, Mutex<Connection>>,
    settings: RetentionSettings,
) -> Result<(), String> {
    if ![0, 30, 90, 180, 365].contains(&settings.read_retention_days)
        || ![0, 30, 90, 180, 365].contains(&settings.full_text_retention_days)
    {
        return Err("unsupported retention period".into());
    }
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::set_retention_settings(&conn, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_retention_cleanup(
    db_state: State<'_, Mutex<Connection>>,
) -> Result<CleanupPreview, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::preview_retention_cleanup(&conn, now_unix()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn run_retention_cleanup(
    db_state: State<'_, Mutex<Connection>>,
) -> Result<CleanupResult, String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db::run_retention_cleanup(&conn, now_unix()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_item_saved(
    db_state: State<'_, Mutex<Connection>>,
    item_id: i64,
    saved: bool,
) -> Result<(), String> {
    let conn = db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    if db::set_item_saved(&conn, item_id, saved).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err(format!("item {item_id} not found"))
    }
}
