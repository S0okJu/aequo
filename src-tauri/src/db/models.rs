//! Row types for the `feeds` and `items` tables (see docs/ARCHITECTURE.md §5).

use serde::{Deserialize, Serialize};

/// Which items a timeline view covers - shared by `list_items` and the bulk
/// read/unread commands so "mark all read" always acts on exactly what the
/// current view lists. Every field is optional on the wire (`default`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemFilter {
    pub unread_only: bool,
    pub saved_only: bool,
    pub feed_id: Option<i64>,
    pub folder_id: Option<i64>,
    /// Inclusive lower / exclusive upper bound on `published_at` (the
    /// Today view).
    pub published_from: Option<i64>,
    pub published_to: Option<i64>,
}

/// Keyset pagination position: "strictly older than this row".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemCursor {
    pub published_at: i64,
    pub id: i64,
}

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
    pub folder_id: Option<i64>,
    pub hidden_from_all: bool,
    pub language: Option<String>,
    /// A display name the user chose; shown instead of `title` when set.
    pub custom_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedFolder {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
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
    pub full_content: Option<String>,
    pub published_at: i64,
    pub fetched_at: i64,
    pub is_read: bool,
    pub read_at: Option<i64>,
    pub is_saved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSettings {
    pub read_retention_days: i64,
    pub full_text_retention_days: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPreview {
    pub articles_to_remove: i64,
    pub full_texts_to_clear: i64,
    pub estimated_bytes_to_free: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub articles_removed: i64,
    pub full_texts_cleared: i64,
    pub bytes_freed: i64,
}

/// A parsed feed item not yet inserted - the subset of `Item` fields
/// known before the row has an id or a read state.
#[derive(Debug, Clone, PartialEq)]
pub struct NewItem {
    pub guid_hash: String,
    pub title: Option<String>,
    pub link: Option<String>,
    pub summary: Option<String>,
    pub full_content: Option<String>,
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
    pub format: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub interval_secs: i64,
    pub next_fetch_at: i64,
    pub last_checked_at: i64,
    pub consecutive_failures: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSession {
    pub id: i64,
    pub item_id: i64,
    pub provider: String,
    pub provider_session_id: Option<String>,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub id: i64,
    pub item_id: i64,
    pub kind: String,
    pub content_hash: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub quoted_text: String,
    pub color: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArticleCache {
    pub item_id: i64,
    pub original_url: String,
    pub final_url: Option<String>,
    pub title: Option<String>,
    pub content_html: Option<String>,
    pub text_content: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub fetched_at: i64,
    pub error: Option<String>,
}
