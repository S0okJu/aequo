//! Repository functions: the only way the rest of the app touches the
//! `feeds`/`items` tables (see docs/ARCHITECTURE.md §3.1, §6).

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Result, params, params_from_iter};

use super::models::{
    AiSession, Annotation, ArticleCache, CleanupPreview, CleanupResult, Feed, FeedFolder, FeedMeta,
    Item, ItemCursor, ItemFilter, NewItem, RetentionSettings,
};

fn row_to_article_cache(row: &rusqlite::Row) -> rusqlite::Result<ArticleCache> {
    Ok(ArticleCache {
        item_id: row.get(0)?,
        original_url: row.get(1)?,
        final_url: row.get(2)?,
        title: row.get(3)?,
        content_html: row.get(4)?,
        text_content: row.get(5)?,
        etag: row.get(6)?,
        last_modified: row.get(7)?,
        fetched_at: row.get(8)?,
        error: row.get(9)?,
    })
}

const ARTICLE_CACHE_COLUMNS: &str = "item_id, original_url, final_url, title, content_html, text_content, etag, last_modified, fetched_at, error";

pub fn get_article_cache(conn: &Connection, item_id: i64) -> Result<Option<ArticleCache>> {
    conn.query_row(
        &format!("SELECT {ARTICLE_CACHE_COLUMNS} FROM article_cache WHERE item_id = ?1"),
        params![item_id],
        row_to_article_cache,
    )
    .optional()
}

pub fn save_article_cache(conn: &Connection, article: &ArticleCache) -> Result<()> {
    conn.execute(
        "INSERT INTO article_cache (item_id, original_url, final_url, title, content_html, text_content, etag, last_modified, fetched_at, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(item_id) DO UPDATE SET original_url=excluded.original_url, final_url=excluded.final_url,
         title=excluded.title, content_html=excluded.content_html, text_content=excluded.text_content,
         etag=excluded.etag, last_modified=excluded.last_modified, fetched_at=excluded.fetched_at, error=excluded.error",
        params![
            article.item_id,
            article.original_url,
            article.final_url,
            article.title,
            article.content_html,
            article.text_content,
            article.etag,
            article.last_modified,
            article.fetched_at,
            article.error,
        ],
    )?;
    Ok(())
}

pub fn insert_feed(
    conn: &Connection,
    url: &str,
    title: Option<&str>,
    format: Option<&str>,
    created_at: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO feeds (url, title, format, interval_secs, next_fetch_at, last_checked_at, created_at) VALUES (?1, ?2, ?3, ?4, CASE WHEN ?3='website' THEN ?5+86400 ELSE 0 END, ?5, ?5)",
        params![url, title, format, if format == Some("website") { 86_400 } else { 900 }, created_at],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Removes a subscription and its cached items as one transaction.
pub fn delete_feed(conn: &Connection, feed_id: i64) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    // Explicitly remove descendants as well as relying on FK cascades. This
    // keeps unsubscribe correct for databases opened by older builds where
    // foreign-key enforcement may not have been enabled on every connection.
    tx.execute(
        "DELETE FROM ai_sessions WHERE item_id IN (SELECT id FROM items WHERE feed_id = ?1)",
        params![feed_id],
    )?;
    tx.execute(
        "DELETE FROM article_cache WHERE item_id IN (SELECT id FROM items WHERE feed_id = ?1)",
        params![feed_id],
    )?;
    tx.execute("DELETE FROM items WHERE feed_id = ?1", params![feed_id])?;
    let deleted = tx.execute("DELETE FROM feeds WHERE id = ?1", params![feed_id])? > 0;
    tx.commit()?;
    Ok(deleted)
}

fn row_to_feed(row: &rusqlite::Row) -> rusqlite::Result<Feed> {
    Ok(Feed {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        format: row.get(3)?,
        etag: row.get(4)?,
        last_modified: row.get(5)?,
        interval_secs: row.get(6)?,
        next_fetch_at: row.get(7)?,
        last_error: row.get(8)?,
        consecutive_failures: row.get(9)?,
        created_at: row.get(10)?,
        folder_id: row.get(11)?,
        hidden_from_all: row.get::<_, i64>(12)? != 0,
        language: row.get(13)?,
    })
}

const FEED_COLUMNS: &str = "id, url, title, format, etag, last_modified, interval_secs, next_fetch_at, last_error, consecutive_failures, created_at, folder_id, hidden_from_all, language";

fn row_to_folder(row: &rusqlite::Row) -> rusqlite::Result<FeedFolder> {
    Ok(FeedFolder {
        id: row.get(0)?,
        name: row.get(1)?,
        sort_order: row.get(2)?,
        created_at: row.get(3)?,
    })
}

pub fn list_feed_folders(conn: &Connection) -> Result<Vec<FeedFolder>> {
    let mut stmt = conn.prepare("SELECT id,name,sort_order,created_at FROM feed_folders ORDER BY sort_order,name COLLATE NOCASE")?;
    stmt.query_map([], row_to_folder)?.collect()
}

pub fn create_feed_folder(conn: &Connection, name: &str, now: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO feed_folders(name,created_at) VALUES(?1,?2)",
        params![name.trim(), now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_feed_folder(conn: &Connection, folder_id: i64, name: &str) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE feed_folders SET name=?1 WHERE id=?2",
        params![name.trim(), folder_id],
    )? > 0)
}

pub fn delete_feed_folder(conn: &Connection, folder_id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM feed_folders WHERE id=?1", params![folder_id])? > 0)
}

pub fn update_feed_organization(
    conn: &Connection,
    feed_id: i64,
    folder_id: Option<i64>,
    hidden_from_all: bool,
) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE feeds SET folder_id=?1, hidden_from_all=?2 WHERE id=?3",
        params![folder_id, hidden_from_all, feed_id],
    )? > 0)
}

