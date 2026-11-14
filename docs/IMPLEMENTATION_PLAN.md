# Aequo — 기능 구현 계획서 (v1)

> 이 문서는 `docs/ARCHITECTURE.md`에서 정의한 v1 범위(RSS/Atom/JSON Feed 통합 타임라인, 읽음/안읽음, 효율적 폴링)를
> 실제 구현 순서와 작업 단위로 분해한 것이다. 마일스톤은 순차 진행을 가정하지만, M3(백엔드 코어)과 M7(UI 뼈대)은
> 병행 가능하다.

## 진행 상태 표기

- [ ] 미시작 · [~] 진행중 · [x] 완료

---

## M0. 프로젝트 초기 설정

- [ ] Tauri 2.x 프로젝트 스캐폴딩 (`npm create tauri-app`)
- [ ] 프론트엔드 프레임워크 설정 (Svelte + TypeScript)
- [ ] Rust 워크스페이스 구조 정리 (`src-tauri/src/{commands,db,fetch,scheduler}` 등 모듈 분리)
- [ ] 의존성 추가: `rusqlite`(bundled, WAL), `reqwest`(rustls, gzip), `feed-rs`, `tokio`
- [ ] `.gitignore`, 린트/포맷 설정 (`rustfmt`, `clippy`, `eslint`/`prettier`)
- [ ] 앱 기본 윈도우 띄우기 (빈 화면이라도 `tauri dev` 정상 동작 확인)

## M1. 데이터 모델 & DB 레이어

- [ ] `feeds`, `items` 테이블 스키마 작성 (ARCHITECTURE.md 5장 그대로)
- [ ] 인덱스 생성: `idx_items_published`, `idx_items_unread`
- [ ] SQLite 연결 초기화 모듈 — WAL 모드 설정, 앱 데이터 디렉토리에 DB 파일 생성
- [ ] 마이그레이션 적용 방식 결정 (단순 `CREATE TABLE IF NOT EXISTS` vs 버전 관리 마이그레이션)
- [ ] DB 접근 레이어 (repository 함수): `insert_feed`, `get_feed_by_id`, `list_feeds`, `upsert_items`, `list_items(limit, offset, unread_only)`, `mark_read(item_id)`, `mark_feed_read(feed_id)`, `count_unread(feed_id)`
- [ ] 단위 테스트: 스키마 생성, UNIQUE 제약(`feed_id, guid_hash`) 중복 삽입 시 무시/무해 동작 확인

## M2. 피드 파싱 & "피드 추가" 플로우

- [ ] `feed-rs` 연동 래퍼 작성 — RSS/Atom/JSON Feed 입력 → 공통 내부 모델 변환
- [ ] GUID 해시 로직: `guid` 있으면 해시, 없으면 정규화된 `link`를 해시
- [ ] Tauri 커맨드 `add_feed(url)` 구현
  - [ ] URL fetch → `Content-Type` + 바디로 포맷 판별(feed-rs 자동 감지)
  - [ ] 파싱 성공 시 `feeds` insert + 초기 아이템 일괄 insert
  - [ ] 파싱/네트워크 실패 시 에러를 프론트엔드에 그대로 반환
- [ ] 단위 테스트: RSS 2.0 / Atom / JSON Feed 각 샘플 파일 파싱 결과 검증
- [ ] 수동 테스트: 실제 블로그 RSS 3~5개로 등록 확인

## M3. 조건부 요청 & Fetch Worker (효율화 핵심)

- [ ] `FeedMeta` 구조체(`etag`, `last_modified`, `interval_secs`, `next_fetch_at`) 및 DB 컬럼 매핑
- [ ] 조건부 요청 적용: 요청 시 `If-None-Match` / `If-Modified-Since` 헤더 세팅
- [ ] 응답 처리 분기
  - [ ] `304 Not Modified` → 바디 다운로드/파싱 없이 즉시 종료, `next_fetch_at`만 갱신
  - [ ] `200 OK` → 파싱 → 신규 아이템만 insert(UNIQUE 충돌 무시) → `etag`/`last_modified`/`next_fetch_at` 갱신
- [ ] 개별 요청 timeout 설정(예: 10초)
- [ ] 통합 테스트: `wiremock`으로 304/200/timeout 3가지 시나리오 검증

## M4. 동시성 제한 & 폴링 스케줄러

- [ ] `tokio::sync::Semaphore`로 동시 fetch 수 제한(기본 4)
- [ ] `JoinSet` 기반 fetch worker 실행 + 에러 로깅
- [ ] Poll Scheduler: `next_fetch_at <= now`인 피드 조회 → 세마포어 제한 하에 워커 디스패치
- [ ] 유휴 시 완전 슬립: `tokio::time::sleep_until(next_wake)` — busy-loop/고정 interval 금지
- [ ] 앱 시작 시 스케줄러 기동, 백그라운드 태스크로 상시 실행
- [ ] 수동 테스트: 피드 10개 이상 등록 후 유휴 CPU 확인(활동 모니터)

## M5. 에러 처리 & 재시도

