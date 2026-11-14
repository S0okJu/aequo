//! Repository functions: the only way the rest of the app touches the
//! `feeds`/`items` tables (see docs/ARCHITECTURE.md §3.1, §6).

use rusqlite::{Connection, OptionalExtension, Result, params};

use super::models::{Feed, FeedMeta, Item, NewItem};

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

fn row_to_feed_meta(row: &rusqlite::Row) -> rusqlite::Result<FeedMeta> {
    Ok(FeedMeta {
        id: row.get(0)?,
        url: row.get(1)?,
        etag: row.get(2)?,
        last_modified: row.get(3)?,
        interval_secs: row.get(4)?,
        next_fetch_at: row.get(5)?,
    })
}

const FEED_META_COLUMNS: &str = "id, url, etag, last_modified, interval_secs, next_fetch_at";

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

/// Records a `304 Not Modified` response: only the next poll time moves.
pub fn update_feed_not_modified(conn: &Connection, feed_id: i64, next_fetch_at: i64) -> Result<()> {
    conn.execute(
        "UPDATE feeds SET next_fetch_at = ?1 WHERE id = ?2",
        params![next_fetch_at, feed_id],
    )?;
    Ok(())
}

/// Records a `200 OK` response: the new etag/last_modified plus the
/// next poll time.
pub fn update_feed_fetched(
    conn: &Connection,
    feed_id: i64,
    etag: Option<&str>,
    last_modified: Option<&str>,
    next_fetch_at: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE feeds SET etag = ?1, last_modified = ?2, next_fetch_at = ?3 WHERE id = ?4",
        params![etag, last_modified, next_fetch_at, feed_id],
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

        let items = list_items(&conn, 10, 0, false).unwrap();
        assert_eq!(items.len(), 1);
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

        let items = list_items(&conn, 10, 0, false).unwrap();
        mark_read(&conn, items[0].id).unwrap();
        assert_eq!(count_unread(&conn, feed_id).unwrap(), 1);

        mark_feed_read(&conn, feed_id).unwrap();
        assert_eq!(count_unread(&conn, feed_id).unwrap(), 0);
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

        let all_items = list_items(&conn, 10, 0, false).unwrap();
        mark_read(&conn, all_items[0].id).unwrap();

        let unread_items = list_items(&conn, 10, 0, true).unwrap();
        assert_eq!(unread_items.len(), 1);
        assert!(!unread_items[0].is_read);
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
}
