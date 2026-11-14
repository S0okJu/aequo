# Aequo — 개발 문서 (아키텍처 설계)

## 1. 개요

Aequo는 macOS 전용 데스크톱 RSS/피드 리더다. 사용자가 RSS(XML) 또는 JSON Feed URL을 등록하면, 등록된 모든 피드의 글을 **최신순으로 통합해서 보여주고**, 글마다 **읽음/안읽음 상태**를 추적한다.

사용자는 10개 이상의 피드를 동시에 구독할 것으로 예상되므로, 이 문서의 모든 설계 결정은 **"피드 개수가 늘어나도 CPU/메모리/네트워크/배터리 사용량이 거의 늘지 않는 것"**을 최우선 기준으로 삼는다.

### 1.1 목표 (Goals)
- RSS 2.0 / Atom / JSON Feed 포맷을 모두 파싱해 하나의 통합 타임라인으로 표시
- 최신순 정렬, 읽음/안읽음 상태의 영속적 저장
- 피드 10~수십 개 기준으로 유휴 상태 메모리 수십 MB, 유휴 CPU 0%대를 유지
- 변경되지 않은 피드는 재파싱하지 않음 (네트워크/CPU 낭비 제거)

### 1.2 비목표 (Non-goals, v1 기준)
- iOS/크로스 플랫폼 지원
- 앱이 완전히 종료된 상태에서의 백그라운드 알림(= 데몬/launchd 상시 실행)
- 피드 콘텐츠의 전문(full-text) 추출(페이월 우회 등)
- 다중 디바이스 동기화(iCloud/서버 동기화)

필요하면 추후 확장(13장 로드맵)에서 다룬다.

## 2. 기술 스택

| 영역 | 선택 | 이유 |
|---|---|---|
| 앱 셸 | **Tauri 2.x** | macOS 네이티브 webview(WKWebView) 사용, Electron처럼 Chromium을 통째로 들고 다니지 않아 메모리/바이너리 크기가 훨씬 작음 |
| 코어/백엔드 | **Rust** | 피드 fetch/파싱/스케줄링/DB 접근을 전부 Rust 쪽에서 수행 → 메인 비용(네트워크 I/O, XML 파싱)이 webview·JS 런타임 밖에서 처리됨 |
| 프론트엔드 | **Svelte + TypeScript** (권장) | 컴파일 타임에 반응성을 처리해 런타임 프레임워크 오버헤드가 거의 없음. React를 써도 구조상 문제는 없지만, "효율 최우선" 요구사항과 가장 잘 맞는 선택은 Svelte |
| 로컬 저장소 | **SQLite** (`rusqlite`, WAL 모드) | 피드 메타데이터·아이템·읽음상태를 구조화 저장, 수천~수만 건의 아이템에도 인덱스 조회로 즉시 응답 |
| HTTP 클라이언트 | **reqwest** (rustls, gzip, HTTP/2) | 커넥션 풀링, 조건부 요청(ETag/Last-Modified) 지원 |
| 피드 파싱 | **feed-rs** | RSS 2.0/0.9x, Atom, JSON Feed를 단일 API로 파싱 — 포맷별 파서를 직접 구현하지 않음 |
| 비동기 런타임 | **tokio** | 폴링 스케줄러와 동시 fetch를 경량 태스크로 처리 |

> 사용자가 이미 "Tauri(Rust + 웹프론트)"를 선택했으므로, 이 문서는 이 스택을 기준으로 작성한다. 프론트엔드 프레임워크(Svelte vs React)는 바꿔도 아래 아키텍처(3~7장)에는 영향이 없다 — 프론트엔드는 순수 UI 레이어이기 때문이다.

## 3. 전체 아키텍처

핵심 원칙: **"무거운 작업은 전부 Rust 코어에서, webview는 상태를 보여주기만 한다."**

