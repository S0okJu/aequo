//! The `add_feed` Tauri command (see docs/ARCHITECTURE.md §6.1).

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use feed_rs::model::Feed;
use rusqlite::Connection;
use tauri::State;

use crate::db::{self, NewItem};
use crate::fetch;

/// Fetches and parses a feed URL. Split out from the `#[tauri::command]`
/// wrapper so it can be exercised directly in tests (incl. against real
/// network URLs) without needing a running Tauri app / managed state.
///
/// Returns the raw bytes alongside the parsed `Feed` - `store_feed`
/// needs them too, to recover `content:encoded` bodies feed-rs's model
/// can drop (see `fetch::parser::to_new_items`).
#[cfg(test)]
pub async fn fetch_and_parse(url: &str) -> Result<(Vec<u8>, Feed), String> {
    let bytes = fetch::client::fetch_bytes(url)
        .await
        .map_err(|e| fetch::client::describe_error(&e))?;
    let parsed = fetch::parser::parse(&bytes).map_err(|e| e.to_string())?;
    Ok((bytes, parsed))
}

fn feed_title(parsed: &Feed) -> Option<String> {
    parsed.title.as_ref().map(|t| t.content.clone())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Inserts the `feeds` row, then bulk-inserts its (already-built, and
/// where applicable already full-content-enriched) items - new ones
/// only, see `db::upsert_items`. Returns the new feed's id.
///
/// Takes pre-built `items` rather than a raw `Feed`/bytes so callers can
/// enrich items (an async step - see `fetch::enrichment`) *before*
/// locking the connection: holding a `MutexGuard` across that `.await`
/// would make `add_feed`'s command future non-`Send`, which Tauri
/// requires.
pub fn store_feed(
    conn: &Connection,
    url: &str,
    title: Option<&str>,
    format: &str,
    language: Option<&str>,
    items: &[NewItem],
) -> Result<i64, String> {
    let now = now_unix();
    let feed_id =
        db::insert_feed(conn, url, title, Some(format), now).map_err(|e| e.to_string())?;
    db::set_feed_language(conn, feed_id, language).map_err(|e| e.to_string())?;
    db::upsert_items(conn, feed_id, items).map_err(|e| e.to_string())?;
    Ok(feed_id)
}

#[tauri::command]
pub async fn add_feed(
    db_state: State<'_, Mutex<Connection>>,
    icon_sync: State<'_, crate::icons::IconSync>,
    url: String,
) -> Result<i64, String> {
    let feed_id = subscribe(&db_state, url).await?;
    // Fetch the new feed's icon now rather than at the next periodic sync.
    icon_sync.0.notify_one();
    Ok(feed_id)
}

async fn subscribe(db_state: &State<'_, Mutex<Connection>>, url: String) -> Result<i64, String> {
    let bytes = fetch::client::fetch_bytes(&url)
        .await
        .map_err(|e| fetch::client::describe_error(&e))?;

    match fetch::parser::parse(&bytes) {
        Ok(parsed) => {
            let now = now_unix();
            let format = fetch::parser::format_label(&parsed);
            let title = feed_title(&parsed);
            let mut items = fetch::parser::to_new_items(&parsed, &bytes, now);
            // This feed row doesn't exist yet, so every item is new -
            // no DB read needed to know nothing already has content.
            fetch::enrichment::enrich_missing_full_content(&mut items, &HashSet::new()).await;

            let language = fetch::parser::feed_language(&parsed);
            let conn = lock_conn(db_state)?;
            store_feed(
                &conn,
                &url,
                title.as_deref(),
                format,
                language.as_deref(),
                &items,
            )
        }
        Err(feed_error) => {
            let now = now_unix();
            // Discovery and the webpage scraper fallback both need no DB
            // access, so the connection is locked only once we're ready to
            // write - holding it across an enrichment/discovery `.await`
            // would make this command's future non-`Send`.
            if let Some(discovered) = fetch::discovery::discover(&url, &bytes).await
                && let Ok(parsed) = fetch::parser::parse(&discovered.bytes)
            {
                let format = fetch::parser::format_label(&parsed);
                let title = feed_title(&parsed);
                let mut items = fetch::parser::to_new_items(&parsed, &discovered.bytes, now);
                fetch::enrichment::enrich_missing_full_content(&mut items, &HashSet::new()).await;

                let language = fetch::parser::feed_language(&parsed);
                let conn = lock_conn(db_state)?;
                return store_feed(
                    &conn,
                    &discovered.url,
                    title.as_deref(),
                    format,
                    language.as_deref(),
                    &items,
                );
            }
            let mut listing = fetch::webpage::parse(&url, &bytes, now).map_err(|page_error| {
                format!("Not a supported feed ({feed_error}) or article page ({page_error})")
            })?;
            fetch::enrichment::enrich_missing_full_content(&mut listing.items, &HashSet::new())
                .await;

            let conn = lock_conn(db_state)?;
            store_feed(
                &conn,
                &url,
                listing.title.as_deref(),
                "website",
                None,
                &listing.items,
            )
        }
    }
}

fn lock_conn<'a>(
    db_state: &'a State<'_, Mutex<Connection>>,
) -> Result<std::sync::MutexGuard<'a, Connection>, String> {
    db_state
        .lock()
        .map_err(|_| "database lock poisoned".to_string())
}

