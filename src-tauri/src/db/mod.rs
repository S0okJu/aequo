//! SQLite connection setup (WAL mode), schema, and repository functions
//! for feeds and items (see docs/ARCHITECTURE.md §5).

mod connection;
mod models;
mod repository;
mod schema;

pub use connection::{init, resolve_db_path};
pub use models::{
    AiSession, Annotation, ArticleCache, CleanupPreview, CleanupResult, Feed, FeedFolder, FeedMeta,
    Item, ItemCursor, ItemFilter, NewItem, RetentionSettings,
};
pub use repository::{
    annotations_overlapping, count_unread, create_ai_session, create_annotation,
    create_feed_folder, delete_ai_session, delete_annotation, delete_feed, delete_feed_folder,
    feeds_needing_icons, get_ai_session, get_annotation, get_article_cache, get_feed_meta,
    get_item_by_id, get_retention_settings, guid_hashes_with_full_content, insert_feed,
    list_ai_sessions, list_annotations, list_due_feed_metas, list_feed_folders, list_feed_icons,
    list_feed_metas, list_feeds, list_items, mark_items_read, mark_items_unread, mark_read,
    preview_retention_cleanup, rename_feed_folder, run_retention_cleanup, save_article_cache,
    save_feed_icon, set_feed_language, set_item_saved, set_retention_settings,
    update_ai_session_provider_id, update_annotation, update_feed_failed, update_feed_fetched,
    update_feed_interval, update_feed_not_modified, update_feed_organization, upsert_items,
};

#[cfg(test)]
pub use repository::get_feed_by_id;

#[cfg(test)]
pub use connection::init_in_memory;
