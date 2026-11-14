//! SQLite schema for feeds and items (see docs/ARCHITECTURE.md §5).

use rusqlite::{Connection, Result};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS feeds (
    id              INTEGER PRIMARY KEY,
    url             TEXT NOT NULL UNIQUE,
    title           TEXT,
    format          TEXT,
    etag            TEXT,
    last_modified   TEXT,
    interval_secs   INTEGER NOT NULL DEFAULT 900,
    next_fetch_at   INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    created_at      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS items (
    id              INTEGER PRIMARY KEY,
    feed_id         INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    guid_hash       TEXT NOT NULL,
    title           TEXT,
    link            TEXT,
    summary         TEXT,
    published_at    INTEGER NOT NULL,
    fetched_at      INTEGER NOT NULL,
    is_read         INTEGER NOT NULL DEFAULT 0,
    UNIQUE(feed_id, guid_hash)
);
"#;

pub fn apply(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)
}
