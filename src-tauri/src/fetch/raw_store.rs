//! On-disk storage for each feed's last-fetched raw document - the
//! RSS/Atom/JSON body for a real feed, or the scraped page HTML for a
//! `format = 'website'` one. Kept as a plain file per feed rather than
//! a SQLite column: it's a disposable cache of the most recent fetch
//! (overwritten every successful poll, deleted with its feed), not
//! state the app ever queries or joins against.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedRawContent {
    pub content: String,
    pub fetched_at: i64,
}

/// Cheap to clone (just the directory `PathBuf`) - every poll task
/// gets its own clone, same as `ResourcePolicy`.
#[derive(Debug, Clone)]
pub struct RawContentStore {
    dir: PathBuf,
}

impl RawContentStore {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            dir: app_data_dir.join("feed-sources"),
        }
    }

    fn path(&self, feed_id: i64) -> PathBuf {
        self.dir.join(format!("{feed_id}.raw"))
    }

    /// Overwrites (or creates) the stored snapshot for `feed_id`.
    /// Lossily decoded to UTF-8 on write so storage and retrieval both
    /// deal in plain `String`; non-UTF-8 feeds are effectively
    /// nonexistent in the wild.
    pub fn write(&self, feed_id: i64, content: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        fs::write(
            self.path(feed_id),
            String::from_utf8_lossy(content).as_bytes(),
        )
    }

    /// `None` if nothing has been stored yet for this feed (never
    /// polled since this feature shipped, or the feed doesn't exist).
    pub fn read(&self, feed_id: i64) -> io::Result<Option<FeedRawContent>> {
        let path = self.path(feed_id);
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let fetched_at = fs::metadata(&path)?
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        Ok(Some(FeedRawContent {
            content,
            fetched_at,
        }))
    }

    pub fn has_content(&self, feed_id: i64) -> bool {
        self.path(feed_id).is_file()
    }

    /// Best-effort: a feed being deleted shouldn't fail because its
    /// cached source file can't be removed.
    pub fn delete(&self, feed_id: i64) {
        let _ = fs::remove_file(self.path(feed_id));
    }
}

#[cfg(test)]
pub fn temp_store() -> RawContentStore {
    let dir = std::env::temp_dir().join(format!(
        "aequo-raw-store-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    RawContentStore::new(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_is_absent_until_written() {
        let store = temp_store();
        assert_eq!(store.read(1).unwrap(), None);
        assert!(!store.has_content(1));

        store.write(1, b"<rss><channel/></rss>").unwrap();
        let raw = store.read(1).unwrap().unwrap();
        assert_eq!(raw.content, "<rss><channel/></rss>");
        assert!(raw.fetched_at > 0);
        assert!(store.has_content(1));

        // A second feed's file is independent.
        assert_eq!(store.read(2).unwrap(), None);

        store.write(1, b"<rss><channel>v2</channel></rss>").unwrap();
        assert_eq!(
            store.read(1).unwrap().unwrap().content,
            "<rss><channel>v2</channel></rss>"
        );

        store.delete(1);
        assert_eq!(store.read(1).unwrap(), None);
    }

    #[test]
    fn non_utf8_bytes_are_lossily_decoded_rather_than_failing() {
        let store = temp_store();
        store.write(1, &[0x3C, 0xFF, 0x3E]).unwrap();
        let raw = store.read(1).unwrap().unwrap();
        assert!(raw.content.contains('\u{FFFD}'));
    }
}
