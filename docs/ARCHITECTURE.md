# Aequo — Development Document (Architecture Design)

## 1. Overview

Aequo is a macOS-only desktop RSS/feed reader. When a user registers RSS (XML) or JSON Feed URLs, it shows items from all registered feeds **merged into a single, most-recent-first timeline**, and tracks **read/unread** status per item.

Since users are expected to subscribe to 10+ feeds at once, every design decision in this document is guided first by **"CPU/memory/network/battery usage should barely increase as the number of feeds grows."**

### 1.1 Goals
- Parse RSS 2.0 / Atom / JSON Feed formats and merge them into a single timeline
- Most-recent-first sorting, with persistent storage of read/unread state
- Keep idle memory in the tens of MB and idle CPU near 0% for 10–dozens of feeds
- Never re-parse a feed that hasn't changed (eliminate wasted network/CPU)

### 1.2 Non-goals (v1 scope)
- iOS / cross-platform support
- Background notifications while the app is fully quit (i.e., an always-running daemon/launchd)
- Full-text extraction of feed content (e.g., paywall bypass)
- Multi-device sync (iCloud/server sync)

These may be addressed later under future expansion (Chapter 13 roadmap) if needed.

## 2. Tech Stack

| Area | Choice | Rationale |
|---|---|---|
| App shell | **Tauri 2.x** | Uses macOS's native webview (WKWebView) instead of bundling all of Chromium like Electron, keeping memory/binary size much smaller |
| Core/backend | **Rust** | Feed fetch/parsing/scheduling/DB access all happen in Rust → the main costs (network I/O, XML parsing) run outside the webview/JS runtime |
| Frontend | **Svelte + TypeScript** (recommended) | Reactivity is handled at compile time, so there's almost no runtime framework overhead. React would work structurally too, but Svelte fits the "efficiency first" requirement best |
| Local storage | **SQLite** (`rusqlite`, WAL mode) | Structured storage for feed metadata, items, and read state; index-based lookups stay instant even with thousands to tens of thousands of items |
| HTTP client | **reqwest** (rustls, gzip, HTTP/2) | Connection pooling, conditional request (ETag/Last-Modified) support |
| Feed parsing | **feed-rs** | Parses RSS 2.0/0.9x, Atom, and JSON Feed through a single API — no need to implement per-format parsers |
| Async runtime | **tokio** | Handles the polling scheduler and concurrent fetches as lightweight tasks |

> Since the user has already chosen "Tauri (Rust + web frontend)," this document is written around that stack. Swapping the frontend framework (Svelte vs React) doesn't affect the architecture in Chapters 3–7 — the frontend is a pure UI layer.

## 3. Overall Architecture

Core principle: **"All heavy work happens in the Rust core; the webview only displays state."**

```
┌─────────────────────────────── Tauri App ───────────────────────────────┐
│                                                                          │
│   ┌────────────────────┐          invoke(cmd)        ┌───────────────┐ │
│   │   Frontend (UI)     │ ───────────────────────────▶│               │ │
│   │  Svelte + TS         │                              │  Rust Core    │ │
│   │  - feed/item list    │ ◀─────────────────────────── │  (Backend)    │ │
│   │  - detail view        │      emit(event)             │               │ │
│   └────────────────────┘                              └──────┬────────┘ │
│                                                                │          │
│                                              ┌─────────────────┼────────┐ │
│                                              │                 ▼        │ │
│                                   ┌──────────────────┐  ┌────────────┐ │ │
│                                   │ Poll Scheduler     │  │  SQLite    │ │ │
│                                   │ (tokio interval)   │  │  (WAL)     │ │ │
│                                   └─────────┬──────────┘  └─────▲──────┘ │ │
│                                              │                   │        │ │
│                                   ┌──────────▼──────────┐        │        │ │
│                                   │ Fetch Workers         │────────┘        │ │
│                                   │ (concurrency limited   │                 │ │
│                                   │  via semaphore)         │                 │ │
│                                   └──────────┬──────────┘                  │ │
└──────────────────────────────────────────────┼─────────────────────────────┘
                                                ▼
                                   (feed servers, HTTP/HTTPS)
```

### 3.1 Layer Responsibilities

- **Frontend (UI)**: Only receives state and renders it. It does not fetch, parse, sort, or deduplicate feeds. User actions (add feed, mark as read, click refresh) are forwarded to Rust via `invoke`.
- **Rust Core**: Tauri command handlers. Exposes `add_feed`, `list_items`, `mark_read`, `refresh_feed(feed_id)`, etc.
- **Poll Scheduler**: Tracks the next poll time for each registered feed and hands work to Fetch Workers when that time arrives.
- **Fetch Workers**: tokio tasks that perform the actual HTTP request + parsing + DB writes. The number of concurrent workers is capped with a semaphore.
- **SQLite**: The single source of truth. The frontend always sees results read from SQLite.