pub fn update_feed_interval(
    conn: &Connection,
    feed_id: i64,
    interval_secs: i64,
    next_fetch_at: i64,
) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE feeds SET interval_secs=?1, next_fetch_at=?2, consecutive_failures=0, last_error=NULL WHERE id=?3",
        params![interval_secs, next_fetch_at, feed_id],
    )? > 0)
}

#[cfg(test)]
pub fn get_feed_by_id(conn: &Connection, feed_id: i64) -> Result<Option<Feed>> {
    conn.query_row(
        &format!("SELECT {FEED_COLUMNS} FROM feeds WHERE id = ?1"),
        params![feed_id],
        row_to_feed,
    )
    .optional()
}

pub fn list_feeds(conn: &Connection) -> Result<Vec<Feed>> {
    let mut stmt = conn.prepare(&format!("SELECT {FEED_COLUMNS} FROM feeds ORDER BY id"))?;
    let rows = stmt.query_map([], row_to_feed)?;
    rows.collect()
}

fn row_to_feed_meta(row: &rusqlite::Row) -> rusqlite::Result<FeedMeta> {
    Ok(FeedMeta {
        id: row.get(0)?,
        url: row.get(1)?,
        format: row.get(2)?,
        etag: row.get(3)?,
        last_modified: row.get(4)?,
        interval_secs: row.get(5)?,
        next_fetch_at: row.get(6)?,
        last_checked_at: row.get(7)?,
        consecutive_failures: row.get(8)?,
    })
}

const FEED_META_COLUMNS: &str = "id, url, format, etag, last_modified, interval_secs, next_fetch_at, last_checked_at, consecutive_failures";

pub fn get_feed_meta(conn: &Connection, feed_id: i64) -> Result<Option<FeedMeta>> {
    conn.query_row(
        &format!("SELECT {FEED_META_COLUMNS} FROM feeds WHERE id = ?1"),
        params![feed_id],
        row_to_feed_meta,
    )
    .optional()
}

pub fn list_feed_metas(conn: &Connection) -> Result<Vec<FeedMeta>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FEED_META_COLUMNS} FROM feeds ORDER BY id"
    ))?;
    let rows = stmt.query_map([], row_to_feed_meta)?;
    rows.collect()
}

/// Feeds whose `next_fetch_at` has passed - what the Poll Scheduler
/// dispatches each cycle (see docs/ARCHITECTURE.md §6.2).
pub fn list_due_feed_metas(conn: &Connection, now: i64) -> Result<Vec<FeedMeta>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FEED_META_COLUMNS} FROM feeds WHERE next_fetch_at <= ?1 ORDER BY next_fetch_at"
    ))?;
    let rows = stmt.query_map(params![now], row_to_feed_meta)?;
    rows.collect()
}

/// Records a `304 Not Modified` response: the next poll time moves,
/// and a clean response clears any prior failure state (§7).
pub fn update_feed_not_modified(conn: &Connection, feed_id: i64, next_fetch_at: i64) -> Result<()> {
    conn.execute(
        "UPDATE feeds SET next_fetch_at = ?1, last_checked_at = ?1 - interval_secs, consecutive_failures = 0, last_error = NULL \
         WHERE id = ?2",
        params![next_fetch_at, feed_id],
    )?;
    Ok(())
}

/// Records a `200 OK` response: the new etag/last_modified, the next
/// poll time, and clears any prior failure state (§7).
/// Records the feed's declared language (see `fetch::parser::feed_language`).
pub fn set_feed_language(conn: &Connection, feed_id: i64, language: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE feeds SET language = ?1 WHERE id = ?2",
        params![language, feed_id],
    )?;
    Ok(())
}

pub fn update_feed_fetched(
    conn: &Connection,
    feed_id: i64,
    etag: Option<&str>,
    last_modified: Option<&str>,
    next_fetch_at: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE feeds SET etag = ?1, last_modified = ?2, next_fetch_at = ?3, last_checked_at = ?3 - interval_secs, \
         consecutive_failures = 0, last_error = NULL WHERE id = ?4",
        params![etag, last_modified, next_fetch_at, feed_id],
    )?;
    Ok(())
}

/// Records a failed attempt (network error or parse error, §7):
/// `etag`/`last_modified` are left untouched (nothing new to adopt),
/// but `last_error`, `consecutive_failures`, and `next_fetch_at`
/// (backoff-scheduled, not the normal interval) all move.
pub fn update_feed_failed(
    conn: &Connection,
    feed_id: i64,
    error: &str,
    next_fetch_at: i64,
    consecutive_failures: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE feeds SET last_error = ?1, next_fetch_at = ?2, consecutive_failures = ?3 \
         WHERE id = ?4",
        params![error, next_fetch_at, consecutive_failures, feed_id],
    )?;
    Ok(())
}

