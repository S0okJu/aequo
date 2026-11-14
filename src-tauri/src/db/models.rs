//! Row types for the `feeds` and `items` tables (see docs/ARCHITECTURE.md §5).

use serde::Serialize;

/// `Feed`/`Item` derive `Serialize` since they cross the Tauri IPC
/// boundary as-is from the M6 query commands (list_feeds/list_items)
/// - no separate DTO layer for v1, the DB row shape is the view shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub format: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub interval_secs: i64,
    pub next_fetch_at: i64,
    pub last_error: Option<String>,
    pub consecutive_failures: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: i64,
    pub feed_id: i64,
    pub guid_hash: String,
    pub title: Option<String>,
    pub link: Option<String>,
    pub summary: Option<String>,
    pub published_at: i64,
    pub fetched_at: i64,
    pub is_read: bool,
}

/// A parsed feed item not yet inserted - the subset of `Item` fields
/// known before the row has an id or a read state.
#[derive(Debug, Clone, PartialEq)]
pub struct NewItem {
    pub guid_hash: String,
    pub title: Option<String>,
    pub link: Option<String>,
    pub summary: Option<String>,
    pub published_at: i64,
    pub fetched_at: i64,
}

/// The subset of `Feed` columns a fetch worker needs to run a
/// conditional request and reschedule the feed (see
/// docs/ARCHITECTURE.md §4.1). A narrower projection than `Feed` so the
/// hot polling path isn't fetching title/summary-irrelevant columns.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedMeta {
    pub id: i64,
    pub url: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub interval_secs: i64,
    pub next_fetch_at: i64,
    pub consecutive_failures: i64,
}
