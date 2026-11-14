//! Repository functions: the only way the rest of the app touches the
//! `feeds`/`items` tables (see docs/ARCHITECTURE.md §3.1, §6).

use rusqlite::{Connection, OptionalExtension, Result, params};

use super::models::{Feed, Item, NewItem};

pub fn insert_feed(
    conn: &Connection,
    url: &str,
    title: Option<&str>,
    format: Option<&str>,
    created_at: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO feeds (url, title, format, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![url, title, format, created_at],
    )?;
    Ok(conn.last_insert_rowid())
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
        created_at: row.get(9)?,
    })
}

const FEED_COLUMNS: &str = "id, url, title, format, etag, last_modified, interval_secs, next_fetch_at, last_error, created_at";

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

/// Inserts new items for `feed_id`, skipping ones already present
/// (matched by the `UNIQUE(feed_id, guid_hash)` constraint). Returns the
/// number of rows actually inserted.
pub fn upsert_items(conn: &Connection, feed_id: i64, items: &[NewItem]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut inserted = 0usize;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO items
                (feed_id, guid_hash, title, link, summary, published_at, fetched_at, is_read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
        )?;
        for item in items {
            inserted += stmt.execute(params![
                feed_id,
                item.guid_hash,
                item.title,
                item.link,
                item.summary,
                item.published_at,
                item.fetched_at,
            ])?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: row.get(0)?,
        feed_id: row.get(1)?,
        guid_hash: row.get(2)?,
        title: row.get(3)?,
        link: row.get(4)?,
        summary: row.get(5)?,
        published_at: row.get(6)?,
        fetched_at: row.get(7)?,
        is_read: row.get::<_, i64>(8)? != 0,
    })
}

const ITEM_COLUMNS: &str =
    "id, feed_id, guid_hash, title, link, summary, published_at, fetched_at, is_read";

/// Lists items merged across all feeds, newest first (§6.3).
pub fn list_items(
    conn: &Connection,
    limit: i64,
    offset: i64,
    unread_only: bool,
) -> Result<Vec<Item>> {
    let sql = if unread_only {
        format!(
            "SELECT {ITEM_COLUMNS} FROM items WHERE is_read = 0 \
             ORDER BY published_at DESC LIMIT ?1 OFFSET ?2"
        )
    } else {
        format!("SELECT {ITEM_COLUMNS} FROM items ORDER BY published_at DESC LIMIT ?1 OFFSET ?2")
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit, offset], row_to_item)?;
    rows.collect()
}

pub fn mark_read(conn: &Connection, item_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE items SET is_read = 1 WHERE id = ?1",
        params![item_id],
    )?;
    Ok(())
}

pub fn mark_feed_read(conn: &Connection, feed_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE items SET is_read = 1 WHERE feed_id = ?1",
        params![feed_id],
    )?;
    Ok(())
}

pub fn count_unread(conn: &Connection, feed_id: i64) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM items WHERE feed_id = ?1 AND is_read = 0",
        params![feed_id],
        |row| row.get(0),
    )
}
