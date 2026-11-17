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
    last_checked_at INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    folder_id       INTEGER REFERENCES feed_folders(id) ON DELETE SET NULL,
    hidden_from_all INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS feed_folders (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

-- Site icon per feed (docs/UI_SIMPLIFICATION_PLAN.md §3.12). A failed
-- lookup is stored as mime = '' with empty data so it isn't retried on
-- every sync, only after fetch::favicon::RETRY_FAILED_AFTER_SECS.
CREATE TABLE IF NOT EXISTS feed_icons (
    feed_id     INTEGER PRIMARY KEY REFERENCES feeds(id) ON DELETE CASCADE,
    mime        TEXT NOT NULL,
    data        BLOB NOT NULL,
    source_url  TEXT,
    fetched_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS items (
    id              INTEGER PRIMARY KEY,
    feed_id         INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    guid_hash       TEXT NOT NULL,
    title           TEXT,
    link            TEXT,
    summary         TEXT,
    full_content    TEXT,
    published_at    INTEGER NOT NULL,
    fetched_at      INTEGER NOT NULL,
    is_read         INTEGER NOT NULL DEFAULT 0,
    read_at         INTEGER,
    is_saved        INTEGER NOT NULL DEFAULT 0,
    UNIQUE(feed_id, guid_hash)
);

CREATE INDEX IF NOT EXISTS idx_items_published ON items(published_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_unread ON items(is_read, published_at DESC);

CREATE TABLE IF NOT EXISTS ai_sessions (
    id                      INTEGER PRIMARY KEY,
    item_id                 INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    provider                TEXT NOT NULL CHECK(provider IN ('codex', 'claude')),
    provider_session_id     TEXT UNIQUE,
    title                   TEXT NOT NULL,
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_sessions_item_updated
    ON ai_sessions(item_id, provider, updated_at DESC);

CREATE TABLE IF NOT EXISTS article_cache (
    item_id         INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    original_url    TEXT NOT NULL,
    final_url       TEXT,
    title           TEXT,
    content_html    TEXT,
    text_content    TEXT,
    etag            TEXT,
    last_modified   TEXT,
    fetched_at      INTEGER NOT NULL,
    error           TEXT
);

CREATE TABLE IF NOT EXISTS annotations (
    id              INTEGER PRIMARY KEY,
    item_id         INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK(kind IN ('highlight', 'memo')),
    content_hash    TEXT NOT NULL,
    start_offset    INTEGER NOT NULL,
    end_offset      INTEGER NOT NULL,
    quoted_text     TEXT NOT NULL,
    color           TEXT,
    note            TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_annotations_item ON annotations(item_id, start_offset);

CREATE TABLE IF NOT EXISTS retention_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    read_retention_days INTEGER NOT NULL DEFAULT 365,
    full_text_retention_days INTEGER NOT NULL DEFAULT 180
);
INSERT OR IGNORE INTO retention_settings (id, read_retention_days, full_text_retention_days)
VALUES (1, 365, 180);
"#;

pub fn apply(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    // Session bodies now remain solely in each provider CLI's transcript store.
    // Drop the old Aequo copy during upgrade so sensitive prompts/responses are removed.
    let has_ai_messages: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ai_messages')",
        [],
        |row| row.get(0),
    )?;
    if has_ai_messages {
        conn.pragma_update(None, "secure_delete", true)?;
        conn.execute_batch("DROP TABLE ai_messages; PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
    }
    let has_full_content = {
        let mut statement = conn.prepare("PRAGMA table_info(items)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        columns
            .collect::<Result<Vec<_>>>()?
            .iter()
            .any(|column| column == "full_content")
    };
    if !has_full_content {
        conn.execute_batch("ALTER TABLE items ADD COLUMN full_content TEXT")?;
    }
    for (name, definition) in [
        (
            "folder_id",
            "INTEGER REFERENCES feed_folders(id) ON DELETE SET NULL",
        ),
        ("hidden_from_all", "INTEGER NOT NULL DEFAULT 0"),
        ("last_checked_at", "INTEGER NOT NULL DEFAULT 0"),
        ("language", "TEXT"),
        ("custom_title", "TEXT"),
    ] {
        let exists = {
            let mut statement = conn.prepare("PRAGMA table_info(feeds)")?;
            let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
            columns
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(|column| column == name)
        };
        if !exists {
            conn.execute_batch(&format!("ALTER TABLE feeds ADD COLUMN {name} {definition}"))?;
        }
    }
    for (name, definition) in [
        ("read_at", "INTEGER"),
        ("is_saved", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let exists = {
            let mut statement = conn.prepare("PRAGMA table_info(items)")?;
            let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
            columns
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(|column| column == name)
        };
        if !exists {
            conn.execute_batch(&format!("ALTER TABLE items ADD COLUMN {name} {definition}"))?;
        }
    }
    Ok(())
}