```
┌─────────────────────────────── Tauri App ───────────────────────────────┐
│                                                                          │
│   ┌────────────────────┐          invoke(cmd)        ┌───────────────┐ │
│   │   Frontend (UI)     │ ───────────────────────────▶│               │ │
│   │  Svelte + TS         │                              │  Rust Core    │ │
│   │  - 피드/아이템 목록   │ ◀─────────────────────────── │  (Backend)    │ │
│   │  - 상세 보기          │      emit(event)             │               │ │
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
│                                   │ (세마포어로 동시성 제한) │                 │ │
│                                   └──────────┬──────────┘                  │ │
└──────────────────────────────────────────────┼─────────────────────────────┘
                                                ▼
                                   (피드 서버들, HTTP/HTTPS)
```

### 3.1 레이어 책임

- **Frontend (UI)**: 상태를 받아 그리기만 한다. 피드 fetch, 파싱, 정렬, 중복 제거는 하지 않는다. 사용자 액션(피드 추가, 읽음 표시, 새로고침 클릭)을 `invoke`로 Rust에 전달한다.
- **Rust Core**: Tauri 커맨드 핸들러. `add_feed`, `list_items`, `mark_read`, `refresh_feed(feed_id)` 등을 노출.
- **Poll Scheduler**: 등록된 각 피드의 다음 폴링 시각을 관리하고, 시각이 되면 Fetch Worker에 작업을 넘긴다.
- **Fetch Workers**: 실제 HTTP 요청 + 파싱 + DB 반영을 수행하는 tokio 태스크. 동시 실행 개수를 세마포어로 제한한다.
- **SQLite**: 유일한 진실의 소스(source of truth). 프론트엔드는 항상 SQLite에서 읽은 결과만 본다.

### 3.2 Frontend ↔ Backend 통신

- **Pull**: 프론트엔드가 화면을 그릴 때 `invoke("list_items", {...})`로 필요한 데이터만 가져온다(가상 스크롤과 페이지네이션으로 한 번에 수백 건만 로드).
- **Push**: 백그라운드 폴링이 새 글을 찾으면, 전체 목록을 다시 fetch하라고 프론트엔드에 강제하지 않고 `emit("feed-updated", { feedId, newItemCount })` 이벤트만 보낸다. 프론트엔드는 이벤트를 받았을 때만 해당 구간을 다시 쿼리한다. → **프론트엔드가 백엔드를 폴링(polling)하지 않는다.** 이것이 유휴 CPU 사용량을 0에 가깝게 만드는 핵심 설계다.

## 4. 피드 처리 효율화 — 핵심 설계

10개 이상의 피드를 "효율적으로" 처리한다는 요구사항은 세 가지 독립적인 문제로 나뉜다: **(a) 불필요한 네트워크/파싱 방지, (b) 동시성 제한, (c) 유휴 시 자원 소비 0.**

### 4.1 조건부 요청으로 재파싱 방지 (가장 큰 효율 이득)

대부분의 RSS 리더가 저지르는 가장 큰 낭비는 "변경 없는 피드를 매번 전체 다운로드 + 파싱"하는 것이다. Aequo는 피드마다 마지막 응답의 `ETag`/`Last-Modified` 헤더를 저장하고, 다음 요청에 `If-None-Match` / `If-Modified-Since`를 실어 보낸다.

- 서버가 `304 Not Modified`를 반환하면 → 바디 다운로드도, XML/JSON 파싱도 하지 않고 즉시 종료.
- `200 OK`일 때만 feed-rs로 파싱.

```rust
struct FeedMeta {
    id: i64,
    url: String,
    etag: Option<String>,
    last_modified: Option<String>,
    interval_secs: u32,   // 피드별 폴링 간격 (기본 900s = 15분)
    next_fetch_at: i64,   // unix timestamp
}
```

이 한 가지 최적화만으로, 변경이 드문 피드(대부분의 블로그)는 실질적인 CPU/네트워크 비용이 "HTTP 헤더 확인" 수준으로 줄어든다.

### 4.2 동시성 제한 (Bounded Concurrency)