- [ ] 피드별 `last_error` 컬럼 기록
- [ ] 지수 백오프 재시도 큐 (1분 → 5분 → 30분, 상한은 피드 기본 interval 이내)
- [ ] 네트워크 오류/타임아웃이 전체 폴링 사이클을 막지 않는지 확인 (한 피드 실패가 다른 피드에 영향 없음)
- [ ] 파싱 오류 시 해당 사이클만 스킵, 다음 폴링에서 재시도
- [ ] 단위 테스트: 백오프 간격 계산 함수

## M6. Frontend ↔ Backend 통신 (이벤트 기반)

- [ ] Tauri 커맨드 노출: `add_feed`, `list_feeds`, `list_items`, `mark_read`, `mark_feed_read`, `refresh_feed(feed_id)`
- [ ] 신규 아이템 발생 시 `emit("feed-updated", { feedId, newItemCount })`
- [ ] 프론트엔드 이벤트 리스너: 이벤트 수신 시에만 해당 구간 재조회 (프론트가 백엔드를 폴링하지 않음)
- [ ] 수동 테스트: 폴링 중 새 글 발생 시 UI가 타이머 없이 업데이트되는지 확인

## M7. UI 구현

### 디자인 원칙 (NetNewsWire / Reeder 스타일)

- 네이티브 macOS 느낌: 시스템 폰트(San Francisco) 사용, macOS 기본 window chrome/타이틀바 활용, 커스텀 크롬 지양
- 미니멀한 3단 리스트형 레이아웃 — 장식적 요소(그라디언트, 그림자, 불필요한 아이콘) 배제, 콘텐츠 밀도 우선
- 여백은 넉넉하게, 하지만 Notion/Linear식 넓은 여백이 아니라 "리스트 가독성" 기준 — 한 화면에 최대한 많은 아이템이 스캔 가능해야 함
- 색상은 중립(그레이 스케일) 기반 + 강조색 1개만 사용(안읽음 표시, 선택 상태 등). 브랜드 컬러를 UI 전반에 흩뿌리지 않음
- 타이포그래피 계층은 2~3단계로 제한: 제목(semibold) / 메타정보·피드명·시각(작은 글씨, 약한 명암) / 본문
- 안읽음/읽음 구분은 색상보다 명암·굵기·점(dot) 표시로 — 과한 배지/카운트 뱃지 지양
- 다크모드는 v1 범위 밖(13장)이지만, 색상을 하드코딩하지 않고 시스템 라이트/다크 전환에 자연스럽게 대응할 수 있는 구조(CSS 변수/토큰화)로 작성 — 나중에 다크모드 추가 시 레이아웃 변경 없이 색상 토큰만 교체

- [ ] 3단 레이아웃: 피드 목록 / 통합 타임라인 / 상세 보기
- [ ] 피드 목록: 전체 + 피드별 안읽음 카운트(`COUNT(*) WHERE is_read = 0`)
- [ ] 타임라인: `published_at DESC` 정렬, 안읽음(bold/점)·읽음(흐리게) 스타일 구분
- [ ] 가상 스크롤(virtual list) 적용 — 수천 건 렌더링 시 DOM 노드 제한
- [ ] 페이지네이션/무한 스크롤로 `list_items(limit, offset)` 호출
- [ ] 아이템 열람 시 `mark_read` 호출 + 옵티미스틱 UI 업데이트
- [ ] 피드 추가 UI (URL 입력 폼, 에러 메시지 표시)
- [ ] 상세 보기: 본문/요약 렌더링

## M8. 보안

- [ ] 피드 HTML 콘텐츠 sanitize (허용 태그 화이트리스트) 적용 후 렌더링
- [ ] Tauri CSP 설정 — webview의 임의 origin 요청/리소스 로드 차단
- [ ] 모든 외부 fetch가 Rust 백엔드를 경유하는지 확인(프론트엔드 직접 fetch 금지)

## M9. 테스트 정리

- [ ] Rust 유닛 테스트 커버리지 확인: 파싱 래퍼, 중복 제거, 조건부 요청 헤더, 백오프 계산
- [ ] 통합 테스트: mock HTTP 서버 기반 304/200/timeout
- [ ] 프론트엔드 컴포넌트 테스트: 읽음/안읽음 렌더링, 정렬, Tauri 커맨드 mock

## M10. 성능 검증 (출시 기준)

- [ ] 유휴 메모리: 피드 20개·아이템 5,000건 기준 80MB 이하
- [ ] 유휴 CPU: 폴링 사이 구간 0%대
- [ ] 폴링 1회(피드 20개, 무변경) 소요: 수백 ms 이내(대부분 304)
- [ ] 콜드 스타트: 1초 이내 첫 화면 표시
- [ ] 위 항목이 목표치를 벗어나면 원인 분석 후 M3~M4로 롤백해 재검토

## M11. 빌드/배포

- [ ] `tauri build`로 `.app`/`.dmg` 생성 확인
- [ ] Developer ID 서명 + notarization (Gatekeeper 통과)
- [ ] (선택) Tauri Updater 플러그인 적용 여부 결정 — v1 이후로 미뤄도 무방

---

## 범위 밖 (v1 아님, 착수 금지)

ARCHITECTURE.md 13장 로드맵 항목 — 아래는 이번 계획서에 포함하지 않는다:

- 적응형 폴링 간격 자동 조정
- OPML 가져오기/내보내기
- 전역 검색, 태그/폴더 분류
- 다크모드
- 메뉴바 상시 아이콘 + launchd 기반 완전 백그라운드 폴링
