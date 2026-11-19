//! Frontend-facing events (see docs/ARCHITECTURE.md §3.2, §6.2): the
//! backend pushes `feed-updated` when new items appear; the frontend
//! never polls the backend for changes. Used by both the background
//! scheduler and the manual `refresh_feed` command, which is why this
//! lives at the crate root rather than under `commands` or
//! `scheduler` - either one depending on the other would invert the
//! natural layering (both are callers of this, not of each other).

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::fetch::worker::PollOutcome;

#[derive(Debug, Clone, Serialize)]
pub struct FeedUpdatedPayload {
    #[serde(rename = "feedId")]
    pub feed_id: i64,
    #[serde(rename = "newItemCount")]
    pub new_item_count: usize,
    pub reconciled: bool,
}

/// Emits on inserted items or repaired website rows (including zero inserts). No
/// event for a `304`, a failure, or a `200` where every item was
/// already known (dedup found nothing new). Generic over the Tauri
/// runtime so tests can pass a mock `AppHandle` (`tauri::test`)
/// instead of the production `Wry` one.
pub fn emit_if_updated<R: Runtime>(app: &AppHandle<R>, feed_id: i64, outcome: &PollOutcome) {
    let count = match outcome {
        PollOutcome::Updated { new_item_count } if *new_item_count > 0 => Some(*new_item_count),
        PollOutcome::Reconciled { new_item_count } => Some(*new_item_count),
        _ => None,
    };
    if let Some(new_item_count) = count {
        let _ = app.emit(
            "feed-updated",
            FeedUpdatedPayload {
                feed_id,
                new_item_count,
                reconciled: matches!(outcome, PollOutcome::Reconciled { .. }),
            },
        );
    }
}