피드가 10~30개로 늘어나도 한 번에 전부 동시 요청을 보내지 않는다. `tokio::sync::Semaphore`로 **동시 fetch 최대 4개**로 제한한다.

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
while let Some(res) = set.join_next().await { /* 에러 로깅, 재시도 큐 반영 */ }
```

- 피드 수가 늘어나도 동시 네트워크 커넥션 수는 상한선을 넘지 않음 → CPU 스파이크/네트워크 혼잡 방지.
- 개별 요청은 timeout(예: 10초)을 걸어, 느리거나 응답 없는 피드 하나가 전체 폴링 사이클을 지연시키지 않게 한다.

### 4.3 유휴 시 자원 소비 (Idle Cost)

- 앱이 켜져 있지만 사용자가 조작하지 않는 동안: Poll Scheduler는 `tokio::time::sleep_until(next_wake)`로 **다음 폴링이 필요한 가장 이른 시각까지 완전히 잠든다.** busy-loop나 고정 짧은 interval(예: 1초마다 체크)로 깨어나지 않는다.
- Frontend는 이벤트가 올 때만 리렌더링(2.2절). 타이머 기반 polling UI를 쓰지 않는다.
- SQLite는 WAL 모드로 열어, 읽기(UI)와 쓰기(백그라운드 fetch)가 서로 블로킹하지 않게 한다.

### 4.4 파싱 자체의 효율

- `feed-rs`는 스트리밍 파서(`quick-xml`) 기반이라 전체 문서를 DOM으로 올리지 않는다.
- RSS/Atom/JSON Feed를 포맷 감지 후 동일한 내부 모델(`feed_rs::model::Feed`)로 변환하므로, 상위 로직(중복 제거, 저장)은 포맷을 신경 쓰지 않는다.
- 아이템 중복 판별은 `guid`(없으면 `link`를 정규화한 값)를 해시해 SQLite의 `UNIQUE` 제약으로 처리 — 애플리케이션 레벨에서 전체 아이템을 순회 비교하지 않는다.

### 4.5 (선택적 확장) 적응형 폴링 간격

피드마다 실제 업데이트 빈도가 다르다(하루에 10번 올리는 뉴스 피드 vs 한 달에 1번인 블로그). v1 이후 확장으로, 최근 N회 폴링에서 새 글이 없었던 피드는 간격을 점진적으로 늘리고(예: 15분 → 1시간 → 4시간, 최대 상한), 새 글이 자주 발견되면 간격을 다시 줄이는 **적응형 간격 조정**을 둘 수 있다. (13장 로드맵에 기재, v1 범위 아님)

## 5. 데이터 모델 (SQLite)

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
    guid_hash       TEXT NOT NULL,   -- guid 또는 정규화된 link의 해시
    title           TEXT,
    link            TEXT,
    summary         TEXT,
    published_at    INTEGER NOT NULL,  -- unix timestamp, 정렬 기준
    fetched_at      INTEGER NOT NULL,
    is_read         INTEGER NOT NULL DEFAULT 0,  -- 0/1
    UNIQUE(feed_id, guid_hash)
);

CREATE INDEX idx_items_published ON items(published_at DESC);
CREATE INDEX idx_items_unread ON items(is_read, published_at DESC);
```

- 통합 타임라인 조회: `SELECT ... FROM items ORDER BY published_at DESC LIMIT ? OFFSET ?` — 인덱스 덕분에 아이템 수가 늘어도 느려지지 않는다.
- 읽음 처리: `UPDATE items SET is_read = 1 WHERE id = ?` (단건) 또는 `WHERE feed_id = ?`(피드 전체 읽음 처리).

## 6. 주요 플로우

### 6.1 피드 추가
1. 사용자가 URL 입력 → `invoke("add_feed", { url })`
2. Rust가 URL로 요청 → `Content-Type`과 바디를 보고 RSS/Atom/JSON Feed 판별 (feed-rs가 자동 감지)
3. 파싱 성공 시 `feeds` 테이블에 메타데이터 저장, 초기 아이템 일괄 저장
4. 실패 시(잘못된 URL, 파싱 불가) 에러를 그대로 프론트엔드에 반환해 사용자에게 표시

### 6.2 백그라운드 폴링
1. Poll Scheduler가 `next_fetch_at <= now`인 피드들을 조회
2. 세마포어로 제한된 동시성으로 Fetch Worker 실행
3. 각 워커: 조건부 요청 → (304면 종료) / (200이면 파싱 → 신규 아이템만 INSERT, 기존 `etag`/`next_fetch_at` 갱신)
4. 신규 아이템이 있으면 `emit("feed-updated", {...})`

