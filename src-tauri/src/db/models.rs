//! Row types for the `feeds` and `items` tables (see docs/ARCHITECTURE.md §5).

#[derive(Debug, Clone, PartialEq)]
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
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
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
