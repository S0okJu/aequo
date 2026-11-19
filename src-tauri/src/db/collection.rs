//! Initial selection and identities omitted at subscription. Later new articles have no cap.
use super::{NewItem, url_identity};
use rusqlite::{Connection, Result, params};
use std::collections::HashSet;

pub const INITIAL_ITEM_LIMIT: usize = 10;

pub struct InitialItems {
    pub items: Vec<NewItem>,
    excluded: Vec<String>,
}

fn identity(item: &NewItem, website: bool) -> String {
    if website && let Some(key) = item.link.as_deref().and_then(url_identity::key) {
        return key;
    }
    item.guid_hash.clone()
}

impl InitialItems {
    pub fn new(mut items: Vec<NewItem>, website: bool) -> Self {
        let mut seen = HashSet::new();
        items.retain(|item| seen.insert(identity(item, website)));
        if items
            .iter()
            .all(|item| item.published_at_kind != "discovered")
        {
            items.sort_by_key(|item| std::cmp::Reverse(item.published_at));
        }
        let excluded = items
            .drain(INITIAL_ITEM_LIMIT.min(items.len())..)
            .map(|item| identity(&item, website))
            .collect();
        Self { items, excluded }
    }

    /// Must share a transaction with the new feed and selected articles.
    pub fn save_state(&self, conn: &Connection, feed_id: i64) -> Result<()> {
        conn.execute(
            "INSERT INTO feed_collection_state(feed_id, policy_version) VALUES(?1, 1)",
            [feed_id],
        )?;
        let mut stmt =
            conn.prepare("INSERT INTO feed_initial_exclusions(feed_id, identity) VALUES(?1, ?2)")?;
        for key in &self.excluded {
            stmt.execute(params![feed_id, key])?;
        }
        Ok(())
    }
}

pub fn exclusions(conn: &Connection, feed_id: i64) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT identity FROM feed_initial_exclusions WHERE feed_id=?1")?;
    stmt.query_map([feed_id], |row| row.get(0))?.collect()
}

pub fn is_excluded(item: &NewItem, website: bool, excluded: &HashSet<String>) -> bool {
    excluded.contains(&identity(item, website))
}