### 3.2 Frontend ↔ Backend Communication

- **Pull**: When the frontend renders a screen, it pulls only the data it needs via `invoke("list_items", {...})` (virtual scrolling + pagination load only a few hundred rows at a time).
- **Push**: When background polling finds new items, it doesn't force the frontend to refetch the whole list — it just sends `emit("feed-updated", { feedId, newItemCount })`. The frontend re-queries only the affected range, and only when it receives the event. → **The frontend never polls the backend.** This is the core design that keeps idle CPU usage near zero.

## 4. Feed Processing Efficiency — Core Design

Handling 10+ feeds "efficiently" breaks down into three independent problems: **(a) avoiding unnecessary network/parsing, (b) bounding concurrency, (c) zero idle resource usage.**

### 4.1 Conditional Requests to Avoid Re-parsing (the biggest efficiency win)

The biggest waste most RSS readers commit is "fully re-downloading and re-parsing a feed that hasn't changed every single time." Aequo stores the `ETag`/`Last-Modified` headers from each feed's last response and sends `If-None-Match` / `If-Modified-Since` on the next request.

- If the server returns `304 Not Modified` → no body download, no XML/JSON parsing — exit immediately.
- Parse with feed-rs only on `200 OK`.

```rust
struct FeedMeta {
    id: i64,
    url: String,
    etag: Option<String>,
    last_modified: Option<String>,
    interval_secs: u32,   // per-feed poll interval (default 900s = 15 min)
    next_fetch_at: i64,   // unix timestamp
}
```

This single optimization alone reduces real CPU/network cost for rarely-changing feeds (most blogs) to roughly "checking an HTTP header."

### 4.2 Bounded Concurrency

As the feed count grows to 10–30, Aequo doesn't fire all requests concurrently. `tokio::sync::Semaphore` caps **concurrent fetches at 4**.

```rust
let semaphore = Arc::new(Semaphore::new(4));
let mut set = JoinSet::new();
for feed in due_feeds {
    let sem = semaphore.clone();
    set.spawn(async move {
        let _permit = sem.acquire().await.unwrap();
        fetch_and_store(feed).await
    });
}
while let Some(res) = set.join_next().await { /* log errors, update retry queue */ }
```

- The number of concurrent network connections never exceeds the cap even as feed count grows → prevents CPU spikes and network congestion.
- Each request has a timeout (e.g., 10s) so a single slow/unresponsive feed doesn't delay the whole polling cycle.

### 4.3 Idle Resource Cost

- While the app is open but the user isn't interacting: the Poll Scheduler **fully sleeps via `tokio::time::sleep_until(next_wake)` until the earliest time the next poll is actually needed.** It does not wake on a busy loop or a fixed short interval (e.g., checking every 1s).
- The frontend only re-renders when an event arrives (see 3.2). It does not use timer-based polling for the UI.
- SQLite is opened in WAL mode so reads (UI) and writes (background fetch) don't block each other.

### 4.4 Parsing Efficiency

- `feed-rs` is built on a streaming parser (`quick-xml`), so it never loads the full document into a DOM.
- RSS/Atom/JSON Feed are auto-detected and converted into the same internal model (`feed_rs::model::Feed`), so upstream logic (deduplication, storage) doesn't need to care about format.
- Item deduplication hashes the `guid` (or a normalized `link` if no guid) and relies on SQLite's `UNIQUE` constraint — no application-level full-item comparison scan.

### 4.5 (Optional extension) Adaptive polling interval

Feeds have different real update frequencies (a news feed posting 10x/day vs. a blog posting once a month). As a post-v1 extension, feeds with no new items across the last N polls could have their interval gradually increased (e.g., 15min → 1h → 4h, up to a cap), and shrunk again if new items start appearing frequently — an **adaptive interval** scheme. (Listed in the Chapter 13 roadmap; not in v1 scope.)

## 5. Data Model (SQLite)

