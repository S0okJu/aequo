//! SQLite schema for feeds and items (see docs/ARCHITECTURE.md §5).
//!
//! Additive changes are idempotent. The first structural migration uses
//! `PRAGMA user_version = 2` to make article associations nullable and move
//! conversation context into an ordered relation. Unversioned v1 databases
//! and explicitly versioned v1 databases use the same transactional upgrade.

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

-- No collection state means a legacy feed; its history remains unrestricted.
CREATE TABLE IF NOT EXISTS feed_collection_state (
    feed_id INTEGER PRIMARY KEY REFERENCES feeds(id) ON DELETE CASCADE,
    policy_version INTEGER NOT NULL CHECK(policy_version = 1)
);
CREATE TABLE IF NOT EXISTS feed_initial_exclusions (
    feed_id INTEGER NOT NULL REFERENCES feed_collection_state(feed_id) ON DELETE CASCADE,
    identity TEXT NOT NULL,
    PRIMARY KEY(feed_id, identity)
) WITHOUT ROWID;

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
CREATE INDEX IF NOT EXISTS idx_items_timeline ON items(published_at DESC, id DESC);
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
        ("website_parser_version", "INTEGER NOT NULL DEFAULT 0"),
        ("sort_order", "INTEGER NOT NULL DEFAULT 0"),
        ("pinned", "INTEGER NOT NULL DEFAULT 0"),
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
            if name == "website_parser_version" {
                conn.execute("UPDATE feeds SET next_fetch_at=0 WHERE format='website' AND consecutive_failures=0", [])?;
            }
        }
    }
    for (name, definition) in [
        ("read_at", "INTEGER"),
        ("is_saved", "INTEGER NOT NULL DEFAULT 0"),
        ("published_at_kind", "TEXT NOT NULL DEFAULT 'timestamp'"),
        ("is_listing_noise", "INTEGER NOT NULL DEFAULT 0"),
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
    migrate_ai_context(conn)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ai_context_positions (
        session_id INTEGER PRIMARY KEY REFERENCES ai_sessions(id) ON DELETE CASCADE,
        next_position INTEGER NOT NULL
    );
    INSERT OR IGNORE INTO ai_context_positions(session_id,next_position)
        SELECT session_id,MAX(position)+1 FROM ai_session_items GROUP BY session_id;",
    )?;
    Ok(())
}

/// The first structural migration preserves provider transcript identifiers.
fn migrate_ai_context(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version >= 2 {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(r#"
        DROP INDEX IF EXISTS idx_ai_sessions_item_updated;
        ALTER TABLE ai_sessions RENAME TO ai_sessions_v1;
        CREATE TABLE ai_sessions (
            id INTEGER PRIMARY KEY,
            item_id INTEGER REFERENCES items(id) ON DELETE SET NULL,
            provider TEXT NOT NULL CHECK(provider IN ('codex', 'claude')),
            provider_session_id TEXT UNIQUE,
            title TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            scope_label TEXT
        );
        INSERT INTO ai_sessions (id,item_id,provider,provider_session_id,title,created_at,updated_at)
            SELECT id,item_id,provider,provider_session_id,title,created_at,updated_at FROM ai_sessions_v1;
        DROP TABLE ai_sessions_v1;
        CREATE INDEX idx_ai_sessions_item_updated ON ai_sessions(item_id,provider,updated_at DESC);
        CREATE TABLE ai_session_items (
            session_id INTEGER NOT NULL REFERENCES ai_sessions(id) ON DELETE CASCADE,
            item_id INTEGER REFERENCES items(id) ON DELETE SET NULL,
            item_title TEXT,
            position INTEGER NOT NULL,
            added_at INTEGER NOT NULL,
            sent_at INTEGER,
            PRIMARY KEY(session_id,position)
        );
        CREATE INDEX idx_ai_session_items_item ON ai_session_items(item_id);
        INSERT INTO ai_session_items(session_id,item_id,item_title,position,added_at,sent_at)
            SELECT s.id,s.item_id,i.title,0,s.created_at,s.updated_at
            FROM ai_sessions s JOIN items i ON i.id=s.item_id;
        PRAGMA user_version=2;
    "#)?;
    tx.commit()
}

#[cfg(test)]
mod context_migration_tests {
    use super::*;
    #[test]
    fn migrates_unversioned_and_v1_sessions_without_losing_provider_ids() {
        for version in [0, 1] {
            let conn = Connection::open_in_memory().unwrap();
            conn.pragma_update(None, "foreign_keys", true).unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
            conn.execute_batch("INSERT INTO feeds(id,url,created_at) VALUES(1,'https://example.com',0);
                INSERT INTO items(id,feed_id,guid_hash,title,published_at,fetched_at) VALUES(7,1,'a','Article A',0,0);
                INSERT INTO ai_sessions VALUES(5,7,'codex','thread-123','Discussion',10,20);").unwrap();
            apply(&conn).unwrap();
            apply(&conn).unwrap();
            let preserved:(String,i64,String)=conn.query_row("SELECT s.provider_session_id,c.item_id,c.item_title FROM ai_sessions s JOIN ai_session_items c ON c.session_id=s.id WHERE s.id=5",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            assert_eq!(preserved, ("thread-123".into(), 7, "Article A".into()));
            conn.execute("DELETE FROM items WHERE id=7", []).unwrap();
            let deleted:(Option<i64>,Option<i64>,String)=conn.query_row("SELECT s.item_id,c.item_id,c.item_title FROM ai_sessions s JOIN ai_session_items c ON c.session_id=s.id WHERE s.id=5",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            assert_eq!(deleted, (None, None, "Article A".into()));
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
                0
            );
        }
    }
}

#[cfg(test)]
mod website_migration_tests {
    use super::*;
    #[test]
    fn legacy_website_is_scheduled_for_repair_without_changing_items_or_backoff() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch("INSERT INTO feeds(id,url,format,next_fetch_at,created_at) VALUES(1,'https://example.com/blog/','website',999999,1);
            INSERT INTO feeds(id,url,format,next_fetch_at,created_at,consecutive_failures) VALUES(2,'https://example.com/failing/','website',888888,1,1);
            INSERT INTO items(id,feed_id,guid_hash,title,published_at,fetched_at,is_read,read_at,is_saved) VALUES(7,1,'web:old','Original',10,20,1,30,1);").unwrap();
        apply(&conn).unwrap();
        apply(&conn).unwrap();
        let values:(i64,i64,i64)=conn.query_row("SELECT next_fetch_at,website_parser_version,(SELECT next_fetch_at FROM feeds WHERE id=2) FROM feeds WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(values, (0, 0, 888888));
        let row:(String,i64,i64,bool,bool,String,bool)=conn.query_row("SELECT title,published_at,read_at,is_read,is_saved,published_at_kind,is_listing_noise FROM items WHERE id=7",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).unwrap();
        assert_eq!(
            row,
            (
                "Original".into(),
                10,
                30,
                true,
                true,
                "timestamp".into(),
                false
            )
        );
    }
}
