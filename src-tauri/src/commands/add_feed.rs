//! The `add_feed` Tauri command (see docs/ARCHITECTURE.md §6.1).

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use feed_rs::model::Feed;
use rusqlite::Connection;
use tauri::State;

use crate::db;
#[cfg(test)]
use crate::db::NewItem;
use crate::db::collection::InitialItems;
use crate::fetch;
use crate::resource_policy::{ResourcePolicy, WorkKind};

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

/// Test entry point for the same initial selection and atomic storage used by registration.
#[cfg(test)]
pub fn store_feed(
    conn: &Connection,
    url: &str,
    title: Option<&str>,
    format: &str,
    language: Option<&str>,
    items: &[NewItem],
) -> Result<i64, String> {
    store_initial_feed(
        conn,
        url,
        title,
        format,
        language,
        &InitialItems::new(items.to_vec(), format == "website"),
    )
}

fn store_initial_feed(
    conn: &Connection,
    url: &str,
    title: Option<&str>,
    format: &str,
    language: Option<&str>,
    initial: &InitialItems,
) -> Result<i64, String> {
    let store = || -> rusqlite::Result<i64> {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
        let feed_id = db::insert_feed(&tx, url, title, Some(format), now_unix())?;
        db::set_feed_language(&tx, feed_id, language)?;
        initial.save_state(&tx, feed_id)?;
        if format == "website" {
            db::reconcile_website_in_transaction(
                &tx,
                feed_id,
                &initial.items,
                &[],
                fetch::webpage::PARSER_VERSION,
            )?;
        } else {
            db::upsert_items_in_transaction(&tx, feed_id, &initial.items)?;
        }
        tx.commit()?;
        Ok(feed_id)
    };
    store().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_feed(
    db_state: State<'_, Mutex<Connection>>,
    icon_sync: State<'_, crate::icons::IconSync>,
    policy: State<'_, ResourcePolicy>,
    url: String,
) -> Result<i64, String> {
    let _permit = policy.acquire(WorkKind::Feed, false).await;
    let feed_id = subscribe(&db_state, url, &policy).await?;
    // Fetch the new feed's icon now rather than at the next periodic sync.
    icon_sync.0.notify_one();
    Ok(feed_id)
}

async fn subscribe(
    db_state: &Mutex<Connection>,
    url: String,
    policy: &ResourcePolicy,
) -> Result<i64, String> {
    let (url, bytes) = fetch::client::fetch_page(&url)
        .await
        .map_err(|e| fetch::client::describe_error(&e))?;

    match fetch::parser::parse(&bytes) {
        Ok(parsed) => {
            let now = now_unix();
            let format = fetch::parser::format_label(&parsed);
            let title = feed_title(&parsed);
            let mut initial =
                InitialItems::new(fetch::parser::to_new_items(&parsed, &bytes, now), false);
            // This feed row doesn't exist yet, so every item is new -
            // no DB read needed to know nothing already has content.
            fetch::enrichment::enrich_missing_full_content(
                &mut initial.items,
                &HashSet::new(),
                policy,
            )
            .await;

            let language = fetch::parser::feed_language(&parsed);
            let conn = lock_conn(db_state)?;
            store_initial_feed(
                &conn,
                &url,
                title.as_deref(),
                format,
                language.as_deref(),
                &initial,
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
                let mut initial = InitialItems::new(
                    fetch::parser::to_new_items(&parsed, &discovered.bytes, now),
                    false,
                );
                fetch::enrichment::enrich_missing_full_content(
                    &mut initial.items,
                    &HashSet::new(),
                    policy,
                )
                .await;

                let language = fetch::parser::feed_language(&parsed);
                let conn = lock_conn(db_state)?;
                return store_initial_feed(
                    &conn,
                    &discovered.url,
                    title.as_deref(),
                    format,
                    language.as_deref(),
                    &initial,
                );
            }
            let listing = fetch::webpage::parse(&url, &bytes, now).map_err(|page_error| {
                format!("Not a supported feed ({feed_error}) or article page ({page_error})")
            })?;
            let mut initial = InitialItems::new(listing.items, true);
            fetch::enrichment::enrich_missing_full_content(
                &mut initial.items,
                &HashSet::new(),
                policy,
            )
            .await;

            let conn = lock_conn(db_state)?;
            store_initial_feed(
                &conn,
                &url,
                listing.title.as_deref(),
                "website",
                None,
                &initial,
            )
        }
    }
}

fn lock_conn<'a>(
    db_state: &'a Mutex<Connection>,
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
        fetch::enrichment::enrich_missing_full_content(
            &mut items,
            &HashSet::new(),
            &ResourcePolicy::with_prefetch_for_tests(),
        )
        .await;

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

#[cfg(test)]
mod website_registration_tests {
    use super::*;
    #[test]
    fn registration_persists_article_only_listings_and_date_precision() {
        for (url, body, count) in [
            (
                "https://kyverno.io/blog/",
                include_bytes!("../../tests/fixtures/kyverno_blog.html").as_slice(),
                43,
            ),
            (
                "https://katacontainers.io/blog/",
                include_bytes!("../../tests/fixtures/kata_blog.html").as_slice(),
                68,
            ),
        ] {
            let listing = fetch::webpage::parse(url, body, 1_800_000_000).unwrap();
            let conn = db::init_in_memory().unwrap();
            let id = store_feed(
                &conn,
                url,
                listing.title.as_deref(),
                "website",
                None,
                &listing.items,
            )
            .unwrap();
            let rows = db::list_items(
                &conn,
                &db::ItemFilter {
                    feed_id: Some(id),
                    ..Default::default()
                },
                100,
                None,
            )
            .unwrap();
            assert_eq!(rows.len(), 10);
            assert_eq!(
                db::collection::exclusions(&conn, id).unwrap().len(),
                count - 10
            );
            assert!(rows.iter().all(|r| r.published_at_kind != "discovered"));
            let selected = InitialItems::new(listing.items.clone(), true);
            assert_eq!(rows[0].title, selected.items[0].title);
            assert!(!db::website_needs_repair(&conn, id, fetch::webpage::PARSER_VERSION).unwrap());
        }
    }
}

#[cfg(test)]
mod collection_flow_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn rss(base: &str, count: usize) -> String {
        let articles: String = (0..count).map(|i| format!("<item><guid>id-{i}</guid><title>Article {i}</title><link>{base}/article/{i}</link><description>Summary</description></item>")).collect();
        format!("<rss version=\"2.0\"><channel><title>Example</title>{articles}</channel></rss>")
    }

    #[tokio::test]
    async fn default_registration_paths_store_ten_without_requesting_articles() {
        let server = MockServer::start().await;
        let base = server.uri();
        Mock::given(method("GET"))
            .and(path("/source.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(rss(&base, 50)))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/discover"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!("<html><head><link rel=\"alternate\" type=\"application/rss+xml\" href=\"{base}/source.xml\"></head></html>")))
            .mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/blog/"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(
                include_bytes!("../../tests/fixtures/kyverno_blog.html").as_slice(),
            ))
            .mount(&server)
            .await;
        for route in ["/source.xml", "/discover", "/blog/"] {
            let conn = Mutex::new(db::init_in_memory().unwrap());
            let id = subscribe(&conn, format!("{base}{route}"), &ResourcePolicy::default())
                .await
                .unwrap();
            let c = conn.lock().unwrap();
            assert_eq!(db::count_unread(&c, id).unwrap(), 10);
            if route == "/blog/" {
                assert_eq!(
                    db::get_feed_meta(&c, id)
                        .unwrap()
                        .unwrap()
                        .format
                        .as_deref(),
                    Some("website")
                );
            }
            assert!(
                db::guid_hashes_with_full_content(&c, id)
                    .unwrap()
                    .is_empty()
            );
            assert!(db::collection::exclusions(&c, id).unwrap().len() >= 33);
        }
        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|r| {
            [
                "/source.xml",
                "/discover",
                "/blog/",
                "/feed",
                "/feed/",
                "/rss",
                "/rss.xml",
                "/atom.xml",
                "/index.xml",
            ]
            .contains(&r.url.path())
        }));
    }

    #[test]
    fn feed_formats_share_selection_and_preserve_embedded_content() {
        for format in ["rss", "atom", "json"] {
            let entries: Vec<_> = (0..50).map(|i| match format {
                "rss" => format!("<item><guid>id-{i}</guid><title>Article {i}</title><description>Summary</description></item>"),
                "atom" => format!("<entry><id>urn:article:{i}</id><title>Article {i}</title><content type=\"html\">Embedded body {i}</content></entry>"),
                _ => format!(r#"{{"id":"{i}","title":"Article {i}","content_html":"Embedded body {i}"}}"#),
            }).collect();
            let body = match format {
                "rss" => format!(
                    "<rss version=\"2.0\"><channel><title>Example</title>{}</channel></rss>",
                    entries.join("")
                ),
                "atom" => format!(
                    "<feed xmlns=\"http://www.w3.org/2005/Atom\"><id>urn:feed</id><title>Example</title>{}</feed>",
                    entries.join("")
                ),
                _ => format!(
                    r#"{{"version":"https://jsonfeed.org/version/1.1","title":"Example","items":[{}]}}"#,
                    entries.join(",")
                ),
            };
            let parsed = fetch::parser::parse(body.as_bytes()).unwrap();
            let source = fetch::parser::to_new_items(&parsed, body.as_bytes(), 1000);
            assert!(source.iter().all(|i| i.published_at_kind == "discovered"));
            let conn = db::init_in_memory().unwrap();
            let id =
                store_feed(&conn, "https://example.com/", None, format, None, &source).unwrap();
            assert_eq!(db::count_unread(&conn, id).unwrap(), 10);
            assert_eq!(db::collection::exclusions(&conn, id).unwrap().len(), 40);
            if format != "rss" {
                assert_eq!(
                    db::guid_hashes_with_full_content(&conn, id).unwrap().len(),
                    10
                );
            }
        }
    }
}

