//! Poll scheduler: finds due feeds, dispatches fetch workers under
//! bounded concurrency, and sleeps fully between cycles (see
//! docs/ARCHITECTURE.md §3.1, §4.2, §4.3).
//!
//! Deliberately uses its own `Arc<Mutex<Connection>>`, separate from
//! the one Tauri manages for commands - ARCHITECTURE.md §4.3 opens
//! the DB in WAL mode specifically so UI reads and background-poll
//! writes don't block each other, which only holds if they go
//! through different connections.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tauri::{AppHandle, Manager, Runtime};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until};

use crate::db;
use crate::events::emit_if_updated;
use crate::fetch::worker::{self, PollOutcome};
use crate::resource_policy::{ResourcePolicy, effective_due_at};

/// Max simultaneous fetches (§4.2) - caps connection/CPU spikes as
/// the feed count grows.
const MAX_CONCURRENT_FETCHES: usize = 4;

/// How long to sleep when no feed is registered yet (there's no real
/// next_fetch_at to compute a wake time from).
const IDLE_FALLBACK: Duration = Duration::from_secs(60);

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Polls every feed whose `next_fetch_at` has passed, bounding
/// concurrent fetches with a semaphore and collecting results via a
/// `JoinSet` (§4.2). A single feed's failure (network error, bad
/// parse, panic) is logged and doesn't stop the others. Emits
/// `feed-updated` (§6.2) for feeds that got new items. Returns the
/// number of feeds attempted, for callers that want to observe cycle
/// size.
pub async fn poll_due_feeds<R: Runtime>(conn: Arc<Mutex<Connection>>, app: AppHandle<R>) -> usize {
    let policy = app.state::<ResourcePolicy>().inner().clone();
    let due = {
        let guard = conn.lock().expect("database lock poisoned");
        db::list_due_feed_metas(&guard, now_unix())
            .unwrap_or_default()
            .into_iter()
            .filter(|meta| {
                effective_due_at(meta, policy.settings().min_auto_refresh_secs) <= now_unix()
            })
            .collect::<Vec<_>>()
    };
    let attempted = due.len();

    let mut set = JoinSet::new();

    for meta in due {
        // Bound waiting tasks too: do not spawn one future per subscription.
        while set.len() >= MAX_CONCURRENT_FETCHES {
            if let Some(joined) = set.join_next().await {
                handle_result(&app, joined);
            }
        }
        let conn = conn.clone();
        let policy = policy.clone();
        set.spawn(async move {
            let feed_id = meta.id;
            let url = meta.url.clone();
            (
                feed_id,
                url,
                worker::poll_feed_locked(&conn, &meta, &policy, false).await,
            )
        });
    }

    while let Some(joined) = set.join_next().await {
        handle_result(&app, joined);
    }

    attempted
}

type PollResult = Result<(i64, String, Result<PollOutcome, String>), tokio::task::JoinError>;

fn handle_result<R: Runtime>(app: &AppHandle<R>, joined: PollResult) {
    match joined {
        Ok((feed_id, _, Ok(outcome))) => {
            // Recorded to last_error/consecutive_failures already
            // (§7) - still logged here so failures are visible
            // without needing to query the DB.
            if let PollOutcome::Failed { error } = &outcome {
                eprintln!("poll failed for feed {feed_id}: {error}");
            }
            emit_if_updated(app, feed_id, &outcome);
        }
        // A hard Err means the DB write itself failed (lock
        // poisoned, disk error, etc.) - an infra problem, not a
        // feed-specific one, so it couldn't be recorded to
        // last_error.
        Ok((_, url, Err(err))) => eprintln!("poll failed for {url}: {err}"),
        Err(join_err) => eprintln!("fetch task panicked: {join_err}"),
    }
}

/// Earliest `next_fetch_at` across all feeds, as a monotonic
/// [`Instant`] the scheduler should sleep until (§4.3). Falls back to
/// [`IDLE_FALLBACK`] when there are no feeds yet.
fn next_wake(conn: &Connection, minimum_secs: u32) -> Instant {
    let earliest = db::list_feed_metas(conn)
        .unwrap_or_default()
        .into_iter()
        .map(|m| effective_due_at(&m, minimum_secs))
        .min();

    match earliest {
        Some(next_fetch_at) => {
            let delay_secs = (next_fetch_at - now_unix()).max(0) as u64;
            Instant::now() + Duration::from_secs(delay_secs)
        }
        None => Instant::now() + IDLE_FALLBACK,
    }
}