/// Inserts new items for `feed_id`, skipping ones already present
/// (matched by the `UNIQUE(feed_id, guid_hash)` constraint). Returns the
/// number of rows actually inserted.
pub fn upsert_items(conn: &Connection, feed_id: i64, items: &[NewItem]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut inserted = 0usize;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO items
                (feed_id, guid_hash, title, link, summary, full_content, published_at, fetched_at, is_read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
        )?;
        for item in items {
            inserted += stmt.execute(params![
                feed_id,
                item.guid_hash,
                item.title,
                item.link,
                item.summary,
                item.full_content,
                item.published_at,
                item.fetched_at,
            ])?;
            tx.execute(
                "UPDATE items SET full_content = COALESCE(full_content, ?1) WHERE feed_id = ?2 AND guid_hash = ?3",
                params![item.full_content, feed_id, item.guid_hash],
            )?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

/// guid_hashes for this feed that already have `full_content` stored -
/// used by `fetch::enrichment` to skip re-fetching articles we already
/// have, rather than redundantly re-extracting them every poll cycle.
pub fn guid_hashes_with_full_content(
    conn: &Connection,
    feed_id: i64,
) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn
        .prepare("SELECT guid_hash FROM items WHERE feed_id = ?1 AND full_content IS NOT NULL")?;
    let rows = stmt.query_map([feed_id], |row| row.get::<_, String>(0))?;
    rows.collect()
}

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: row.get(0)?,
        feed_id: row.get(1)?,
        guid_hash: row.get(2)?,
        title: row.get(3)?,
        link: row.get(4)?,
        summary: row.get(5)?,
        full_content: row.get(6)?,
        published_at: row.get(7)?,
        fetched_at: row.get(8)?,
        is_read: row.get::<_, i64>(9)? != 0,
        read_at: row.get(10)?,
        is_saved: row.get::<_, i64>(11)? != 0,
    })
}

const ITEM_COLUMNS: &str = "id, feed_id, guid_hash, title, link, summary, full_content, published_at, fetched_at, is_read, read_at, is_saved";
const ITEM_LIST_COLUMNS: &str = "id, feed_id, guid_hash, title, link, summary, NULL, published_at, fetched_at, is_read, read_at, is_saved";

pub fn get_item_by_id(conn: &Connection, item_id: i64) -> Result<Option<Item>> {
    conn.query_row(
        &format!("SELECT {ITEM_COLUMNS} FROM items WHERE id = ?1"),
        params![item_id],
        row_to_item,
    )
    .optional()
}