/// Run in isolated processes via scripts/perf/collection.py; excluded from normal tests.
#[cfg(test)]
mod collection_benchmark {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    #[ignore = "manual collection policy comparison"]
    async fn resource_collection_benchmark() {
        let variant = std::env::var("AEQUO_COLLECTION_BENCH_VARIANT").unwrap();
        let feeds: usize = std::env::var("AEQUO_COLLECTION_BENCH_FEEDS")
            .unwrap()
            .parse()
            .unwrap();
        assert!(["legacy", "limit", "prefetch_off", "both"].contains(&variant.as_str()));
        let limited = variant == "limit" || variant == "both";
        let prefetch = variant == "legacy" || variant == "limit";
        let policy = if prefetch {
            ResourcePolicy::with_prefetch_for_tests()
        } else {
            ResourcePolicy::default()
        };
        let server = MockServer::start().await;
        let article = format!(
            "<html><head><title>Article</title></head><body><article><p>{}</p></article></body></html>",
            "A readable article paragraph with concrete examples and detailed explanations. "
                .repeat(100)
        );
        Mock::given(method("GET"))
            .and(path("/article"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(article.clone(), "text/html")
                    .set_delay(std::time::Duration::from_millis(2)),
            )
            .mount(&server)
            .await;
        let entries: String = (0..50).map(|i| format!("<item><guid>id-{i}</guid><title>Article {i}</title><link>{}/article</link><description>Summary</description></item>",server.uri())).collect();
        let body = format!(
            "<rss version=\"2.0\"><channel><title>Example</title>{entries}</channel></rss>"
        );
        Mock::given(method("GET"))
            .and(path("/source.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body.clone()))
            .mount(&server)
            .await;
        let file = std::env::temp_dir().join(format!(
            "aequo-collection-benchmark-{}.db",
            std::process::id()
        ));
        let conn = db::init(&file).unwrap();
        let start = std::time::Instant::now();
        for index in 0..feeds {
            let (_, bytes) = fetch::client::fetch_page(&format!("{}/source.xml", server.uri()))
                .await
                .unwrap();
            let parsed = fetch::parser::parse(&bytes).unwrap();
            let mut items = fetch::parser::to_new_items(&parsed, &bytes, 1000);
            let url = format!("https://benchmark.invalid/feed/{index}");
            if limited {
                let mut initial = InitialItems::new(items, false);
                fetch::enrichment::enrich_missing_full_content(
                    &mut initial.items,
                    &HashSet::new(),
                    &policy,
                )
                .await;
                store_initial_feed(&conn, &url, None, "rss", None, &initial).unwrap();
            } else {
                fetch::enrichment::enrich_missing_full_content(
                    &mut items,
                    &HashSet::new(),
                    &policy,
                )
                .await;
                let id = db::insert_feed(&conn, &url, None, Some("rss"), 1000).unwrap();
                db::upsert_items(&conn, id, &items).unwrap();
            }
        }
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        let (rows, content_bytes): (usize, usize) = conn
            .query_row(
                "SELECT COUNT(*),COALESCE(SUM(length(CAST(full_content AS BLOB))),0) FROM items",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let exclusions: usize = conn
            .query_row("SELECT COUNT(*) FROM feed_initial_exclusions", [], |r| {
                r.get(0)
            })
            .unwrap();
        let identity_bytes: usize = conn.query_row("SELECT COALESCE(SUM(length(CAST(identity AS BLOB))),0) FROM feed_initial_exclusions",[],|r|r.get(0)).unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        let db_bytes = std::fs::metadata(&file).unwrap().len();
        let requests = server.received_requests().await.unwrap();
        let article_requests = requests
            .iter()
            .filter(|r| r.url.path() == "/article")
            .count();
        assert_eq!(rows, feeds * if limited { 10 } else { 50 });
        assert_eq!(article_requests, feeds * if prefetch { 8 } else { 0 });
        println!(
            "COLLECTION_BENCH {}",
            serde_json::json!({"variant":variant,"feeds":feeds,"elapsed_ms":elapsed,"rows":rows,"body_bytes":content_bytes,"exclusions":exclusions,"identity_bytes":identity_bytes,"db_bytes":db_bytes,"feed_requests":feeds,"article_requests":article_requests,"feed_response_bytes":feeds*body.len(),"article_response_bytes":article_requests*article.len()})
        );
        drop(conn);
        std::fs::remove_file(file).unwrap();
    }
}
