//! SQLite schema for feeds and items (see docs/ARCHITECTURE.md §5).
//!
//! Migration approach: plain `CREATE TABLE/INDEX IF NOT EXISTS`, applied
//! idempotently on every startup, rather than a versioned migration
//! framework. The v1 schema (§5) is fixed scope and not expected to
//! change before release, so there is no prior schema version to migrate
//! *from* yet - a migration runner would have nothing to do. If a
//! post-v1 change needs to alter existing columns/tables (not just add
//! new ones, which `IF NOT EXISTS` already handles), introduce a
//! versioned migration step then, rather than building that machinery
//! speculatively now.

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

CREATE INDEX IF NOT EXISTS idx_items_published ON items(published_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_unread ON items(is_read, published_at DESC);
"#;

pub fn apply(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)
}
