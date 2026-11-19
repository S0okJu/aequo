use rusqlite::{Connection, Result};

pub fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS update_settings (
      id INTEGER PRIMARY KEY CHECK(id=1), auto_update INTEGER NOT NULL DEFAULT 1,
      last_successful_check_at INTEGER, pending_installed_version TEXT, blocked_update TEXT);
      INSERT OR IGNORE INTO update_settings(id) VALUES(1);",
    )?;
    let exists = conn
        .prepare("PRAGMA table_info(update_settings)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>>>()?
        .iter()
        .any(|column| column == "blocked_update");
    if !exists {
        conn.execute_batch("ALTER TABLE update_settings ADD COLUMN blocked_update TEXT")?;
    }
    Ok(())
}
pub fn load(conn: &Connection) -> Result<(bool, Option<i64>, Option<String>)> {
    conn.query_row("SELECT auto_update,last_successful_check_at,pending_installed_version FROM update_settings WHERE id=1",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
}
pub fn auto_update(conn: &Connection, enabled: bool) -> Result<()> {
    conn.execute(
        "UPDATE update_settings SET auto_update=?1 WHERE id=1",
        [enabled],
    )?;
    Ok(())
}
pub fn checked(conn: &Connection, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE update_settings SET last_successful_check_at=?1 WHERE id=1",
        [now],
    )?;
    Ok(())
}
pub fn pending(conn: &Connection, version: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE update_settings SET pending_installed_version=?1 WHERE id=1",
        [version],
    )?;
    Ok(())
}

pub fn load_blocked(conn: &Connection) -> Result<Option<String>> {
    conn.query_row(
        "SELECT blocked_update FROM update_settings WHERE id=1",
        [],
        |row| row.get(0),
    )
}
pub fn blocked(conn: &Connection, value: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE update_settings SET blocked_update=?1 WHERE id=1",
        [value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_settings_gain_blocked_column_without_losing_preferences() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE update_settings(id INTEGER PRIMARY KEY, auto_update INTEGER NOT NULL, last_successful_check_at INTEGER,pending_installed_version TEXT);
          INSERT INTO update_settings VALUES(1,0,123,'1.0.2');").unwrap();
        init(&conn).unwrap();
        assert_eq!(
            load(&conn).unwrap(),
            (false, Some(123), Some("1.0.2".into()))
        );
        blocked(&conn, Some("failure")).unwrap();
        init(&conn).unwrap();
        assert_eq!(load_blocked(&conn).unwrap().as_deref(), Some("failure"));
    }
    #[test]
    fn settings_and_pending_install_survive_schema_reapplication() {
        let conn = Connection::open_in_memory().unwrap();
        init(&conn).unwrap();
        assert_eq!(load(&conn).unwrap(), (true, None, None));
        auto_update(&conn, false).unwrap();
        checked(&conn, 123).unwrap();
        pending(&conn, Some("1.0.2")).unwrap();
        init(&conn).unwrap();
        assert_eq!(
            load(&conn).unwrap(),
            (false, Some(123), Some("1.0.2".into()))
        );
        pending(&conn, None).unwrap();
        assert_eq!(load(&conn).unwrap().2, None);
    }
}