/// Filter before optional enrichment as well as at the persistence boundary.
pub fn filter_items(conn: &Connection, feed_id: i64, items: &mut Vec<NewItem>) -> Result<()> {
    let website: bool = conn.query_row(
        "SELECT COALESCE(format='website',0) FROM feeds WHERE id=?1",
        [feed_id],
        |row| row.get(0),
    )?;
    let excluded = exclusions(conn, feed_id)?;
    items.retain(|item| !is_excluded(item, website, &excluded));
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::commands::add_feed::store_feed;
    use crate::db;

    pub(crate) fn items(count: usize) -> Vec<NewItem> {
        (0..count)
            .map(|i| NewItem {
                guid_hash: format!("id-{i}"),
                title: Some(format!("Article {i}")),
                link: Some(format!("https://example.com/post/{i}")),
                summary: Some("Summary".into()),
                full_content: None,
                published_at: i as i64,
                fetched_at: 1000,
                published_at_kind: "timestamp",
            })
            .collect()
    }

    #[test]
    fn initial_selection_deduplicates_sorts_and_falls_back_to_source_order() {
        for count in [0, 5, 10, 50] {
            let source = items(count);
            let duplicated: Vec<_> = source.iter().flat_map(|i| [i.clone(), i.clone()]).collect();
            let selected = InitialItems::new(duplicated, false);
            assert_eq!(selected.items.len(), count.min(10));
            assert_eq!(selected.excluded.len(), count.saturating_sub(10));
            if count > 0 {
                assert_eq!(selected.items[0].guid_hash, format!("id-{}", count - 1));
            }
        }
        let mut source = items(50);
        source[15].published_at_kind = "discovered";
        let selected = InitialItems::new(source, false);
        assert_eq!(selected.items[0].guid_hash, "id-0");
        assert_eq!(selected.items[9].guid_hash, "id-9");
        let mut tied = items(50);
        for i in &mut tied {
            i.published_at = 1;
        }
        assert_eq!(InitialItems::new(tied, false).items[0].guid_hash, "id-0");
    }

    #[test]
    fn initial_exclusions_survive_reopen_and_do_not_limit_later_backdated_articles() {
        let path = std::env::temp_dir().join(format!(
            "aequo-collection-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for format in ["rss", "atom", "json", "website"] {
            let conn = db::init(&path).unwrap();
            let source = items(50);
            let id = store_feed(
                &conn,
                &format!("https://example.com/{format}"),
                None,
                format,
                None,
                &source,
            )
            .unwrap();
            assert_eq!(db::count_unread(&conn, id).unwrap(), 10);
            assert_eq!(exclusions(&conn, id).unwrap().len(), 40);
            assert!(
                store_feed(
                    &conn,
                    &format!("https://example.com/{format}"),
                    None,
                    format,
                    None,
                    &source
                )
                .is_err()
            );
            drop(conn);
            let conn = db::init(&path).unwrap();
            let mut incoming = items(65);
            incoming[50].published_at = -100; // Newly discovered, despite an old date.
            incoming[51].published_at_kind = "discovered";
            if format == "website" {
                let changes =
                    db::website::reconcile_website(&conn, id, &incoming, &[], 99).unwrap();
                assert_eq!(changes.inserted, 15);
                assert_eq!(
                    db::website::reconcile_website(&conn, id, &incoming, &[], 99)
                        .unwrap()
                        .inserted,
                    0
                );
            } else {
                assert_eq!(db::upsert_items(&conn, id, &incoming).unwrap(), 15);
                assert_eq!(db::upsert_items(&conn, id, &incoming).unwrap(), 0);
            }
            assert_eq!(db::count_unread(&conn, id).unwrap(), 25);
            assert_eq!(exclusions(&conn, id).unwrap().len(), 40);
            filter_items(&conn, id, &mut incoming).unwrap();
            assert_eq!(incoming.len(), 25);
            db::delete_feed(&conn, id).unwrap();
            assert!(exclusions(&conn, id).unwrap().is_empty());
            let new_id = store_feed(
                &conn,
                &format!("https://example.com/{format}"),
                None,
                format,
                None,
                &items(65),
            )
            .unwrap();
            assert_eq!(db::count_unread(&conn, new_id).unwrap(), 10);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn registration_rolls_back_feed_items_and_exclusions_on_storage_failure() {
        for format in ["rss", "website"] {
            let conn = db::init_in_memory().unwrap();
            conn.execute_batch("CREATE TRIGGER reject_item BEFORE INSERT ON items BEGIN SELECT RAISE(ABORT,'disk failed'); END").unwrap();
            assert!(
                store_feed(
                    &conn,
                    "https://example.com/",
                    None,
                    format,
                    None,
                    &items(50)
                )
                .is_err()
            );
            for table in [
                "feeds",
                "items",
                "feed_collection_state",
                "feed_initial_exclusions",
            ] {
                let count: i64 = conn
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                    .unwrap();
                assert_eq!(count, 0, "{table}");
            }
            conn.execute_batch("DROP TRIGGER reject_item").unwrap();
            let id = store_feed(
                &conn,
                "https://example.com/",
                None,
                format,
                None,
                &items(50),
            )
            .unwrap();
            assert_eq!(db::count_unread(&conn, id).unwrap(), 10);
        }
    }

    #[test]
    fn website_aliases_share_initial_exclusions_and_legacy_feeds_remain_unlimited() {
        let conn = db::init_in_memory().unwrap();
        let source = items(50);
        let id = store_feed(
            &conn,
            "https://example.com/blog",
            None,
            "website",
            None,
            &source,
        )
        .unwrap();
        let mut aliases = source.clone();
        for i in &mut aliases {
            i.link = i.link.as_ref().map(|url| format!("{url}/#section"));
            i.guid_hash.push_str("-new");
        }
        assert_eq!(
            db::website::reconcile_website(&conn, id, &aliases, &[], 99)
                .unwrap()
                .inserted,
            0
        );
        assert_eq!(db::count_unread(&conn, id).unwrap(), 10);
        let legacy =
            db::insert_feed(&conn, "https://example.com/old", None, Some("rss"), 0).unwrap();
        db::schema::apply(&conn).unwrap();
        assert_eq!(db::upsert_items(&conn, legacy, &source).unwrap(), 50);
        assert!(exclusions(&conn, legacy).unwrap().is_empty());
    }
}
