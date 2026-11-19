use crate::db::resources::{self, ResourceSettings};
use crate::resource_policy::{ResourcePolicy, ResourceStatus};
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn get_resource_settings(policy: State<'_, ResourcePolicy>) -> ResourceStatus {
    policy.status()
}

#[tauri::command]
pub fn set_resource_settings(
    app: AppHandle,
    db_state: State<'_, Mutex<Connection>>,
    policy: State<'_, ResourcePolicy>,
    schedule_changed: State<'_, Arc<tokio::sync::Notify>>,
    expected_revision: i64,
    settings: ResourceSettings,
) -> Result<ResourceStatus, String> {
    {
        let conn = db_state
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let snapshot = resources::save(&conn, expected_revision, settings)?;
        // Keep persistence and runtime publication in the same DB lock order.
        policy.apply(snapshot);
    }
    schedule_changed.notify_one();
    let status = policy.status();
    // Persistence is authoritative; a UI event delivery failure is not a failed save.
    let _ = app.emit("resource-settings-changed", &status);
    Ok(status)
}

#[tauri::command]
pub fn set_resource_visibility(policy: State<'_, ResourcePolicy>, hidden: bool) {
    policy.set_document_hidden(hidden);
}
