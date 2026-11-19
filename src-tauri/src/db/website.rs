//! Non-destructive repair of HTML subscriptions. Only positively rejected URLs
//! are hidden; missing entries may simply have moved off the listing page.
use super::NewItem;
use super::url_identity;
use rusqlite::{Connection, Result, params};
use std::collections::{HashMap, HashSet};

pub struct WebsiteChanges {
    pub inserted: usize,
    pub changed: bool,
}

pub fn website_needs_repair(conn: &Connection, feed_id: i64, version: i64) -> Result<bool> {
    conn.query_row(
        "SELECT website_parser_version < ?2 FROM feeds WHERE id=?1",
        params![feed_id, version],
        |r| r.get(0),
    )
}

#[cfg(test)]
pub fn reconcile_website(
    conn: &Connection,
    feed_id: i64,
    items: &[NewItem],
    rejected: &[String],
    version: i64,
) -> Result<WebsiteChanges> {
    let tx = conn.unchecked_transaction()?;
    let changes = reconcile_website_in_transaction(&tx, feed_id, items, rejected, version)?;
    tx.commit()?;
    Ok(changes)
}

pub(crate) fn reconcile_website_in_transaction(
    tx: &Connection,
    feed_id: i64,
    items: &[NewItem],
    rejected: &[String],
    version: i64,
) -> Result<WebsiteChanges> {
    let excluded = super::collection::exclusions(tx, feed_id)?;
    let existing = {
        let mut stmt=tx.prepare("SELECT id,link,guid_hash,published_at,fetched_at,published_at_kind FROM items WHERE feed_id=?1 ORDER BY (is_saved=1
            OR EXISTS(SELECT 1 FROM annotations WHERE item_id=items.id)
            OR EXISTS(SELECT 1 FROM ai_sessions WHERE item_id=items.id)
            OR EXISTS(SELECT 1 FROM ai_session_items WHERE item_id=items.id)) DESC,id")?;
        stmt.query_map([feed_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>>>()?
    };
    let mut by_key = HashMap::new();
    for row in &existing {
        if let Some(key) = row.1.as_deref().and_then(url_identity::key) {
            by_key.entry(key).or_insert(row);
        }
    }
    let mut changes = WebsiteChanges {
        inserted: 0,
        changed: false,
    };
    let accepted: HashSet<_> = items
        .iter()
        .filter_map(|i| i.link.as_deref().and_then(url_identity::key))
        .collect();
    // Reverse insertion preserves source order for equal/unknown dates in DESC timelines.
    for item in items.iter().rev() {
        if super::collection::is_excluded(item, true, &excluded) {
            continue;
        }
        let key = item.link.as_deref().and_then(url_identity::key);
        if let Some(old) = key.as_ref().and_then(|k| by_key.get(k)) {
            let (published, kind) = if item.published_at_kind == "discovered" {
                if old.5 != "discovered" && old.3 != old.4 {
                    (old.3, old.5.as_str())
                } else {
                    (old.4, "discovered")
                }
            } else {
                (item.published_at, item.published_at_kind)
            };
            changes.changed |= tx.execute("UPDATE items SET title=?2,link=?3,published_at=?4,published_at_kind=?5,is_listing_noise=0,full_content=COALESCE(full_content,?6)
                WHERE id=?1 AND (title IS NOT ?2 OR link IS NOT ?3 OR published_at<>?4 OR published_at_kind<>?5 OR is_listing_noise<>0 OR (full_content IS NULL AND ?6 IS NOT NULL))",
                params![old.0,item.title,item.link,published,kind,item.full_content])?>0;
        } else {
            changes.inserted+=tx.execute("INSERT OR IGNORE INTO items(feed_id,guid_hash,title,link,summary,full_content,published_at,fetched_at,published_at_kind) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![feed_id,item.guid_hash,item.title,item.link,item.summary,item.full_content,item.published_at,item.fetched_at,item.published_at_kind])?;
        }
    }
    let rejected: HashSet<_> = rejected.iter().collect();
    for old in &existing {
        let Some(key) = old.1.as_deref().and_then(url_identity::key) else {
            continue;
        };
        let duplicate =
            accepted.contains(&key) && by_key.get(&key).is_some_and(|kept| kept.0 != old.0);
        let noise = !accepted.contains(&key) && rejected.contains(&key);
        if !old.2.starts_with("web:") || (!duplicate && !noise) {
            continue;
        }
        // Preserve saved/annotated/conversation-linked entries and all their IDs.
        changes.changed |= tx.execute(
            "UPDATE items SET is_listing_noise=1 WHERE id=?1 AND is_listing_noise=0 AND is_saved=0
            AND NOT EXISTS(SELECT 1 FROM annotations WHERE item_id=?1)
            AND NOT EXISTS(SELECT 1 FROM ai_sessions WHERE item_id=?1)
            AND NOT EXISTS(SELECT 1 FROM ai_session_items WHERE item_id=?1)",
            [old.0],
        )? > 0;
    }
    tx.execute(
        "UPDATE feeds SET website_parser_version=?2 WHERE id=?1",
        params![feed_id, version],
    )?;
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    fn item(url: &str, when: i64, kind: &'static str) -> NewItem {
        NewItem {
            guid_hash: format!("web:{url}"),
            title: Some(url.into()),
            link: Some(url.into()),
            summary: None,
            full_content: None,
            published_at: when,
            published_at_kind: kind,
            fetched_at: when,
        }
    }
    #[test]
    fn repairs_in_place_preserves_state_and_hides_only_confirmed_unprotected_noise() {
        let conn = db::init_in_memory().unwrap();
        let feed = db::insert_feed(
            &conn,
            "https://example.com/blog/",
            None,
            Some("website"),
            1000,
        )
        .unwrap();
        let old = vec![
            item("https://example.com/blog/old", 1000, "timestamp"),
            item("https://example.com/docs", 1000, "timestamp"),
            item("https://example.com/saved", 1000, "timestamp"),
            item("https://example.com/annotated", 1000, "timestamp"),
            item("https://example.com/disappeared", 900, "timestamp"),
        ];
        db::upsert_items(&conn, feed, &old).unwrap();
        let id = conn
            .query_row(
                "SELECT id FROM items WHERE link='https://example.com/blog/old'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap();
        conn.execute(
            "UPDATE items SET is_read=1,read_at=1234,is_saved=1 WHERE id=?1",
            [id],
        )
        .unwrap();
        conn.execute("INSERT INTO annotations(item_id,kind,content_hash,start_offset,end_offset,quoted_text,created_at,updated_at) VALUES(?1,'memo','hash',0,1,'a',1,1)",[id]).unwrap();
        conn.execute("INSERT INTO ai_sessions(item_id,provider,provider_session_id,title,created_at,updated_at) VALUES(?1,'codex','thread','Discussion',1,1)",[id]).unwrap();
        conn.execute("INSERT INTO ai_session_items(session_id,item_id,item_title,position,added_at) VALUES(1,?1,'Old article',0,1)",[id]).unwrap();
        conn.execute(
            "UPDATE items SET is_saved=1 WHERE link='https://example.com/saved'",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO annotations(item_id,kind,content_hash,start_offset,end_offset,quoted_text,created_at,updated_at) SELECT id,'memo','hash',0,1,'a',1,1 FROM items WHERE link='https://example.com/annotated'",[]).unwrap();
        let incoming = [
            item("https://example.com/blog/old/#section", 100, "date"),
            item("https://example.com/blog/new", 200, "date"),
        ];
        let rejected = ["docs", "saved", "annotated"].map(|s| format!("https://example.com/{s}"));
        let first = reconcile_website(&conn, feed, &incoming, &rejected, 1).unwrap();
        assert_eq!(first.inserted, 1);
        assert!(first.changed);
        let repaired = db::get_item_by_id(&conn, id).unwrap().unwrap();
        assert_eq!(repaired.published_at, 100);
        assert_eq!(repaired.published_at_kind, "date");
        assert_eq!(repaired.fetched_at, 1000);
        assert!(repaired.is_read && repaired.is_saved);
        assert_eq!(repaired.read_at, Some(1234));
        assert_eq!(
            conn.query_row(
                "SELECT item_id FROM annotations WHERE item_id=?1",
                [id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            id
        );
        assert_eq!(
            conn.query_row(
                "SELECT item_id FROM ai_session_items WHERE item_id=?1",
                [id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            id
        );
        assert_eq!(
            db::list_items(&conn, &db::ItemFilter::default(), 100, None)
                .unwrap()
                .len(),
            5
        );
        assert_eq!(db::count_unread(&conn, feed).unwrap(), 4);
        assert_eq!(
            db::count_unread_items(&conn, &db::ItemFilter::default()).unwrap(),
            4
        );
        let again = reconcile_website(&conn, feed, &incoming, &rejected, 1).unwrap();
        assert_eq!(again.inserted, 0);
        assert!(!again.changed);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            6
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
    }
    #[test]
    fn unknown_dates_keep_first_discovery_and_document_order() {
        let conn = db::init_in_memory().unwrap();
        let feed = db::insert_feed(&conn, "https://example.com", None, Some("website"), 0).unwrap();
        let mut items = vec![
            item("https://example.com/newer", 1000, "discovered"),
            item("https://example.com/older", 1000, "discovered"),
        ];
        reconcile_website(&conn, feed, &items, &[], 1).unwrap();
        for i in &mut items {
            i.published_at = 2000;
            i.fetched_at = 2000;
        }
        let change = reconcile_website(&conn, feed, &items, &[], 1).unwrap();
        assert!(!change.changed);
        assert_eq!(change.inserted, 0);
        let rows = db::list_items(&conn, &db::ItemFilter::default(), 10, None).unwrap();
        assert_eq!(rows[0].link, items[0].link);
        assert!(
            rows.iter()
                .all(|r| r.published_at == 1000 && r.published_at_kind == "discovered")
        );
    }
}

#[cfg(test)]
mod atomicity_tests {
    use super::*;
    use crate::db;
    #[test]
    fn a_failed_repair_rolls_back_rows_and_version_and_aliases_do_not_duplicate() {
        let conn = db::init_in_memory().unwrap();
        let id =
            db::insert_feed(&conn, "https://example.com/blog/", None, Some("website"), 0).unwrap();
        let item = NewItem {
            guid_hash: "web:one".into(),
            title: Some("First".into()),
            link: Some("https://example.com/post/".into()),
            summary: None,
            full_content: None,
            published_at: 1,
            published_at_kind: "date",
            fetched_at: 5,
        };
        conn.execute_batch("CREATE TRIGGER reject_repair BEFORE UPDATE OF website_parser_version ON feeds BEGIN SELECT RAISE(ABORT,'failure'); END;").unwrap();
        assert!(reconcile_website(&conn, id, std::slice::from_ref(&item), &[], 1).is_err());
        assert_eq!(db::count_unread(&conn, id).unwrap(), 0);
        assert!(website_needs_repair(&conn, id, 1).unwrap());
        conn.execute_batch("DROP TRIGGER reject_repair").unwrap();
        let alias = NewItem {
            guid_hash: "web:two".into(),
            link: Some("https://example.com/post#more".into()),
            ..item.clone()
        };
        db::upsert_items(&conn, id, &[item.clone(), alias]).unwrap();
        let repaired = reconcile_website(&conn, id, std::slice::from_ref(&item), &[], 1).unwrap();
        assert_eq!(repaired.inserted, 0);
        assert!(repaired.changed);
        assert_eq!(db::count_unread(&conn, id).unwrap(), 1);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
