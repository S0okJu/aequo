//! Background feed-icon sync (docs/UI_SIMPLIFICATION_PLAN.md §3.12): one
//! long-lived task fetches icons for feeds that lack one (or whose icon is
//! due a refresh/retry), one feed at a time so it never competes with feed
//! polling for bandwidth. It runs at startup, whenever `add_feed` signals
//! [`IconSync`], and every few hours. Each stored icon is announced with a
//! `feed-icon-updated` event so the frontend can pick it up without polling.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::Notify;

use crate::db;
use crate::fetch::favicon;

/// Wakes the sync loop early (e.g. right after a feed is added).
#[derive(Clone, Default)]
pub struct IconSync(pub Arc<Notify>);

const PERIODIC_CHECK: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Serialize)]
pub struct FeedIconUpdatedPayload {
    #[serde(rename = "feedId")]
    pub feed_id: i64,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn run<R: Runtime>(conn: Arc<Mutex<Connection>>, app: AppHandle<R>, wake: IconSync) {
    loop {
        sync_due(&conn, &app).await;
        tokio::select! {
            _ = wake.0.notified() => {}
            _ = tokio::time::sleep(PERIODIC_CHECK) => {}
        }
    }
}

async fn sync_due<R: Runtime>(conn: &Arc<Mutex<Connection>>, app: &AppHandle<R>) {
    let now = now_unix();
    let due = {
        let Ok(conn) = conn.lock() else { return };
        db::feeds_needing_icons(
            &conn,
            now - favicon::REFRESH_AFTER_SECS,
            now - favicon::RETRY_FAILED_AFTER_SECS,
        )
        .unwrap_or_default()
    };

    for (feed_id, url) in due {
        // Network first, lock only for the write.
        let icon = favicon::fetch_for_feed(&url).await;
        let Ok(conn) = conn.lock() else { return };
        let saved = match &icon {
            Some(icon) => db::save_feed_icon(
                &conn,
                feed_id,
                icon.mime,
                &icon.data,
                Some(&icon.source_url),
                now_unix(),
            ),
            None => db::save_feed_icon(&conn, feed_id, "", &[], None, now_unix()),
        };
        if saved.is_ok() && icon.is_some() {
            let _ = app.emit("feed-icon-updated", FeedIconUpdatedPayload { feed_id });
        }
    }
}
