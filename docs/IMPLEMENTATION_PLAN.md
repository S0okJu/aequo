# Aequo — Feature Implementation Plan (v1)

> This document breaks down the v1 scope defined in `docs/ARCHITECTURE.md` (merged RSS/Atom/JSON Feed timeline,
> read/unread, efficient polling) into an actual implementation order and work units. Milestones assume sequential
> progress, but M3 (backend core) and M7 (UI skeleton) can be worked on in parallel.

## Status markers

- [ ] not started · [~] in progress · [x] done

---

## M0. Initial Project Setup

- [x] Scaffold the Tauri 2.x project (`npm create tauri-app`)
- [x] Set up the frontend framework (Svelte + TypeScript)
- [x] Organize the Rust workspace structure (split modules like `src-tauri/src/{commands,db,fetch,scheduler}`)
- [x] Add dependencies: `rusqlite` (bundled, WAL), `reqwest` (rustls, gzip), `feed-rs`, `tokio`
- [x] Set up `.gitignore` and lint/format config (`rustfmt`, `clippy`, `eslint`/`prettier`)
- [x] Get the basic app window running (confirm `tauri dev` works, even with an empty screen)
- [x] Generate the Tauri app icon set from `img/logo.png` (`tauri icon img/logo.png`)

## M1. Data Model & DB Layer

- [x] Write the `feeds` and `items` table schemas (as in ARCHITECTURE.md Chapter 5)
- [x] Create indexes: `idx_items_published`, `idx_items_unread`
- [x] SQLite connection init module — enable WAL mode, create the DB file in the app data directory
- [x] Decide on a migration approach (plain `CREATE TABLE IF NOT EXISTS` vs. versioned migrations)
- [x] DB access layer (repository functions): `insert_feed`, `get_feed_by_id`, `list_feeds`, `upsert_items`, `list_items(limit, offset, unread_only)`, `mark_read(item_id)`, `mark_feed_read(feed_id)`, `count_unread(feed_id)`
- [x] Unit tests: schema creation, and confirm duplicate inserts are safely ignored by the `UNIQUE(feed_id, guid_hash)` constraint

## M2. Feed Parsing & "Add Feed" Flow

- [x] Write a `feed-rs` wrapper — convert RSS/Atom/JSON Feed input into the shared internal model
- [x] GUID hashing logic: hash the `guid` if present, otherwise hash the normalized `link`
- [x] Implement the `add_feed(url)` Tauri command
  - [x] Fetch the URL → detect format from `Content-Type` + body (feed-rs auto-detection)
  - [x] On successful parse: insert into `feeds` + bulk-insert initial items
  - [x] On parse/network failure: return the error as-is to the frontend
- [x] Unit tests: verify parsing results against RSS 2.0 / Atom / JSON Feed sample files
- [x] Manual test: register 3–5 real blog RSS feeds

## M3. Conditional Requests & Fetch Workers (core efficiency work)

- [x] `FeedMeta` struct (`etag`, `last_modified`, `interval_secs`, `next_fetch_at`) mapped to DB columns
- [x] Apply conditional requests: set `If-None-Match` / `If-Modified-Since` headers on each request
- [x] Response handling branches
  - [x] `304 Not Modified` → exit immediately with no body download/parsing, update only `next_fetch_at`
  - [x] `200 OK` → parse → insert only new items (ignore UNIQUE conflicts) → update `etag`/`last_modified`/`next_fetch_at`
- [x] Set a timeout per request (e.g., 10s)
- [x] Integration tests: verify all three scenarios (304/200/timeout) using `wiremock`

## M4. Bounded Concurrency & Poll Scheduler

- [x] Limit concurrent fetches with `tokio::sync::Semaphore` (default 4)
- [x] Run fetch workers via `JoinSet` + log errors
- [x] Poll Scheduler: query feeds where `next_fetch_at <= now` → dispatch workers under the semaphore limit
- [x] Fully sleep when idle: `tokio::time::sleep_until(next_wake)` — no busy-loop/fixed-interval wakeups
- [x] Start the scheduler on app launch, run it as a persistent background task
- [x] Manual test: register 10+ feeds, confirm idle CPU in Activity Monitor

## M5. Error Handling & Retry