#[cfg(test)]
mod manual_network_tests {
    use super::*;

    /// Registers real blog feeds spanning all three supported formats
    /// (RSS 2.0, Atom, JSON Feed) through the exact add_feed logic
    /// (fetch_and_parse + store_feed), end to end against a real
    /// network. Network-dependent, so it's `#[ignore]`d by default -
    /// run manually with `cargo test -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn registers_real_blog_feeds() {
        let urls = [
            "https://overreacted.io/rss.xml",        // RSS 2.0
            "https://www.joelonsoftware.com/feed/",  // RSS 2.0
            "https://blog.rust-lang.org/feed.xml",   // Atom
            "https://jvns.ca/atom.xml",              // Atom
            "https://daringfireball.net/feeds/json", // JSON Feed
        ];

        let conn = crate::db::init_in_memory().unwrap();

        for url in urls {
            let (bytes, parsed) = fetch_and_parse(url)
                .await
                .unwrap_or_else(|e| panic!("fetch+parse failed for {url}: {e}"));
            let format = fetch::parser::format_label(&parsed);
            let title = feed_title(&parsed);
            let items = fetch::parser::to_new_items(&parsed, &bytes, now_unix());
            let feed_id = store_feed(&conn, url, title.as_deref(), format, None, &items)
                .unwrap_or_else(|e| panic!("store failed for {url}: {e}"));
            let item_count = crate::db::count_unread(&conn, feed_id).unwrap();

            println!("{url} -> format={format}, feed_id={feed_id}, items={item_count}");
            assert!(item_count > 0, "{url} stored zero items");
        }

        let total_feeds = crate::db::list_feeds(&conn).unwrap().len();
        assert_eq!(total_feeds, urls.len());
    }
}

#[cfg(test)]
mod enrichment_wiring_tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// Exercises the exact sequence `add_feed`'s success arm performs
    /// (parse -> build items -> enrich -> store), against a mock feed
    /// and a mock article page rather than the real network - confirms
    /// a freshly added feed's items come back with `full_content`
    /// pre-populated rather than needing a later on-demand fetch.
    #[tokio::test]
    async fn added_feed_items_get_full_content_pre_fetched() {
        let mock_server = MockServer::start().await;
        let article_html = r#"<html><head><title>Example</title></head><body>
<article><p>This is a long enough paragraph of real article text to clear the readability floor comfortably.</p></article>
</body></html>"#;
        Mock::given(method("GET"))
            .and(path("/post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(article_html, "text/html"))
            .mount(&mock_server)
            .await;

        let feed_xml = format!(
            r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Mock Feed</title>
<item><title>Post</title><link>{}/post</link><guid>guid-1</guid><description>Short summary.</description></item>
</channel></rss>"#,
            mock_server.uri()
        );

        let parsed = fetch::parser::parse(feed_xml.as_bytes()).unwrap();
        let format = fetch::parser::format_label(&parsed);
        let title = feed_title(&parsed);
        let mut items = fetch::parser::to_new_items(&parsed, feed_xml.as_bytes(), now_unix());
        fetch::enrichment::enrich_missing_full_content(&mut items, &HashSet::new()).await;

        let conn = crate::db::init_in_memory().unwrap();
        let feed_id = store_feed(
            &conn,
            "https://example.com/feed.xml",
            title.as_deref(),
            format,
            None,
            &items,
        )
        .unwrap();

        let stored_items = crate::db::list_items(
            &conn,
            &crate::db::ItemFilter {
                feed_id: Some(feed_id),
                ..Default::default()
            },
            10,
            None,
        )
        .unwrap();
        assert_eq!(stored_items.len(), 1);
        let stored = crate::db::get_item_by_id(&conn, stored_items[0].id)
            .unwrap()
            .unwrap();
        assert!(
            stored
                .full_content
                .as_deref()
                .is_some_and(|c| c.contains("long enough paragraph")),
            "expected pre-fetched full content, got {stored:?}"
        );
    }
}
