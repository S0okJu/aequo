//! Versioned, atomically saved resource policy. Runtime consumers share this snapshot.
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceMode {
    Light,
    Balanced,
    Fast,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceSettings {
    pub schema_version: u32,
    pub mode: ResourceMode,
    pub feed_concurrency: u32,
    pub prefetch_full_text: bool,
    pub article_concurrency: u32,
    pub min_auto_refresh_secs: u32,
    pub pause_optional_when_hidden: bool,
    pub retained_pages: u32,
    pub cache_budget_mib: u32,
    pub ai_flush_interval_ms: u32,
}

impl Default for ResourceSettings {
    fn default() -> Self {
        Self::preset(ResourceMode::Balanced).expect("balanced preset")
    }
}

impl ResourceSettings {
    pub fn preset(mode: ResourceMode) -> Option<Self> {
        let (feeds, prefetch, articles, interval, pause, pages, cache, flush) = match mode {
            ResourceMode::Light => (1, false, 1, 1800, true, 2, 16, 100),
            ResourceMode::Balanced => (2, false, 2, 900, false, 3, 32, 50),
            ResourceMode::Fast => (4, false, 3, 300, false, 5, 64, 33),
            ResourceMode::Custom => return None,
        };
        Some(Self {
            schema_version: 1,
            mode,
            feed_concurrency: feeds,
            prefetch_full_text: prefetch,
            article_concurrency: articles,
            min_auto_refresh_secs: interval,
            pause_optional_when_hidden: pause,
            retained_pages: pages,
            cache_budget_mib: cache,
            ai_flush_interval_ms: flush,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported resource settings version".into());
        }
        if !(1..=4).contains(&self.feed_concurrency)
            || !(1..=3).contains(&self.article_concurrency)
            || ![300, 900, 1800, 3600].contains(&self.min_auto_refresh_secs)
            || !(2..=5).contains(&self.retained_pages)
            || ![16, 32, 64].contains(&self.cache_budget_mib)
            || !(33..=200).contains(&self.ai_flush_interval_ms)
        {
            return Err("Resource settings are outside supported limits".into());
        }
        if let Some(preset) = Self::preset(self.mode)
            && self != &preset
        {
            return Err("Modified preset values must use custom mode".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceSnapshot {
    pub revision: i64,
    pub settings: ResourceSettings,
}

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS resource_settings (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            revision INTEGER NOT NULL CHECK(revision >= 0),
            settings_json TEXT NOT NULL
        )",
    )?;
    let json =
        serde_json::to_string(&ResourceSettings::default()).expect("serialize default policy");
    conn.execute(
        "INSERT OR IGNORE INTO resource_settings(id,revision,settings_json) VALUES(1,0,?1)",
        [json],
    )?;
    migrate_legacy_presets(conn)?;
    Ok(())
}

/// Preserve every execution value from known old presets before strict validation.
/// Unknown/corrupt settings still require the existing explicit recovery path.
fn migrate_legacy_presets(conn: &Connection) -> rusqlite::Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let json: String = tx.query_row(
        "SELECT settings_json FROM resource_settings WHERE id=1",
        [],
        |r| r.get(0),
    )?;
    if let Ok(mut settings) = serde_json::from_str::<ResourceSettings>(&json) {
        for mode in [ResourceMode::Balanced, ResourceMode::Fast] {
            let mut old = ResourceSettings::preset(mode).expect("known preset");
            old.prefetch_full_text = true;
            if settings == old {
                settings.mode = ResourceMode::Custom;
                tx.execute(
                    "UPDATE resource_settings SET settings_json=?1, revision=revision+1 WHERE id=1",
                    [serde_json::to_string(&settings).expect("serialize settings")],
                )?;
                break;
            }
        }
    }
    tx.commit()
}

pub fn get(conn: &Connection) -> Result<ResourceSnapshot, String> {
    let (revision, json): (i64, String) = conn
        .query_row(
            "SELECT revision,settings_json FROM resource_settings WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let settings: ResourceSettings = serde_json::from_str(&json)
        .map_err(|_| "Resource settings are damaged; restore defaults to recover".to_string())?;
    settings.validate()?;
    Ok(ResourceSnapshot { revision, settings })
}

/// Compare-and-swap prevents stale UI saves overwriting newer settings.
/// This also recovers damaged JSON when explicitly saving validated defaults.
pub fn save(
    conn: &Connection,
    expected_revision: i64,
    settings: ResourceSettings,
) -> Result<ResourceSnapshot, String> {
    settings.validate()?;
    let revision = expected_revision
        .checked_add(1)
        .filter(|_| expected_revision >= 0)
        .ok_or_else(|| "Invalid resource settings revision".to_string())?;
    let json = serde_json::to_string(&settings).map_err(|error| error.to_string())?;
    let changed = conn
        .execute(
            "UPDATE resource_settings SET settings_json=?1,revision=?2 WHERE id=1 AND revision=?3",
            params![json, revision, expected_revision],
        )
        .map_err(|error| error.to_string())?;
    if changed != 1 {
        return Err("Resource settings changed; reload before saving again".into());
    }
    Ok(ResourceSnapshot { revision, settings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_presets_match_backend_contract() {
        let presets: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/lib/resourcePresets.json")).unwrap();
        for mode in [
            ResourceMode::Light,
            ResourceMode::Balanced,
            ResourceMode::Fast,
        ] {
            let settings = ResourceSettings::preset(mode).unwrap();
            let value = serde_json::to_value(&settings).unwrap();
            assert_eq!(presets[value["mode"].as_str().unwrap()], value);
        }
    }

    #[test]
    fn presets_and_all_mutable_boundaries_are_validated() {
        for mode in [
            ResourceMode::Light,
            ResourceMode::Balanced,
            ResourceMode::Fast,
        ] {
            ResourceSettings::preset(mode).unwrap().validate().unwrap();
        }
        let custom = ResourceSettings {
            mode: ResourceMode::Custom,
            ..Default::default()
        };
        let mut invalid = custom.clone();
        invalid.feed_concurrency = 0;
        assert!(invalid.validate().is_err());
        invalid = custom.clone();
        invalid.feed_concurrency = 5;
        assert!(invalid.validate().is_err());
        invalid = custom.clone();
        invalid.article_concurrency = 4;
        assert!(invalid.validate().is_err());
        invalid = custom.clone();
        invalid.min_auto_refresh_secs = 1;
        assert!(invalid.validate().is_err());
        invalid = custom.clone();
        invalid.retained_pages = 6;
        assert!(invalid.validate().is_err());
        invalid = custom.clone();
        invalid.cache_budget_mib = 0;
        assert!(invalid.validate().is_err());
        invalid = custom.clone();
        invalid.ai_flush_interval_ms = 201;
        assert!(invalid.validate().is_err());
        invalid = custom;
        invalid.schema_version = 2;
        assert!(invalid.validate().is_err());
        invalid = ResourceSettings::default();
        invalid.feed_concurrency = 1;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn upgrades_existing_db_without_touching_user_content_and_rejects_stale_saves() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE user_content(value TEXT); INSERT INTO user_content VALUES('keep')",
        )
        .unwrap();
        init(&conn).unwrap();
        let initial = get(&conn).unwrap();
        assert_eq!(initial.revision, 0);
        let light = ResourceSettings::preset(ResourceMode::Light).unwrap();
        let saved = save(&conn, initial.revision, light.clone()).unwrap();
        init(&conn).unwrap(); // repeated startup must not reset user policy
        assert_eq!(get(&conn).unwrap(), saved);
        assert!(save(&conn, 0, ResourceSettings::default()).is_err());
        assert_eq!(get(&conn).unwrap().settings, light);
        let content: String = conn
            .query_row("SELECT value FROM user_content", [], |r| r.get(0))
            .unwrap();
        assert_eq!(content, "keep");
    }

    #[test]
    fn corrupted_or_future_settings_require_explicit_recovery() {
        let conn = Connection::open_in_memory().unwrap();
        init(&conn).unwrap();
        conn.execute("UPDATE resource_settings SET settings_json='not-json'", [])
            .unwrap();
        assert!(get(&conn).unwrap_err().contains("damaged"));
        save(&conn, 0, ResourceSettings::default()).unwrap();
        assert_eq!(get(&conn).unwrap().revision, 1);
        let future = ResourceSettings {
            schema_version: 2,
            ..ResourceSettings::default()
        };
        assert!(save(&conn, 1, future).is_err());
        assert_eq!(get(&conn).unwrap().revision, 1);
    }
}

#[cfg(test)]
mod collection_migration_tests {
    use super::*;

    #[test]
    fn old_presets_preserve_values_and_migrate_once_before_validation() {
        for mode in [
            ResourceMode::Light,
            ResourceMode::Balanced,
            ResourceMode::Fast,
            ResourceMode::Custom,
        ] {
            let conn = Connection::open_in_memory().unwrap();
            init(&conn).unwrap();
            let mut old = ResourceSettings::preset(mode).unwrap_or(ResourceSettings {
                mode,
                ..Default::default()
            });
            if mode != ResourceMode::Light {
                old.prefetch_full_text = true;
            }
            let json = serde_json::to_string(&old).unwrap();
            conn.execute(
                "UPDATE resource_settings SET revision=7, settings_json=?1",
                [json],
            )
            .unwrap();
            init(&conn).unwrap();
            let snapshot = get(&conn).unwrap();
            let changed = matches!(mode, ResourceMode::Balanced | ResourceMode::Fast);
            let mut expected = old.clone();
            if changed {
                expected.mode = ResourceMode::Custom;
            }
            assert_eq!(snapshot.settings, expected);
            assert_eq!(snapshot.revision, if changed { 8 } else { 7 });
            init(&conn).unwrap();
            assert_eq!(get(&conn).unwrap(), snapshot);
            if changed {
                assert!(save(&conn, 7, ResourceSettings::default()).is_err());
            }
        }
    }

    #[test]
    fn migration_is_atomic_and_does_not_repair_unknown_or_damaged_settings() {
        let conn = Connection::open_in_memory().unwrap();
        init(&conn).unwrap();
        let mut old = ResourceSettings::default();
        old.prefetch_full_text = true;
        let json = serde_json::to_string(&old).unwrap();
        conn.execute("UPDATE resource_settings SET settings_json=?1", [&json])
            .unwrap();
        conn.execute_batch("CREATE TRIGGER reject_migration BEFORE UPDATE ON resource_settings BEGIN SELECT RAISE(ABORT,'failure'); END").unwrap();
        assert!(init(&conn).is_err());
        let persisted: (String, i64) = conn
            .query_row(
                "SELECT settings_json,revision FROM resource_settings",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted, (json, 0));
        conn.execute_batch("DROP TRIGGER reject_migration").unwrap();
        old.feed_concurrency = 4; // Not a known old balanced preset.
        conn.execute(
            "UPDATE resource_settings SET settings_json=?1",
            [serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
        init(&conn).unwrap();
        assert!(get(&conn).is_err());
        conn.execute("UPDATE resource_settings SET settings_json='broken'", [])
            .unwrap();
        init(&conn).unwrap();
        assert!(get(&conn).is_err());
        save(&conn, 0, ResourceSettings::default()).unwrap();
        assert!(!get(&conn).unwrap().settings.prefetch_full_text);
    }
}
