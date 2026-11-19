//! The `refresh_feed` Tauri command: a manual, single-feed poll
//! triggered by the user (e.g. a refresh button), independent of the
//! scheduler's regular cycle (see docs/ARCHITECTURE.md §3.1).

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, State};

use crate::db;
use crate::events::emit_if_updated;
use crate::fetch::worker::{self, PollOutcome};
use crate::resource_policy::ResourcePolicy;

#[tauri::command]
pub async fn refresh_feed(
    app: AppHandle,
    db_state: State<'_, Mutex<Connection>>,
    policy: State<'_, ResourcePolicy>,
    feed_id: i64,
) -> Result<String, String> {
    let meta = {
        let conn = db_state
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db::get_feed_meta(&conn, feed_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("feed {feed_id} not found"))?
    };

    let outcome = worker::poll_feed_locked(&db_state, &meta, &policy, true).await?;
    emit_if_updated(&app, feed_id, &outcome);

    match outcome {
        PollOutcome::Deferred { until } => Err(format!("Feed retry is paused until {until}.")),
        PollOutcome::Updated { new_item_count } | PollOutcome::Reconciled { new_item_count } => {
            Ok(format!("{new_item_count} new item(s)"))
        }
        PollOutcome::NotModified => Ok("not modified".to_string()),
        PollOutcome::Failed { error } => Err(error),
    }
}
