//! Connection setup: WAL mode, schema application, and the app-data-dir
//! DB file path. Decoupled from `tauri::AppHandle` so it stays testable
//! without a running Tauri app.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Result};

use super::{resources, schema, search};

const DB_FILE_NAME: &str = "aequo.db";

pub fn resolve_db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(DB_FILE_NAME)
}

pub fn init(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    // Before any schema work: the search-index triggers call strip_html.
    search::register_functions(&conn)?;
    schema::apply(&conn)?;
    resources::init(&conn)?;
    search::apply(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn init_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", true)?;
    search::register_functions(&conn)?;
    schema::apply(&conn)?;
    resources::init(&conn)?;
    search::apply(&conn)?;
    Ok(conn)
}
