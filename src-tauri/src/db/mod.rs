//! SQLite connection setup (WAL mode), schema, and repository functions
//! for feeds and items (see docs/ARCHITECTURE.md §5).

mod connection;
mod models;
mod repository;
mod schema;

pub use connection::{init, resolve_db_path};
pub use models::{Feed, FeedMeta, Item, NewItem};
pub use repository::{
    count_unread, get_feed_by_id, get_feed_meta, insert_feed, list_feed_metas, list_feeds,
    list_items, mark_feed_read, mark_read, update_feed_fetched, update_feed_not_modified,
    upsert_items,
};

#[cfg(test)]
pub use connection::init_in_memory;