```sql
CREATE TABLE feeds (
    id              INTEGER PRIMARY KEY,
    url             TEXT NOT NULL UNIQUE,
    title           TEXT,
    format          TEXT,            -- 'rss' | 'atom' | 'json_feed'
    etag            TEXT,
    last_modified   TEXT,
    interval_secs   INTEGER NOT NULL DEFAULT 900,
    next_fetch_at   INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    created_at      INTEGER NOT NULL
);

CREATE TABLE items (
    id              INTEGER PRIMARY KEY,
    feed_id         INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    guid_hash       TEXT NOT NULL,   -- hash of guid, or of the normalized link if no guid
    title           TEXT,
    link            TEXT,
    summary         TEXT,
    published_at    INTEGER NOT NULL,  -- unix timestamp, the sort key
    fetched_at      INTEGER NOT NULL,
    is_read         INTEGER NOT NULL DEFAULT 0,  -- 0/1
    UNIQUE(feed_id, guid_hash)
);

CREATE INDEX idx_items_published ON items(published_at DESC);
CREATE INDEX idx_items_unread ON items(is_read, published_at DESC);
```

- Merged timeline query: `SELECT ... FROM items ORDER BY published_at DESC LIMIT ? OFFSET ?` — stays fast as item count grows thanks to the index.
- Marking read: `UPDATE items SET is_read = 1 WHERE id = ?` (single item) or `WHERE feed_id = ?` (mark an entire feed as read).

## 6. Main Flows

### 6.1 Adding a feed
1. User enters a URL → `invoke("add_feed", { url })`
2. Rust requests the URL → determines RSS/Atom/JSON Feed from `Content-Type` and body (feed-rs auto-detects)
3. On successful parse, store metadata in `feeds` and bulk-insert the initial items
4. On failure (invalid URL, unparseable), return the error as-is to the frontend to display

### 6.2 Background polling
1. The Poll Scheduler queries feeds where `next_fetch_at <= now`
2. Fetch Workers run with concurrency bounded by the semaphore
3. Each worker: sends a conditional request → (exits on 304) / (on 200, parses → inserts only new items → updates `etag`/`next_fetch_at`)
4. If new items were found, `emit("feed-updated", {...})`

### 6.3 Marking read & displaying the list
- The list is always queried by `published_at DESC`
- Unread items show as bold/with a dot in the UI, read items appear dimmed (mockup in Chapter 8)
- Opening an item (or after it's been visible for a while) calls `invoke("mark_read", { itemId })` → immediate optimistic UI update + DB write

## 7. Error Handling & Retry

- Network errors/timeouts: recorded in the feed's `last_error`, shown as a warning icon in the UI. Doesn't block the overall polling cycle (see the 4.2 timeout).
- Retry: exponential backoff (e.g., 1min → 5min → 30min, capped at the feed's base interval). Recovers automatically on the next normal poll even after consecutive failures.
- Parse errors: if a feed's format is broken, skip that cycle only and retry on the next poll.

## 8. UI Structure (overview)

```
┌─────────────┬──────────────────────────────┬────────────────┐
│  Feed list   │   Merged timeline (newest)     │   Detail view   │
│  - All       │   ● unread  title · feed · time │   (body/summary  │
│  - Feed A (3)│   ○ read    title · feed · time │    of selected   │
│  - Feed B    │   ...                          │    item)         │
└─────────────┴──────────────────────────────┴────────────────┘
```

- Virtual scroll (virtual list) keeps DOM nodes limited to what's visible even with thousands of items.
- Per-feed unread counts are aggregated from SQLite via `COUNT(*) WHERE is_read = 0`.

## 9. Security Considerations

- HTML content from external feeds (summary/body) is not rendered as-is in the webview — it's **sanitized (allowed-tag whitelist)** before display, preventing script injection via a malicious feed.
- Tauri's CSP is configured to prevent the webview from making network requests or loading resources from arbitrary origins (all external fetches go through the Rust backend).

## 10. Build/Deployment

- `tauri build` produces the `.app` / `.dmg`.
- macOS distribution requires Developer ID signing + notarization (to pass Gatekeeper).
- Consider the Tauri Updater plugin for auto-updates (post-v1).

## 11. Testing Strategy

- Rust unit tests: feed-rs parsing wrapper, deduplication logic, conditional request header handling, adaptive interval calculation (if applicable)
- Integration tests: verify 304/200/timeout scenarios against a local mock HTTP server (e.g., `wiremock`)
- Frontend: component-level tests (read/unread rendering, sorting) + mock Tauri commands

## 12. Performance Targets (acceptance criteria)

| Metric | Target |
|---|---|
| Idle memory | ≤ 80MB with 20 feeds / 5,000 items |
| Idle CPU | Near 0% between polling cycles |
| Time per poll cycle (20 feeds, no changes) | Under a few hundred ms (mostly 304 responses) |
| App cold start | First screen shown within 1 second |

## 13. Roadmap (out of v1 scope)

- Automatic adaptive polling interval (section 4.5)
- OPML import/export
- Global search, tags/folders
- Dark mode
- Persistent menu bar icon + launchd-based fully-background polling (updates even while the app is quit)