- [x] Record errors per feed in the `last_error` column
- [x] Exponential backoff retry queue (1min → 5min → 30min, capped at the feed's base interval)
- [x] Confirm network errors/timeouts don't block the overall polling cycle (one feed's failure doesn't affect others)
- [x] On parse errors, skip only that cycle and retry on the next poll
- [x] Unit tests: backoff interval calculation function

## M6. Frontend ↔ Backend Communication (event-based)

- [x] Expose Tauri commands: `add_feed`, `list_feeds`, `list_items`, `mark_read`, `mark_feed_read`, `refresh_feed(feed_id)`
- [x] `emit("feed-updated", { feedId, newItemCount })` when new items appear
- [x] Frontend event listener: re-query only the affected range when the event arrives (the frontend never polls the backend)
- [x] Manual test: confirm the UI updates without a timer when new items show up during polling

## M7. UI Implementation

### Design principles (NetNewsWire / Reeder style)

- Native macOS feel: use the system font (San Francisco), rely on macOS's default window chrome/title bar, avoid custom chrome
- Minimal 3-pane list layout — no decorative elements (gradients, shadows, unnecessary icons), prioritize content density
- Generous whitespace, but not Notion/Linear-style wide whitespace — optimize for "list scannability," fitting as many items on screen as possible
- Neutral (grayscale) color base + a single accent color (for unread indicators, selection state, etc.). Don't scatter a brand color throughout the UI
- Limit the type hierarchy to 2–3 levels: title (semibold) / metadata–feed name–timestamp (small, low-contrast) / body
- Distinguish read/unread by contrast, weight, or a dot — not color — and avoid excessive badges/count bubbles
- Dark mode is out of v1 scope (Chapter 13), but avoid hardcoding colors — structure them as CSS variables/tokens so the system can adapt to light/dark naturally and dark mode can be added later by swapping color tokens, with no layout changes

- [ ] 3-pane layout: feed list / merged timeline / detail view
- [ ] Feed list: "All" + per-feed unread count (`COUNT(*) WHERE is_read = 0`)
- [ ] Timeline: sorted by `published_at DESC`, visually distinguishing unread (bold/dot) from read (dimmed)
- [ ] Apply virtual scrolling — keep DOM node count bounded when rendering thousands of items
- [ ] Pagination/infinite scroll calling `list_items(limit, offset)`
- [ ] Call `mark_read` on item open + optimistic UI update
- [ ] Add-feed UI (URL input form, error message display)
- [ ] Detail view: render body/summary

## M8. Security

- [ ] Sanitize feed HTML content (allowed-tag whitelist) before rendering
- [ ] Configure Tauri CSP — block the webview from making requests/loading resources from arbitrary origins
- [ ] Confirm all external fetches go through the Rust backend (no direct frontend fetch)

## M9. Test Cleanup

- [ ] Confirm Rust unit test coverage: parsing wrapper, deduplication, conditional request headers, backoff calculation
- [ ] Integration tests: mock-HTTP-server-based 304/200/timeout
- [ ] Frontend component tests: read/unread rendering, sorting, Tauri commands mocked

## M10. Performance Verification (release criteria)

- [ ] Idle memory: ≤ 80MB with 20 feeds / 5,000 items
- [ ] Idle CPU: near 0% between polling cycles
- [ ] Time per poll cycle (20 feeds, no changes): under a few hundred ms (mostly 304s)
- [ ] Cold start: first screen shown within 1 second
- [ ] If any of the above misses its target, investigate and roll back to M3–M4 for re-review

## M11. Build/Deployment

- [ ] Confirm `tauri build` produces a working `.app`/`.dmg`
- [ ] Confirm the app icon (generated from `img/logo.png` in M0) appears correctly in the built `.app`/`.dmg`
- [ ] Developer ID signing + notarization (to pass Gatekeeper)
- [ ] (Optional) Decide whether to adopt the Tauri Updater plugin — fine to defer past v1

---

## Out of scope (not v1, do not start)

From ARCHITECTURE.md's Chapter 13 roadmap — the following are not included in this plan:

- Automatic adaptive polling interval
- OPML import/export
- Global search, tags/folders
- Dark mode
- Persistent menu bar icon + launchd-based fully-background polling