/// Runs forever: sleeps until the next feed is due, polls all due
/// feeds, repeats. Call once on app launch as a background task
/// (§3.1) - never busy-loops or wakes on a fixed short interval
/// (§4.3): between cycles the task is fully parked by `sleep_until`.
pub async fn run<R: Runtime>(
    conn: Arc<Mutex<Connection>>,
    app: AppHandle<R>,
    schedule_changed: Arc<tokio::sync::Notify>,
) {
    let policy = app.state::<ResourcePolicy>().inner().clone();
    let mut policy_changes = policy.subscribe();
    loop {
        let wake_at = {
            let guard = conn.lock().expect("database lock poisoned");
            next_wake(&guard, policy.settings().min_auto_refresh_secs)
        };
        tokio::select! {
            _ = sleep_until(wake_at) => {},
            _ = schedule_changed.notified() => continue,
            _ = policy_changes.changed() => continue,
        }
        poll_due_feeds(conn.clone(), app.clone()).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant as StdInstant;

    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::db::init_in_memory;

    const RSS2_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sample_rss2.xml"
    ));

    fn shared_conn(conn: Connection) -> Arc<Mutex<Connection>> {
        Arc::new(Mutex::new(conn))
    }

    /// A real `AppHandle` from Tauri's mock runtime (the `test`
    /// feature) - `emit_if_updated` needs one to call, but these
    /// tests don't care whether anything is actually listening.
    fn mock_app_handle() -> AppHandle<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        let policy = ResourcePolicy::default();
        policy.apply(crate::db::resources::ResourceSnapshot {
            revision: 1,
            settings: crate::db::resources::ResourceSettings::preset(
                crate::db::resources::ResourceMode::Fast,
            )
            .unwrap(),
        });
        app.manage(policy);
        app.handle().clone()
    }

    /// Indirect but reliable way to verify the semaphore actually
    /// bounds concurrency: 6 feeds each taking ~120ms would finish in
    /// ~120ms if unbounded, or ~720ms if fully sequential. Bounded at
    /// 4 concurrent, 6 feeds run in two batches (4 then 2) and must
    /// take at least ~2x120ms.
    #[tokio::test]
    async fn poll_due_feeds_bounds_concurrency() {
        let mock_server = MockServer::start().await;
        let delay = Duration::from_millis(120);
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(RSS2_FIXTURE)
                    .set_delay(delay),
            )
            .mount(&mock_server)
            .await;

        let conn = init_in_memory().unwrap();
        for i in 0..6 {
            db::insert_feed(
                &conn,
                &format!("{}/feed-{i}.xml", mock_server.uri()),
                None,
                None,
                0,
            )
            .unwrap();
        }
        let conn = shared_conn(conn);

        let start = StdInstant::now();
        let attempted = poll_due_feeds(conn, mock_app_handle()).await;
        let elapsed = start.elapsed();

        assert_eq!(attempted, 6);
        // Comfortably below "6 fully sequential" (~720ms) and above
        // "fully parallel" (~120ms) - consistent with 2 batches of 4+2.
        assert!(
            elapsed >= Duration::from_millis(200),
            "expected >= 2 batches worth of delay, got {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(600),
            "expected well under fully-sequential time, got {elapsed:?}"
        );
    }

    /// Reproducible release-path measurement for M10: 20 due feeds,
    /// 5,000 existing SQLite items, and unchanged feeds returning 304.
    /// Run explicitly with `cargo test -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "manual M10 performance verification"]
    async fn performance_20_feeds_5000_items_304_cycle() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("If-None-Match", "\"m10-etag\""))
            .respond_with(ResponseTemplate::new(304))
            .expect(20)
            .mount(&mock_server)
            .await;

        let conn = init_in_memory().unwrap();
        for feed_index in 0..20 {
            let feed_id = db::insert_feed(
                &conn,
                &format!("{}/feed-{feed_index}.xml", mock_server.uri()),
                None,
                None,
                0,
            )
            .unwrap();
            db::update_feed_fetched(&conn, feed_id, Some("\"m10-etag\""), None, 0).unwrap();

            let items = (0..250)
                .map(|item_index| crate::db::NewItem {
                    published_at_kind: "timestamp",
                    guid_hash: format!("{feed_index}-{item_index}"),
                    title: Some(format!("Article {item_index}")),
                    link: Some(format!("https://feed-{feed_index}.example/{item_index}")),
                    summary: Some("M10 fixture article summary".to_string()),
                    full_content: None,
                    published_at: 1_700_000_000 + item_index,
                    fetched_at: 1_700_000_000 + item_index,
                })
                .collect::<Vec<_>>();
            assert_eq!(db::upsert_items(&conn, feed_id, &items).unwrap(), 250);
        }

        let item_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(item_count, 5_000);

        let conn = shared_conn(conn);
        let started = StdInstant::now();
        let attempted = poll_due_feeds(conn, mock_app_handle()).await;
        let elapsed = started.elapsed();

        assert_eq!(attempted, 20);
        assert!(
            elapsed < Duration::from_millis(500),
            "20-feed 304 cycle exceeded 500ms: {elapsed:?}"
        );
        eprintln!(
            "M10 measurement: {attempted} feeds, {item_count} existing items, 304-only cycle in {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn poll_due_feeds_continues_past_one_failure() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/good.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RSS2_FIXTURE))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/bad.xml"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let conn = init_in_memory().unwrap();
        let good_id = db::insert_feed(
            &conn,
            &format!("{}/good.xml", mock_server.uri()),
            None,
            None,
            0,
        )
        .unwrap();
        let bad_id = db::insert_feed(
            &conn,
            &format!("{}/bad.xml", mock_server.uri()),
            None,
            None,
            0,
        )
        .unwrap();
        let conn = shared_conn(conn);

        let attempted = poll_due_feeds(conn.clone(), mock_app_handle()).await;
        assert_eq!(attempted, 2);

        let guard = conn.lock().unwrap();
        let good_meta = db::get_feed_meta(&guard, good_id).unwrap().unwrap();
        assert!(good_meta.next_fetch_at > 0); // succeeded, rescheduled normally
        assert_eq!(good_meta.consecutive_failures, 0);

        // Failed, but still recorded (§7) rather than left untouched -
        // rescheduled via backoff, not stuck at next_fetch_at = 0.
        let bad_meta = db::get_feed_meta(&guard, bad_id).unwrap().unwrap();
        assert!(bad_meta.next_fetch_at > 0);
        assert_eq!(bad_meta.consecutive_failures, 1);
        let bad_feed = db::get_feed_by_id(&guard, bad_id).unwrap().unwrap();
        assert!(bad_feed.last_error.is_some());
    }

    /// Same claim as `poll_due_feeds_continues_past_one_failure`, but
    /// for a real *timeout* specifically (item 3 names both "network
    /// errors/timeouts" separately) through the actual production
    /// codepath (client::fetch_conditional's real 10s REQUEST_TIMEOUT,
    /// not an injected short one). #[ignore]'d since it must wait out
    /// that real 10s - run manually with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn poll_due_feeds_timeout_does_not_block_other_feeds() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/good.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RSS2_FIXTURE))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/slow.xml"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(11)))
            .mount(&mock_server)
            .await;

        let conn = init_in_memory().unwrap();
        let good_id = db::insert_feed(
            &conn,
            &format!("{}/good.xml", mock_server.uri()),
            None,
            None,
            0,
        )
        .unwrap();
        let slow_id = db::insert_feed(
            &conn,
            &format!("{}/slow.xml", mock_server.uri()),
            None,
            None,
            0,
        )
        .unwrap();
        let conn = shared_conn(conn);

        let attempted = poll_due_feeds(conn.clone(), mock_app_handle()).await;
        assert_eq!(attempted, 2);

        let guard = conn.lock().unwrap();
        let good_meta = db::get_feed_meta(&guard, good_id).unwrap().unwrap();
        assert!(good_meta.next_fetch_at > 0);
        assert_eq!(good_meta.consecutive_failures, 0);

        let slow_meta = db::get_feed_meta(&guard, slow_id).unwrap().unwrap();
        assert_eq!(slow_meta.consecutive_failures, 1);
        let slow_feed = db::get_feed_by_id(&guard, slow_id).unwrap().unwrap();
        let error = slow_feed
            .last_error
            .expect("expected a recorded timeout error");
        assert!(
            error.contains("timed out") || error.contains("timeout"),
            "{error}"
        );
    }

    #[test]
    fn next_wake_uses_earliest_next_fetch_at() {
        let conn = init_in_memory().unwrap();
        let soon_id = db::insert_feed(&conn, "https://example.com/a.xml", None, None, 0).unwrap();
        let later_id = db::insert_feed(&conn, "https://example.com/b.xml", None, None, 0).unwrap();

        let now = now_unix();
        db::update_feed_not_modified(&conn, soon_id, now + 5).unwrap();
        db::update_feed_not_modified(&conn, later_id, now + 500).unwrap();

        let wake = next_wake(&conn, 0);
        let delay = wake.saturating_duration_since(Instant::now());
        assert!(
            delay <= Duration::from_secs(6),
            "expected ~5s, got {delay:?}"
        );
    }

    #[test]
    fn next_wake_falls_back_when_no_feeds() {
        let conn = init_in_memory().unwrap();
        let wake = next_wake(&conn, 0);
        let delay = wake.saturating_duration_since(Instant::now());
        assert!(
            delay >= Duration::from_secs(55),
            "expected ~{IDLE_FALLBACK:?}, got {delay:?}"
        );
    }
}
