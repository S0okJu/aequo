//! SQLite connection setup (WAL mode), schema, and repository functions
//! for feeds and items (see docs/ARCHITECTURE.md §5).

mod connection;
mod models;
mod repository;
mod schema;

pub use connection::{init, resolve_db_path};
pub use models::{Feed, Item, NewItem};
pub use repository::{
    count_unread, get_feed_by_id, insert_feed, list_feeds, list_items, mark_feed_read, mark_read,
    upsert_items,
};

#[cfg(test)]
pub use connection::init_in_memory;
