//! Fixed DB/serialization workload. This is NOT a native WebView benchmark.
#![allow(dead_code, unused_imports)]

#[path = "../src/db/mod.rs"]
mod db;

use std::path::Path;
use std::time::Instant;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: list_workload <fixture-copy.db>");
    assert!(
        Path::new(&path).is_file(),
        "fixture copy must already exist"
    );
    let setup = Instant::now();
    let conn = db::init(Path::new(&path)).expect("open and migrate fixture");
    let setup_ms = setup.elapsed().as_secs_f64() * 1000.0;
    for run in 1..=3 {
        let started = Instant::now();
        let mut cursor = None;
        let mut items = 0;
        let mut payload_bytes = 0;
        let mut page_ms = Vec::new();
        loop {
            let page_started = Instant::now();
            let page =
                db::list_items(&conn, &db::ItemFilter::default(), 50, cursor).expect("query page");
            if page.is_empty() {
                break;
            }
            payload_bytes += serde_json::to_vec(&page)
                .expect("serialize IPC payload")
                .len();
            cursor = page.last().map(|item| db::ItemCursor {
                published_at: item.published_at,
                id: item.id,
            });
            items += page.len();
            page_ms.push(page_started.elapsed().as_secs_f64() * 1000.0);
        }
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        page_ms.sort_by(f64::total_cmp);
        let p95 = if page_ms.is_empty() {
            0.0
        } else {
            page_ms[(page_ms.len() * 95).div_ceil(100) - 1]
        };
        println!(
            "{}",
            serde_json::json!({
                "scope": "DB query + IPC JSON serialization only; no WebView or rendering",
                "run": run, "items": items, "pages": page_ms.len(), "payloadBytes": payload_bytes,
                "elapsedMs": elapsed_ms, "pageP95Ms": p95, "setupMs": setup_ms,
            })
        );
    }
}