fn row_to_ai_session(row: &rusqlite::Row) -> rusqlite::Result<AiSession> {
    Ok(AiSession {
        id: row.get(0)?,
        item_id: row.get(1)?,
        provider: row.get(2)?,
        provider_session_id: row.get(3)?,
        title: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

const AI_SESSION_COLUMNS: &str =
    "id, item_id, provider, provider_session_id, title, created_at, updated_at";

pub fn get_ai_session(conn: &Connection, session_id: i64) -> Result<Option<AiSession>> {
    conn.query_row(
        &format!("SELECT {AI_SESSION_COLUMNS} FROM ai_sessions WHERE id = ?1"),
        params![session_id],
        row_to_ai_session,
    )
    .optional()
}

pub fn list_ai_sessions(conn: &Connection, item_id: i64, provider: &str) -> Result<Vec<AiSession>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {AI_SESSION_COLUMNS} FROM ai_sessions WHERE item_id = ?1 AND provider = ?2 ORDER BY updated_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map(params![item_id, provider], row_to_ai_session)?;
    rows.collect()
}

pub fn create_ai_session(
    conn: &Connection,
    item_id: i64,
    provider: &str,
    provider_session_id: Option<&str>,
    title: &str,
    now: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO ai_sessions (item_id, provider, provider_session_id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![item_id, provider, provider_session_id, title, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_ai_session_provider_id(
    conn: &Connection,
    session_id: i64,
    provider_session_id: &str,
    now: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE ai_sessions SET provider_session_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![provider_session_id, now, session_id],
    )?;
    Ok(())
}

pub fn delete_ai_session(conn: &Connection, session_id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM ai_sessions WHERE id = ?1", params![session_id])? > 0)
}

/// SQL conditions (joined with AND) and their positional values for an
/// `ItemFilter`. Shared by listing and bulk updates so both always cover
/// the same rows.
fn filter_conditions(filter: &ItemFilter) -> (Vec<&'static str>, Vec<Value>) {
    let mut conditions = Vec::new();
    let mut values = Vec::new();

    if filter.unread_only {
        conditions.push("is_read = 0");
    }
    if filter.saved_only {
        conditions.push("is_saved = 1");
    }
    if let Some(id) = filter.feed_id {
        conditions.push("feed_id = ?");
        values.push(Value::Integer(id));
    } else if let Some(id) = filter.folder_id {
        conditions.push("feed_id IN (SELECT id FROM feeds WHERE folder_id = ?)");
        values.push(Value::Integer(id));
    } else if !filter.saved_only {
        // The Starred view spans every feed, including ones hidden
        // from "All" - a feed being decluttered from the merged
        // timeline shouldn't also hide items the user explicitly
        // starred from it.
        conditions.push("feed_id IN (SELECT id FROM feeds WHERE hidden_from_all = 0)");
    }
    if let Some(from) = filter.published_from {
        conditions.push("published_at >= ?");
        values.push(Value::Integer(from));
    }
    if let Some(to) = filter.published_to {
        conditions.push("published_at < ?");
        values.push(Value::Integer(to));
    }
    (conditions, values)
}

fn where_clause(conditions: &[&str]) -> String {
    if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    }
}

/// Lists items for the merged timeline, newest first (§6.3), narrowed by
/// `filter` (one feed, one folder, starred, unread, a day range). An empty
/// filter is the default "All" view across every non-hidden feed.
pub fn list_items(
    conn: &Connection,
    filter: &ItemFilter,
    limit: i64,
    cursor: Option<ItemCursor>,
) -> Result<Vec<Item>> {
    let (mut conditions, mut values) = filter_conditions(filter);

    // Keyset pagination on (published_at, id) rather than OFFSET: an
    // OFFSET is a position in a result set that background fetches keep
    // mutating mid-scroll, which drifts into skipped/duplicated rows.
    // "Strictly older than the last row we handed out" stays correct no
    // matter how many rows are inserted between pages.
    if let Some(cursor) = cursor {
        conditions.push("(published_at < ? OR (published_at = ? AND id < ?))");
        values.push(Value::Integer(cursor.published_at));
        values.push(Value::Integer(cursor.published_at));
        values.push(Value::Integer(cursor.id));
    }
    values.push(Value::Integer(limit));

    let sql = format!(
        "SELECT {ITEM_LIST_COLUMNS} FROM items {} ORDER BY published_at DESC, id DESC LIMIT ?",
        where_clause(&conditions)
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values), row_to_item)?;
    rows.collect()
}

pub fn mark_read(conn: &Connection, item_id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE items SET is_read = 1, read_at = COALESCE(read_at, ?2) WHERE id = ?1",
        params![item_id, now],
    )?;
    Ok(())
}

/// "Mark all as read" for a view: every unread item matching `filter` that
/// was already fetched at `fetched_before` (when the view was loaded), so
/// posts that arrived after the user looked are left unread. Returns the
/// affected ids so the action can be undone with [`mark_items_unread`].
pub fn mark_items_read(
    conn: &Connection,
    filter: &ItemFilter,
    fetched_before: i64,
    now: i64,
) -> Result<Vec<i64>> {
    let (mut conditions, mut values) = filter_conditions(filter);
    conditions.push("is_read = 0");
    conditions.push("fetched_at <= ?");
    values.push(Value::Integer(fetched_before));
    values.insert(0, Value::Integer(now));

    let sql = format!(
        "UPDATE items SET is_read = 1, read_at = COALESCE(read_at, ?) {} RETURNING id",
        where_clause(&conditions)
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt.query_map(params_from_iter(values), |row| row.get(0))?;
    ids.collect()
}

/// Puts items back to unread - the undo for [`mark_items_read`].
pub fn mark_items_unread(conn: &Connection, item_ids: &[i64]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    {
        let mut stmt = tx.prepare("UPDATE items SET is_read = 0, read_at = NULL WHERE id = ?1")?;
        for id in item_ids {
            changed += stmt.execute(params![id])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

pub fn set_item_saved(conn: &Connection, item_id: i64, saved: bool) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE items SET is_saved = ?1 WHERE id = ?2",
        params![saved, item_id],
    )? > 0)
}

pub fn get_retention_settings(conn: &Connection) -> Result<RetentionSettings> {
    conn.query_row(
        "SELECT read_retention_days, full_text_retention_days FROM retention_settings WHERE id = 1",
        [],
        |row| {
            Ok(RetentionSettings {
                read_retention_days: row.get(0)?,
                full_text_retention_days: row.get(1)?,
            })
        },
    )
}

pub fn set_retention_settings(conn: &Connection, settings: &RetentionSettings) -> Result<()> {
    conn.execute(
        "UPDATE retention_settings SET read_retention_days = ?1, full_text_retention_days = ?2 WHERE id = 1",
        params![settings.read_retention_days, settings.full_text_retention_days],
    )?;
    Ok(())
}

fn cleanup_preview(
    conn: &Connection,
    settings: &RetentionSettings,
    now: i64,
) -> Result<CleanupPreview> {
    let read_cutoff = now.saturating_sub(settings.read_retention_days.saturating_mul(86400));
    let body_cutoff = now.saturating_sub(settings.full_text_retention_days.saturating_mul(86400));
    let articles_to_remove: i64 = if settings.read_retention_days == 0 {
        0
    } else {
        conn.query_row("SELECT COUNT(*) FROM items WHERE is_read=1 AND is_saved=0 AND COALESCE(read_at,published_at,fetched_at) < ?1", params![read_cutoff], |r| r.get(0))?
    };
    let body_clause =
        "(i.full_content IS NOT NULL OR c.content_html IS NOT NULL OR c.text_content IS NOT NULL)";
    let full_texts_to_clear: i64 = if settings.full_text_retention_days == 0 {
        0
    } else {
        conn.query_row(&format!("SELECT COUNT(*) FROM items i LEFT JOIN article_cache c ON c.item_id=i.id WHERE i.is_read=1 AND i.is_saved=0 AND COALESCE(i.read_at,i.published_at,i.fetched_at) < ?1 AND {body_clause} AND (?2=0 OR COALESCE(i.read_at,i.published_at,i.fetched_at) >= ?3)"), params![body_cutoff, settings.read_retention_days, read_cutoff], |r| r.get(0))?
    };
    let bytes: i64 = if settings.full_text_retention_days == 0 {
        0
    } else {
        conn.query_row("SELECT COALESCE(SUM(LENGTH(CAST(i.full_content AS BLOB))),0) + COALESCE(SUM(LENGTH(CAST(c.content_html AS BLOB)) + LENGTH(CAST(c.text_content AS BLOB))),0) FROM items i LEFT JOIN article_cache c ON c.item_id=i.id WHERE i.is_read=1 AND i.is_saved=0 AND COALESCE(i.read_at,i.published_at,i.fetched_at) < ?1 AND (?2=0 OR COALESCE(i.read_at,i.published_at,i.fetched_at) >= ?3)", params![body_cutoff, settings.read_retention_days, read_cutoff], |r| r.get(0))?
    };
    Ok(CleanupPreview {
        articles_to_remove,
        full_texts_to_clear,
        estimated_bytes_to_free: bytes,
    })
}

pub fn preview_retention_cleanup(conn: &Connection, now: i64) -> Result<CleanupPreview> {
    cleanup_preview(conn, &get_retention_settings(conn)?, now)
}

pub fn run_retention_cleanup(conn: &Connection, now: i64) -> Result<CleanupResult> {
    let settings = get_retention_settings(conn)?;
    let preview = cleanup_preview(conn, &settings, now)?;
    let tx = conn.unchecked_transaction()?;
    if settings.full_text_retention_days > 0 {
        let cutoff = now.saturating_sub(settings.full_text_retention_days.saturating_mul(86400));
        tx.execute("UPDATE items SET full_content=NULL WHERE id IN (SELECT id FROM items WHERE is_read=1 AND is_saved=0 AND COALESCE(read_at,published_at,fetched_at) < ?1 AND (?2=0 OR COALESCE(read_at,published_at,fetched_at) >= ?3))", params![cutoff, settings.read_retention_days, now.saturating_sub(settings.read_retention_days.saturating_mul(86400))])?;
        tx.execute("DELETE FROM article_cache WHERE item_id IN (SELECT id FROM items WHERE is_read=1 AND is_saved=0 AND COALESCE(read_at,published_at,fetched_at) < ?1 AND (?2=0 OR COALESCE(read_at,published_at,fetched_at) >= ?3))", params![cutoff, settings.read_retention_days, now.saturating_sub(settings.read_retention_days.saturating_mul(86400))])?;
    }
    let articles_removed = if settings.read_retention_days > 0 {
        tx.execute("DELETE FROM items WHERE is_read=1 AND is_saved=0 AND COALESCE(read_at,published_at,fetched_at) < ?1", params![now.saturating_sub(settings.read_retention_days.saturating_mul(86400))])? as i64
    } else {
        0
    };
    tx.commit()?;
    if articles_removed > 0 || preview.full_texts_to_clear > 0 {
        // Reclaim SQLite pages after pruning cached bodies; otherwise the
        // database file would retain the old high-water size until reused.
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
    }
    Ok(CleanupResult {
        articles_removed,
        full_texts_cleared: preview.full_texts_to_clear,
        bytes_freed: preview.estimated_bytes_to_free,
    })
}

pub fn count_unread(conn: &Connection, feed_id: i64) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM items WHERE feed_id = ?1 AND is_read = 0",
        params![feed_id],
        |row| row.get(0),
    )
}