### 6.3 읽음 처리 & 목록 표시
- 목록은 항상 `published_at DESC`로 조회
- 안읽은 아이템은 UI에서 bold/점 표시, 읽은 아이템은 흐리게 표시 (목업은 8장)
- 아이템을 열면(또는 일정 시간 노출되면) `invoke("mark_read", { itemId })` 호출 → 즉시 UI 반영(옵티미스틱 업데이트) + DB 반영

## 7. 에러 처리 및 재시도

- 네트워크 오류/타임아웃: 피드별 `last_error`에 기록, UI에 경고 아이콘 표시. 전체 폴링 사이클을 막지 않음(4.2절 타임아웃).
- 재시도: 지수 백오프(예: 1분 → 5분 → 30분, 최대 간격은 피드의 기본 interval을 넘지 않음). 연속 실패해도 다음 정상 폴링 때 자동 복구.
- 파싱 오류: 피드 포맷이 깨진 경우 해당 사이클만 스킵하고 다음 폴링에서 재시도.

## 8. UI 구조 (개요)

```
┌─────────────┬──────────────────────────────┬────────────────┐
│  피드 목록    │   통합 타임라인 (최신순)        │   상세 보기       │
│  - 전체       │   ● 안읽음  제목 · 피드명 · 시각 │   (선택한 글의     │
│  - 피드 A (3) │   ○ 읽음   제목 · 피드명 · 시각 │    본문/요약)     │
│  - 피드 B     │   ...                         │                │
└─────────────┴──────────────────────────────┴────────────────┘
```

- 가상 스크롤(virtual list)로 수천 건이 쌓여도 DOM 노드는 화면에 보이는 만큼만 유지.
- 피드별 안읽음 카운트는 SQLite에서 `COUNT(*) WHERE is_read = 0`으로 집계.

## 9. 보안 고려사항

- 외부 피드의 HTML 콘텐츠(요약/본문)는 webview에서 그대로 렌더링하지 않고 **sanitize(허용 태그 화이트리스트)** 후 표시 — 악성 피드를 통한 스크립트 삽입 방지.
- Tauri의 CSP를 설정해 webview가 임의 origin에 네트워크 요청/리소스 로드를 하지 못하게 제한(모든 외부 fetch는 Rust 백엔드를 거침).

## 10. 빌드/배포

- `tauri build`로 `.app` / `.dmg` 생성.
- macOS 배포 시 Developer ID 서명 + notarization 필요 (Gatekeeper 통과).
- 자동 업데이트는 Tauri Updater 플러그인 사용 고려(v1 이후).

## 11. 테스트 전략

- Rust 유닛 테스트: feed-rs 파싱 래퍼, 중복 제거 로직, 조건부 요청 헤더 처리, 적응형 간격 계산(해당 시)
- 통합 테스트: 로컬 mock HTTP 서버(예: `wiremock`)로 304/200/타임아웃 시나리오 검증
- 프론트엔드: 컴포넌트 단위 테스트(읽음/안읽음 렌더링, 정렬) + Tauri 커맨드는 mock으로 대체

## 12. 성능 목표 (검증 기준)

| 지표 | 목표 |
|---|---|
| 유휴 메모리 | 피드 20개, 아이템 5,000건 기준 80MB 이하 |
| 유휴 CPU | 폴링 사이의 구간에서 0%대 |
| 폴링 1회 소요 시간 (피드 20개, 변경 없음) | 수백 ms 이내 (대부분 304 응답) |
| 앱 콜드 스타트 | 1초 이내 첫 화면 표시 |

## 13. 로드맵 (v1 범위 밖)

- 적응형 폴링 간격 자동 조정 (4.5절)
- OPML 가져오기/내보내기
- 전역 검색, 태그/폴더 분류
- 다크모드
- 메뉴바 상시 아이콘 + launchd 기반 완전 백그라운드 폴링(앱 종료 중에도 갱신)
