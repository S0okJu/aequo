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
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until};

use crate::db::{self, FeedMeta};
use crate::fetch::client;
use crate::fetch::worker::{self, PollOutcome};

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

/// Fetches (no DB lock held) then applies (DB lock held only for the
/// synchronous part) one feed. See [`worker::apply_fetch`]'s doc for
/// why the lock can't simply wrap the whole async call.
async fn poll_one(conn: &Arc<Mutex<Connection>>, meta: &FeedMeta) -> Result<PollOutcome, String> {
    let fetched = client::fetch_conditional(
        &meta.url,
        meta.etag.as_deref(),
        meta.last_modified.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    let guard = conn
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    worker::apply_fetch(&guard, meta, fetched)
}

/// Polls every feed whose `next_fetch_at` has passed, bounding
/// concurrent fetches with a semaphore and collecting results via a
/// `JoinSet` (§4.2). A single feed's failure (network error, bad
/// parse, panic) is logged and doesn't stop the others. Returns the
/// number of feeds attempted, for callers that want to observe cycle
/// size.
pub async fn poll_due_feeds(conn: Arc<Mutex<Connection>>) -> usize {
    let due = {
        let guard = conn.lock().expect("database lock poisoned");
        db::list_due_feed_metas(&guard, now_unix()).unwrap_or_default()
    };
    let attempted = due.len();

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES));
    let mut set = JoinSet::new();

    for meta in due {
        let semaphore = semaphore.clone();
        let conn = conn.clone();
        set.spawn(async move {
            let _permit = semaphore.acquire().await.expect("semaphore closed");
            let url = meta.url.clone();
            (url, poll_one(&conn, &meta).await)
        });
    }

    while let Some(joined) = set.join_next().await {
        match joined {
            // Success: nothing to log. M6 will emit("feed-updated", ...)
            // here once the frontend exists to receive it.
            Ok((_, Ok(_))) => {}
            Ok((url, Err(err))) => eprintln!("poll failed for {url}: {err}"),
            Err(join_err) => eprintln!("fetch task panicked: {join_err}"),
        }
    }

    attempted
}

/// Earliest `next_fetch_at` across all feeds, as a monotonic
/// [`Instant`] the scheduler should sleep until (§4.3). Falls back to
/// [`IDLE_FALLBACK`] when there are no feeds yet.
fn next_wake(conn: &Connection) -> Instant {
    let earliest = db::list_feed_metas(conn)
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.next_fetch_at)
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
pub async fn run(conn: Arc<Mutex<Connection>>) {
    loop {
        let wake_at = {
            let guard = conn.lock().expect("database lock poisoned");
            next_wake(&guard)
        };
        sleep_until(wake_at).await;
        poll_due_feeds(conn.clone()).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant as StdInstant;

    use wiremock::matchers::{method, path};
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
        let attempted = poll_due_feeds(conn).await;
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

        let attempted = poll_due_feeds(conn.clone()).await;
        assert_eq!(attempted, 2);

        let guard = conn.lock().unwrap();
        let good_meta = db::get_feed_meta(&guard, good_id).unwrap().unwrap();
        assert!(good_meta.next_fetch_at > 0); // succeeded, rescheduled

        let bad_meta = db::get_feed_meta(&guard, bad_id).unwrap().unwrap();
        assert_eq!(bad_meta.next_fetch_at, 0); // failed before any DB update
    }

    #[test]
    fn next_wake_uses_earliest_next_fetch_at() {
        let conn = init_in_memory().unwrap();
        let soon_id = db::insert_feed(&conn, "https://example.com/a.xml", None, None, 0).unwrap();
        let later_id = db::insert_feed(&conn, "https://example.com/b.xml", None, None, 0).unwrap();

        let now = now_unix();
        db::update_feed_not_modified(&conn, soon_id, now + 5).unwrap();
        db::update_feed_not_modified(&conn, later_id, now + 500).unwrap();

        let wake = next_wake(&conn);
        let delay = wake.saturating_duration_since(Instant::now());
        assert!(
            delay <= Duration::from_secs(6),
            "expected ~5s, got {delay:?}"
        );
    }

    #[test]
    fn next_wake_falls_back_when_no_feeds() {
        let conn = init_in_memory().unwrap();
        let wake = next_wake(&conn);
        let delay = wake.saturating_duration_since(Instant::now());
        assert!(
            delay >= Duration::from_secs(55),
            "expected ~{IDLE_FALLBACK:?}, got {delay:?}"
        );
    }
}