fn row_to_annotation(row: &rusqlite::Row) -> rusqlite::Result<Annotation> {
    Ok(Annotation {
        id: row.get(0)?,
        item_id: row.get(1)?,
        kind: row.get(2)?,
        content_hash: row.get(3)?,
        start_offset: row.get(4)?,
        end_offset: row.get(5)?,
        quoted_text: row.get(6)?,
        color: row.get(7)?,
        note: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

const ANNOTATION_COLUMNS: &str = "id, item_id, kind, content_hash, start_offset, end_offset, quoted_text, color, note, created_at, updated_at";

pub fn get_annotation(conn: &Connection, annotation_id: i64) -> Result<Option<Annotation>> {
    conn.query_row(
        &format!("SELECT {ANNOTATION_COLUMNS} FROM annotations WHERE id = ?1"),
        params![annotation_id],
        row_to_annotation,
    )
    .optional()
}

pub fn list_annotations(conn: &Connection, item_id: i64) -> Result<Vec<Annotation>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ANNOTATION_COLUMNS} FROM annotations WHERE item_id = ?1 ORDER BY start_offset ASC"
    ))?;
    let rows = stmt.query_map(params![item_id], row_to_annotation)?;
    rows.collect()
}

/// Annotations on the same content version (`content_hash`) whose
/// `[start_offset, end_offset)` range intersects the given range.
/// Scoped to a single `content_hash` because comparing offsets across
/// different document versions (before/after `refreshArticle()`) is
/// meaningless - a stale annotation's offsets only mean something
/// relative to the plain text they were anchored against.
pub fn annotations_overlapping(
    conn: &Connection,
    item_id: i64,
    content_hash: &str,
    start_offset: i64,
    end_offset: i64,
) -> Result<Vec<Annotation>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ANNOTATION_COLUMNS} FROM annotations
         WHERE item_id = ?1 AND content_hash = ?2 AND start_offset < ?4 AND end_offset > ?3"
    ))?;
    let rows = stmt.query_map(
        params![item_id, content_hash, start_offset, end_offset],
        row_to_annotation,
    )?;
    rows.collect()
}

#[allow(clippy::too_many_arguments)]
pub fn create_annotation(
    conn: &Connection,
    item_id: i64,
    kind: &str,
    content_hash: &str,
    start_offset: i64,
    end_offset: i64,
    quoted_text: &str,
    color: Option<&str>,
    note: Option<&str>,
    now: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO annotations (item_id, kind, content_hash, start_offset, end_offset, quoted_text, color, note, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            item_id,
            kind,
            content_hash,
            start_offset,
            end_offset,
            quoted_text,
            color,
            note,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Updates the mutable fields of an annotation in one shot - this
/// covers both ordinary edits (note/color) and a self-healing
/// re-anchor (offsets/hash/quoted_text) via the same statement, since
/// callers always pass the full current field set through unchanged.
#[allow(clippy::too_many_arguments)]
pub fn update_annotation(
    conn: &Connection,
    annotation_id: i64,
    content_hash: &str,
    start_offset: i64,
    end_offset: i64,
    quoted_text: &str,
    color: Option<&str>,
    note: Option<&str>,
    now: i64,
) -> Result<bool> {
    let updated = conn.execute(
        "UPDATE annotations
         SET content_hash = ?1, start_offset = ?2, end_offset = ?3, quoted_text = ?4,
             color = ?5, note = ?6, updated_at = ?7
         WHERE id = ?8",
        params![
            content_hash,
            start_offset,
            end_offset,
            quoted_text,
            color,
            note,
            now,
            annotation_id
        ],
    )?;
    Ok(updated > 0)
}

pub fn delete_annotation(conn: &Connection, annotation_id: i64) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM annotations WHERE id = ?1",
        params![annotation_id],
    )? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    fn sample_item(guid_hash: &str) -> NewItem {
        NewItem {
            guid_hash: guid_hash.to_string(),
            title: Some("Title".to_string()),
            link: Some("https://example.com/post".to_string()),
            summary: Some("Summary".to_string()),
            full_content: None,
            published_at: 1_700_000_000,
            fetched_at: 1_700_000_100,
        }
    }

    #[test]
    fn schema_creates_feeds_and_items_tables() {
        let conn = init_in_memory().unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('feeds', 'items')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 2);
    }

    #[test]
    fn duplicate_items_are_ignored_on_upsert() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();

        let first_pass = upsert_items(&conn, feed_id, &[sample_item("guid-1")]).unwrap();
        assert_eq!(first_pass, 1);

        // Same guid_hash again (simulating a re-fetch with no new content) must be ignored.
        let second_pass = upsert_items(&conn, feed_id, &[sample_item("guid-1")]).unwrap();
        assert_eq!(second_pass, 0);

        let items = list_items(&conn, &ItemFilter::default(), 10, None).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn guid_hashes_with_full_content_only_returns_populated_rows() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();

        let mut with_content = sample_item("guid-has-content");
        with_content.full_content = Some("<p>full</p>".to_string());
        upsert_items(
            &conn,
            feed_id,
            &[with_content, sample_item("guid-no-content")],
        )
        .unwrap();

        let result = guid_hashes_with_full_content(&conn, feed_id).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains("guid-has-content"));
    }

    #[test]
    fn mark_read_and_count_unread_roundtrip() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();
        upsert_items(
            &conn,
            feed_id,
            &[sample_item("guid-1"), sample_item("guid-2")],
        )
        .unwrap();

        assert_eq!(count_unread(&conn, feed_id).unwrap(), 2);

        let items = list_items(&conn, &ItemFilter::default(), 10, None).unwrap();
        mark_read(&conn, items[0].id, 1_700_000_200).unwrap();
        assert_eq!(count_unread(&conn, feed_id).unwrap(), 1);

        let filter = ItemFilter {
            feed_id: Some(feed_id),
            ..Default::default()
        };
        mark_items_read(&conn, &filter, 1_700_000_100, 1_700_000_300).unwrap();
        assert_eq!(count_unread(&conn, feed_id).unwrap(), 0);
    }

    #[test]
    fn mark_items_read_covers_the_view_and_returns_ids_for_undo() {
        let conn = init_in_memory().unwrap();
        let shown = insert_feed(&conn, "https://a.example/feed.xml", None, None, 0).unwrap();
        let hidden = insert_feed(&conn, "https://b.example/feed.xml", None, None, 0).unwrap();
        update_feed_organization(&conn, hidden, None, true).unwrap();
        upsert_items(&conn, shown, &[sample_item("a1"), sample_item("a2")]).unwrap();
        upsert_items(&conn, hidden, &[sample_item("b1")]).unwrap();
        // Arrived after the user loaded the view: must stay unread.
        let late = NewItem {
            fetched_at: 1_700_000_500,
            ..sample_item("a3")
        };
        upsert_items(&conn, shown, &[late]).unwrap();

        let ids =
            mark_items_read(&conn, &ItemFilter::default(), 1_700_000_100, 1_700_000_600).unwrap();
        assert_eq!(
            ids.len(),
            2,
            "only the shown feed's items fetched before the view loaded"
        );
        assert_eq!(count_unread(&conn, shown).unwrap(), 1);
        assert_eq!(
            count_unread(&conn, hidden).unwrap(),
            1,
            "All excludes hidden feeds"
        );

        assert_eq!(mark_items_unread(&conn, &ids).unwrap(), 2);
        assert_eq!(count_unread(&conn, shown).unwrap(), 3);
    }

    #[test]
    fn list_items_filters_by_published_range() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();
        let at = |guid: &str, published_at: i64| NewItem {
            published_at,
            ..sample_item(guid)
        };
        upsert_items(
            &conn,
            feed_id,
            &[at("old", 100), at("today", 200), at("tomorrow", 300)],
        )
        .unwrap();

        let filter = ItemFilter {
            published_from: Some(200),
            published_to: Some(300),
            ..Default::default()
        };
        let items = list_items(&conn, &filter, 10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].guid_hash, "today");
    }

    #[test]
    fn list_items_unread_only_filters_read_items() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();
        upsert_items(
            &conn,
            feed_id,
            &[sample_item("guid-1"), sample_item("guid-2")],
        )
        .unwrap();

        let all_items = list_items(&conn, &ItemFilter::default(), 10, None).unwrap();
        mark_read(&conn, all_items[0].id, 1_700_000_200).unwrap();

        let unread_items = list_items(
            &conn,
            &ItemFilter {
                unread_only: true,
                ..Default::default()
            },
            10,
            None,
        )
        .unwrap();
        assert_eq!(unread_items.len(), 1);
        assert!(!unread_items[0].is_read);
    }

    #[test]
    fn list_items_feed_id_filters_to_one_feed() {
        let conn = init_in_memory().unwrap();
        let feed_a = insert_feed(&conn, "https://example.com/a.xml", None, None, 0).unwrap();
        let feed_b = insert_feed(&conn, "https://example.com/b.xml", None, None, 0).unwrap();
        upsert_items(
            &conn,
            feed_a,
            &[sample_item("guid-a1"), sample_item("guid-a2")],
        )
        .unwrap();
        upsert_items(&conn, feed_b, &[sample_item("guid-b1")]).unwrap();

        let all_items = list_items(&conn, &ItemFilter::default(), 10, None).unwrap();
        assert_eq!(all_items.len(), 3);

        let feed_a_items = list_items(
            &conn,
            &ItemFilter {
                feed_id: Some(feed_a),
                ..Default::default()
            },
            10,
            None,
        )
        .unwrap();
        assert_eq!(feed_a_items.len(), 2);
        assert!(feed_a_items.iter().all(|item| item.feed_id == feed_a));

        let feed_b_items = list_items(
            &conn,
            &ItemFilter {
                feed_id: Some(feed_b),
                ..Default::default()
            },
            10,
            None,
        )
        .unwrap();
        assert_eq!(feed_b_items.len(), 1);

        // Combined with unread_only: mark one of feed_a's items read,
        // confirm the filters compose (WHERE ... AND ...).
        mark_read(&conn, feed_a_items[0].id, 1_700_000_200).unwrap();
        let feed_a_unread = list_items(
            &conn,
            &ItemFilter {
                unread_only: true,
                feed_id: Some(feed_a),
                ..Default::default()
            },
            10,
            None,
        )
        .unwrap();
        assert_eq!(feed_a_unread.len(), 1);
    }

    #[test]
    fn list_items_saved_only_filters_to_starred_items_including_hidden_feeds() {
        let conn = init_in_memory().unwrap();
        let visible_feed = insert_feed(&conn, "https://example.com/a.xml", None, None, 0).unwrap();
        let hidden_feed = insert_feed(&conn, "https://example.com/b.xml", None, None, 0).unwrap();
        update_feed_organization(&conn, hidden_feed, None, true).unwrap();

        upsert_items(&conn, visible_feed, &[sample_item("guid-a1")]).unwrap();
        upsert_items(&conn, hidden_feed, &[sample_item("guid-b1")]).unwrap();

        let all_items = list_items(&conn, &ItemFilter::default(), 10, None).unwrap();
        assert_eq!(
            all_items.len(),
            1,
            "hidden feed is excluded from the default view"
        );

        let starred_item = all_items
            .iter()
            .find(|item| item.feed_id == visible_feed)
            .unwrap();
        // Star the hidden feed's item too, not the visible one, to prove
        // the Starred view ignores hidden_from_all entirely.
        let hidden_item_id = list_items(
            &conn,
            &ItemFilter {
                feed_id: Some(hidden_feed),
                ..Default::default()
            },
            10,
            None,
        )
        .unwrap()[0]
            .id;
        set_item_saved(&conn, hidden_item_id, true).unwrap();

        let starred = list_items(
            &conn,
            &ItemFilter {
                saved_only: true,
                ..Default::default()
            },
            10,
            None,
        )
        .unwrap();
        assert_eq!(starred.len(), 1);
        assert_eq!(starred[0].id, hidden_item_id);
        assert!(starred[0].is_saved);
        assert_ne!(starred[0].id, starred_item.id);
    }

    #[test]
    fn get_feed_by_id_and_list_feeds() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(
            &conn,
            "https://example.com/feed.xml",
            Some("Example"),
            Some("rss"),
            0,
        )
        .unwrap();

        let feed = get_feed_by_id(&conn, feed_id).unwrap().unwrap();
        assert_eq!(feed.url, "https://example.com/feed.xml");
        assert_eq!(feed.title, Some("Example".to_string()));

        assert!(get_feed_by_id(&conn, feed_id + 1).unwrap().is_none());
        assert_eq!(list_feeds(&conn).unwrap().len(), 1);
    }

    #[test]
    fn feed_meta_defaults_and_not_modified_update() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();

        let meta = get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(meta.etag, None);
        assert_eq!(meta.last_modified, None);
        assert_eq!(meta.interval_secs, 900); // schema default
        assert_eq!(meta.next_fetch_at, 0); // schema default

        update_feed_not_modified(&conn, feed_id, 1_700_000_900).unwrap();
        let meta = get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(meta.next_fetch_at, 1_700_000_900);
        assert_eq!(meta.etag, None); // untouched by a 304

        assert_eq!(list_feed_metas(&conn).unwrap().len(), 1);
    }

    #[test]
    fn feed_meta_fetched_update() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();

        update_feed_fetched(
            &conn,
            feed_id,
            Some("etag-1"),
            Some("Mon, 01 Jan 2024"),
            999,
        )
        .unwrap();
        let meta = get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(meta.etag, Some("etag-1".to_string()));
        assert_eq!(meta.last_modified, Some("Mon, 01 Jan 2024".to_string()));
        assert_eq!(meta.next_fetch_at, 999);
    }

    #[test]
    fn list_due_feed_metas_filters_by_next_fetch_at() {
        let conn = init_in_memory().unwrap();
        let due_feed = insert_feed(&conn, "https://example.com/due.xml", None, None, 0).unwrap();
        let future_feed =
            insert_feed(&conn, "https://example.com/future.xml", None, None, 0).unwrap();

        update_feed_not_modified(&conn, due_feed, 1000).unwrap();
        update_feed_not_modified(&conn, future_feed, 5000).unwrap();

        let due = list_due_feed_metas(&conn, 2000).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, due_feed);

        let all_due = list_due_feed_metas(&conn, 9999).unwrap();
        assert_eq!(all_due.len(), 2);
    }

    #[test]
    fn update_feed_failed_records_error_without_touching_etag() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();
        update_feed_fetched(&conn, feed_id, Some("etag-1"), Some("lm-1"), 500).unwrap();

        update_feed_failed(&conn, feed_id, "connection timed out", 600, 1).unwrap();

        let meta = get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(meta.next_fetch_at, 600);
        assert_eq!(meta.consecutive_failures, 1);
        assert_eq!(meta.etag, Some("etag-1".to_string())); // untouched by a failure
        assert_eq!(meta.last_modified, Some("lm-1".to_string()));

        let feed = get_feed_by_id(&conn, feed_id).unwrap().unwrap();
        assert_eq!(feed.last_error, Some("connection timed out".to_string()));
    }

    #[test]
    fn success_paths_clear_prior_failure_state() {
        let conn = init_in_memory().unwrap();
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();
        update_feed_failed(&conn, feed_id, "boom", 100, 2).unwrap();

        update_feed_not_modified(&conn, feed_id, 200).unwrap();
        let meta = get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(meta.consecutive_failures, 0);
        let feed = get_feed_by_id(&conn, feed_id).unwrap().unwrap();
        assert_eq!(feed.last_error, None);

        update_feed_failed(&conn, feed_id, "boom again", 300, 1).unwrap();
        update_feed_fetched(&conn, feed_id, Some("etag-2"), None, 400).unwrap();
        let meta = get_feed_meta(&conn, feed_id).unwrap().unwrap();
        assert_eq!(meta.consecutive_failures, 0);
        let feed = get_feed_by_id(&conn, feed_id).unwrap().unwrap();
        assert_eq!(feed.last_error, None);
    }

    fn sample_item_id(conn: &Connection) -> i64 {
        let feed_id = insert_feed(&conn, "https://example.com/feed.xml", None, None, 0).unwrap();
        upsert_items(conn, feed_id, &[sample_item("guid-1")]).unwrap();
        list_items(conn, &ItemFilter::default(), 10, None).unwrap()[0].id
    }

    #[test]
    fn annotation_crud_roundtrip() {
        let conn = init_in_memory().unwrap();
        let item_id = sample_item_id(&conn);

        let id = create_annotation(
            &conn,
            item_id,
            "highlight",
            "hash-1",
            0,
            5,
            "hello",
            Some("yellow"),
            None,
            1_700_000_000,
        )
        .unwrap();

        let annotation = get_annotation(&conn, id).unwrap().unwrap();
        assert_eq!(annotation.kind, "highlight");
        assert_eq!(annotation.color, Some("yellow".to_string()));
        assert_eq!(annotation.note, None);

        let updated = update_annotation(
            &conn,
            id,
            "hash-1",
            0,
            5,
            "hello",
            Some("green"),
            None,
            1_700_000_100,
        )
        .unwrap();
        assert!(updated);
        let annotation = get_annotation(&conn, id).unwrap().unwrap();
        assert_eq!(annotation.color, Some("green".to_string()));

        let deleted = delete_annotation(&conn, id).unwrap();
        assert!(deleted);
        assert!(get_annotation(&conn, id).unwrap().is_none());
    }

    #[test]
    fn list_annotations_orders_by_start_offset() {
        let conn = init_in_memory().unwrap();
        let item_id = sample_item_id(&conn);

        create_annotation(
            &conn,
            item_id,
            "highlight",
            "hash-1",
            10,
            15,
            "world",
            Some("pink"),
            None,
            1,
        )
        .unwrap();
        create_annotation(
            &conn,
            item_id,
            "highlight",
            "hash-1",
            0,
            5,
            "hello",
            Some("yellow"),
            None,
            1,
        )
        .unwrap();

        let annotations = list_annotations(&conn, item_id).unwrap();
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].start_offset, 0);
        assert_eq!(annotations[1].start_offset, 10);
    }

    #[test]
    fn annotations_overlapping_detects_intersection_within_same_content_hash() {
        let conn = init_in_memory().unwrap();
        let item_id = sample_item_id(&conn);

        create_annotation(
            &conn,
            item_id,
            "highlight",
            "hash-1",
            0,
            10,
            "hello worl",
            Some("yellow"),
            None,
            1,
        )
        .unwrap();

        // Overlapping range, same content_hash -> detected.
        let overlaps = annotations_overlapping(&conn, item_id, "hash-1", 5, 15).unwrap();
        assert_eq!(overlaps.len(), 1);

        // Non-overlapping range, same content_hash -> not detected.
        let no_overlap = annotations_overlapping(&conn, item_id, "hash-1", 10, 20).unwrap();
        assert_eq!(no_overlap.len(), 0);

        // Same range but a different content_hash (a different document
        // version) must not be reported as an overlap.
        let different_version = annotations_overlapping(&conn, item_id, "hash-2", 0, 10).unwrap();
        assert_eq!(different_version.len(), 0);
    }

    #[test]
    fn annotations_overlapping_allows_highlight_and_memo_on_identical_range() {
        // The repository layer only reports intersections; the exact-range
        // highlight+memo exception is enforced by the command layer
        // deciding whether to treat a reported overlap as fatal (see
        // commands::annotations::create_annotation).
        let conn = init_in_memory().unwrap();
        let item_id = sample_item_id(&conn);

        create_annotation(
            &conn,
            item_id,
            "highlight",
            "hash-1",
            0,
            5,
            "hello",
            Some("yellow"),
            None,
            1,
        )
        .unwrap();

        let overlaps = annotations_overlapping(&conn, item_id, "hash-1", 0, 5).unwrap();
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].kind, "highlight");
    }

    #[test]
    fn annotation_kind_check_constraint_rejects_invalid_kind() {
        let conn = init_in_memory().unwrap();
        let item_id = sample_item_id(&conn);

        let result = conn.execute(
            "INSERT INTO annotations (item_id, kind, content_hash, start_offset, end_offset, quoted_text, created_at, updated_at)
             VALUES (?1, 'bogus', 'hash-1', 0, 5, 'hello', 1, 1)",
            params![item_id],
        );
        assert!(result.is_err());
    }
}
