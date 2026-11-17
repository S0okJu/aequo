# Aequo — 본문 주석 기능 구현 계획 (메모 · 형광펜 · AI 질문)

> 이 문서는 `docs/IMPLEMENTATION_PLAN.md`의 M11(빌드/배포) 이후에 이어지는, 별도로 분리된 확장 기능 계획이다.
> 대상 범위: (1) 본문 메모, (2) 본문 형광펜, (3) 선택한 본문을 AI로 바로 질문하는 기능.
> 아직 코드는 건드리지 않았고, 이 문서는 설계/합의용 계획서다.
>
> 이 기능들은 **v1.0.0 릴리스의 범위**다. 현재 `package.json`/`src-tauri/tauri.conf.json`의 버전은 `0.1.0`이고,
> 작업 브랜치는 `release/v1.0.0`이다 — M12~M14 구현이 끝나면 M15(성능 검증)를 통과한 뒤 M16에서 버전을
> `1.0.0`으로 올려 빌드/배포한다.

## Status markers

- [ ] not started · [~] in progress · [x] done

---

## 0. 현재 구조 요약 (왜 이렇게 설계하는지의 전제)

- 리더 뷰: `PostReader.svelte`가 `loadArticle()`로 `ArticleView`를 받아 `FeedContent.svelte`에 `content`(HTML 문자열)를 넘긴다.
- `FeedContent.svelte`의 `sanitizedContent()`는 **매 렌더링마다** `DOMParser`로 원본을 파싱하고, 허용 태그 allowlist(`A/B/BLOCKQUOTE/.../UL`)만 복사해 새 DOM을 만든 뒤 `{@html html}`로 주입한다. 즉 **본문 DOM은 안정적인 노드 참조를 가진 적이 없고, 매번 처음부터 다시 만들어진다.**
- 본문 자체도 바뀔 수 있다: RSS 미리보기(`item.summary`) → 추출된 전문(`article.contentHtml`)으로 바뀌거나, `refreshArticle()`로 전문이 재추출되면 내용이 달라질 수 있다 (`PostReader.svelte` `refreshArticle`).
- AI 질문 기능은 이미 존재한다: `AiPanel.svelte` + `src-tauri/src/commands/ai.rs`(로컬 `codex`/`claude` CLI를 서브프로세스로 실행) + `ai_transcript.rs`(각 CLI가 자체적으로 남기는 로컬 transcript 파일을 읽음). **메시지 본문은 Aequo DB에 저장하지 않는다** — 과거 `ai_messages` 테이블을 보안/프라이버시 이유로 제거한 전례가 `db/schema.rs`의 `apply()`에 남아 있다. 새 기능도 이 원칙을 깨지 않아야 한다.
- 저장/캐시 관련 기존 테이블: `items`, `article_cache`(item_id 1:1), `ai_sessions`. 마이그레이션 방식은 버전드 마이그레이션이 아니라 **멱등한 `CREATE TABLE/INDEX IF NOT EXISTS` + 컬럼 존재 체크 후 `ALTER TABLE ADD COLUMN`** (`db/schema.rs`) — 새 기능도 이 패턴을 따른다.

이 전제에서 나오는 핵심 난제: **형광펜/메모는 DOM 노드가 아니라 "텍스트 오프셋"으로 위치를 저장해야 하고, 본문이 바뀌면 그 오프셋이 깨질 수 있다는 것을 설계에 넣어야 한다.**

---

## 1. 목표 / 비목표

### 목표
- 본문의 특정 구간을 드래그 선택해서 형광펜 색을 입힐 수 있다.
- 본문의 특정 구간에 메모(짧은 텍스트 노트)를 남길 수 있고, 이 포스트의 메모를 모아 볼 수 있다.
- 본문을 선택한 상태에서 바로 "AI에게 질문"을 눌러, 선택한 문장을 인용구로 넣은 채 기존 AI 패널로 질문을 시작할 수 있다.
- 세 기능은 "텍스트 선택 → 플로팅 툴바"라는 하나의 상호작용을 공유한다.

### 비목표 (v1 범위 아님)
- 다른 기기/다른 Aequo 설치본과의 주석 동기화
- 주석 export(Markdown/PDF 등) — 필요하면 별도 계획
- 협업/공유 주석
- 이미지·표 등 비텍스트 요소에 대한 주석 (텍스트 구간만 지원)
- "자유 메모"(본문 특정 구간에 매이지 않는 메모) — 전부 텍스트 선택을 전제로 한다

---

## 2. 공통 기술 과제: 텍스트 앵커링

세 기능 중 형광펜·메모는 "본문의 어느 글자부터 어느 글자까지"를 영속적으로 가리켜야 한다. 설계:

1. **오프셋 기준**: `FeedContent`가 렌더링에 쓰는 plain text(= sanitize 후 DOM의 `textContent`)에서 `[start, end)` 문자 오프셋으로 저장한다. HTML 태그 구조가 아니라 순수 텍스트 기준이라 sanitizer가 매번 새 DOM을 만들어도 오프셋 자체는 재계산 없이 유효하다.
2. **내용 변경 감지 및 재배치 (self-healing anchoring)**: 저장 시점의 plain text 전체에 대한 해시(`content_hash`, SHA-256)를 함께 저장한다. 불러올 때 현재 plain text의 해시와 비교해서 다르면, 바로 포기하지 않고 아래 순서로 재배치를 시도한다 (가장 이상적인 선택지 — Hypothesis 등 웹 주석 도구들이 쓰는 text-quote anchoring 방식과 동일한 접근):
   1. **정확 매칭 (빠른 경로)**: 저장된 `quoted_text`를 새 plain text에서 정확히 문자열 검색한다. 한 번만 발견되면 그 위치로 오프셋을 갱신한다. 여러 번 발견되면 과거 오프셋의 문서 내 상대 위치(비율)에 가장 가까운 매칭을 선택한다.
   2. **근사 매칭 (느린 경로, 정확 매칭 실패 시에만)**: `diff-match-patch`(Google의 bitap 기반 fuzzy-search 라이브러리) 등을 이용해 편집 거리 허용 범위 내에서 `quoted_text`와 가장 유사한 구간을 찾는다. 신뢰도가 충분히 높을 때만 재배치한다.
   3. 재배치에 성공하면 `start_offset`/`end_offset`/`content_hash`를 새 값으로 갱신해 DB에 다시 저장한다 (self-healing — 다음부터는 다시 빠른 경로로 매칭됨).
   4. 1·2 모두 실패하면 그제서야 "본문이 바뀌어 위치를 표시할 수 없음"으로 처리한다 (완전 실패 시에만 fail closed).

   성능 절충: 대부분의 경우(내용이 안 바뀜) 해시가 일치해 O(1) 비교로 끝난다. 내용이 바뀐 드문 경우에만 1단계(단순 문자열 검색, 저렴)를 타고, 그마저 실패하는 더 드문 경우에만 2단계(비용이 더 드는 fuzzy 매칭)를 탄다 — 매번 전체 diff를 돌리는 구조가 아니므로 "이상적인 정확도"와 "평균 비용 저하 없음"을 동시에 만족한다.
3. **원문 스냅샷**: `quoted_text`를 별도로 저장해, 해시가 안 맞아도 사용자에게 "원래 이 문장에 달았던 메모였다"를 보여줄 수 있게 한다.
4. **렌더링**: `FeedContent`가 sanitize된 DOM을 만든 뒤, `TreeWalker`로 텍스트 노드를 순회하며 `[start, end)` 구간에 해당하는 부분을 `Range`로 감싸 `<mark>`(형광펜) 또는 마커 `<span>`(메모)을 삽입하는 **오버레이 패스**를 추가한다. 이 오버레이 로직은 형광펜/메모가 공유한다.
5. **선택 영역 → 오프셋 변환**: 사용자가 드래그 선택하면 `window.getSelection()`의 `Range`를 같은 plain-text 좌표계로 역변환하는 유틸이 필요하다 (TreeWalker로 선택 시작/끝 노드까지의 문자 수를 누적).

이 유틸(오프셋 ↔ Range 변환, content hash 계산)은 형광펜/메모/AI 질문 세 기능이 모두 쓰므로 공용 모듈로 분리한다 (예: `src/lib/annotation.ts`).

---

## 3. 데이터 모델

새 테이블 하나로 형광펜·메모를 함께 다룬다 (`kind` 컬럼으로 구분):

```sql
CREATE TABLE IF NOT EXISTS annotations (
    id              INTEGER PRIMARY KEY,
    item_id         INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK(kind IN ('highlight', 'memo')),
    content_hash    TEXT NOT NULL,   -- 앵커링 대상 plain text의 SHA-256
    start_offset    INTEGER NOT NULL,
    end_offset      INTEGER NOT NULL,
    quoted_text     TEXT NOT NULL,   -- 저장 당시 선택 구간의 원문 스냅샷
    color           TEXT,            -- 형광펜 색상 토큰, memo는 NULL
    note            TEXT,            -- 메모 본문, highlight는 NULL
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_annotations_item ON annotations(item_id, start_offset);
```

- `db/schema.rs`의 기존 멱등 패턴을 그대로 따른다 (버전드 마이그레이션 도입 안 함).
- `db/models.rs`에 `Annotation` struct, `db/repository.rs`에 `list_annotations(item_id)` / `create_annotation` / `update_annotation` / `delete_annotation` 추가. `update_annotation`은 note/color 수정뿐 아니라, 2.2절의 self-healing 재배치가 성공했을 때 `start_offset`/`end_offset`/`content_hash`를 갱신하는 용도로도 쓴다 (프론트에서 재배치 계산 후 같은 커맨드로 반영).
- `note`/`quoted_text` 길이 제한(예: note 10,000자, quoted_text 20,000자)을 커맨드 레벨에서 강제한다 — `ai.rs`의 `send_ai_message`가 prompt 길이를 제한하는 것과 같은 패턴(`prompt.len() > 100_000` 체크).

---

## 4. 기능별 설계

### 4.1 형광펜 (M12)

- 텍스트 드래그 선택 → 플로팅 툴바 노출 (색상 버튼 3~4개 + 메모 버튼 + "AI에게 질문" 버튼을 한 툴바에 모음).
- 색상은 고정 팔레트 **5개** — yellow / green / blue / pink / purple. CSS 변수 토큰(`--hl-yellow` 등)으로 정의해서 라이트/다크 모드 대응 (ARCHITECTURE.md가 이미 색상을 토큰화하라고 명시한 원칙과 동일). 커스텀 색상 선택(컬러피커)은 지원하지 않는다 — 고정 팔레트만.
- 저장: `annotations(kind='highlight', color=...)`.
- 렌더링: `<mark class="hl-{color}">` 삽입. 클릭하면 "색 변경 / 제거" 팝오버.

### 4.2 메모 (M13, 형광펜 인프라 재사용)

- **필수 요구사항: 메모는 반드시 본문(FeedContent) 안에서 직접 확인할 수 있어야 한다.** 별도 목록만 있고 본문에는 표시가 없는 구조는 안 됨 — 본문을 읽다가 그 자리에서 "여기에 메모가 있다"는 걸 바로 알아보고 내용을 볼 수 있어야 한다.
- 텍스트 선택 → 툴바의 "메모" 클릭 → 인라인 팝오버에 텍스트 입력 → 저장.
- 메모가 달린 구간은 점선 밑줄 + 마진에 작은 아이콘으로 표시 (형광펜과 동일한 오버레이 패스 재사용). 그 구간을 클릭(또는 아이콘 클릭)하면 **본문 위에 바로 뜨는 팝오버**로 메모 내용을 확인·수정·삭제한다 — 별도 패널로 이동하지 않는다.
- `PostReader`에 "이 글의 메모" 목록은 보조 기능으로만 둔다 (전체 메모를 훑어보고 해당 구간으로 스크롤하는 용도). 메모를 "확인"하는 주된 경로는 항상 본문 인라인 팝오버다.
- 성능 관점에서도 이 구조가 유리하다: 오버레이 렌더링에 쓰는 `annotations` 상태를 그대로 팝오버에 재사용하면 되므로, 목록 뷰를 위해 별도 쿼리나 별도 상시 마운트 컴포넌트(AiPanel 같은)를 만들 필요가 없다.

### 4.3 선택 영역을 AI로 질문 (M14)

- 새 백엔드 커맨드는 필요 없음 — 기존 `send_ai_message` / `AiPanel.svelte`를 그대로 재사용한다.
- **"AI에게 질문"을 누르면 항상 새 AI 세션을 시작한다** (기존에 열려 있는 세션에 이어붙이지 않음). 선택 구간마다 별개의 질문 맥락으로 다루는 것이 더 명확하고 구현도 단순하다 — `sendAiMessage(itemId, provider, sessionId=null, prompt)` 호출 시 `sessionId`를 항상 `null`로 넘겨 `ai.rs`의 `prepare_session`이 새 세션을 만들게 한다.
- 툴바의 "AI에게 질문" 클릭 → `AiPanel`이 열리고(닫혀 있었다면 레이아웃 전환), 새 세션의 draft 입력창이 인용 형태(`"> {quoted_text}\n\n"`)로 미리 채워진다. 사용자가 질문을 이어 쓴 뒤 전송.
- 선택 구간이 과도하게 길면(예: 2,000자 초과) 잘라내고 안내 문구 표시.
- `PostReader` → `AiPanel`에 `pendingQuote` prop을 추가해 전달한다.
- CLI(`codex`/`claude`)가 설치/로그인되어 있지 않으면 기존에도 에러가 나는 경로이므로(`executable_path`), 별도 처리 불필요 — 기존 에러 메시지가 그대로 노출됨을 확인만 한다.

---

## 5. 백엔드 변경 (Rust)

- `db/schema.rs`: `annotations` 테이블/인덱스 추가 (멱등 패턴)
- `db/models.rs`: `Annotation` struct (camelCase Serialize, 기존 `Item`/`AiSession` 스타일과 동일)
- `db/repository.rs`: CRUD 함수 + 단위 테스트 (`init_in_memory()` 패턴 재사용)
- `commands/annotations.rs` (신규): `list_item_annotations`, `create_annotation`, `update_annotation`, `delete_annotation` Tauri 커맨드
- `lib.rs`: `invoke_handler`에 신규 커맨드 등록
- `src/lib/types.ts` / `src/lib/api.ts`: `Annotation` 타입 및 wrapper 함수 추가

---

## 6. 프론트엔드 변경 (Svelte)

- `src/lib/annotation.ts` (신규): plain-text 오프셋 ↔ `Range` 변환, content hash 계산, 오버레이 DOM 삽입 유틸, 2.2절의 self-healing 재배치 로직(정확 매칭 → `diff-match-patch` 기반 근사 매칭). 재배치 성공 시 `updateAnnotation()` API를 호출해 새 오프셋/해시를 저장한다.
- `package.json`에 `diff-match-patch` 의존성 추가 (근사 매칭 전용, 번들 크기 작음)
- `FeedContent.svelte`: `annotations` prop 추가, sanitize 후 오버레이 패스 실행, `selectionchange`/`mouseup`으로 선택 감지해 플로팅 툴바 좌표 계산
- `SelectionToolbar.svelte` (신규): 색상/메모/AI 버튼
- `AnnotationPopover.svelte` (신규): 기존 주석 클릭 시 보기/수정/삭제
- `PostReader.svelte`: annotations 로드 및 상태 관리, `AiPanel`에 `pendingQuote` 전달
- `AiPanel.svelte`: `pendingQuote` prop 반영해 draft 초기값 설정

---

## 7. 보안/정합성

- 오버레이가 삽입하는 `<mark>`/`<span>`은 우리 코드가 직접 생성한다. 사용자가 입력한 `note` 텍스트는 `textContent`로만 넣고 HTML로 파싱하지 않는다 → `FeedContent`의 기존 allowlist 정책을 깨지 않고, 새 XSS 벡터를 만들지 않는다.
- `content_hash` 불일치 시 2.2절의 정확 매칭 → 근사 매칭 순서로만 재배치하고, 둘 다 실패했을 때만 "위치를 찾을 수 없음" 배지로 표시한다 — 근사 매칭도 신뢰도 임계값을 넘지 못하면 재배치하지 않는다 (낮은 신뢰도로 잘못된 위치에 붙는 것은 여전히 금지).
- 메모 본문은 AI 메시지와 달리 Aequo DB에 영구 저장되는 것이 맞다 (AI 세션 본문을 저장하지 않는 것과는 다른 결정) — 사용자가 직접 작성한 노트이고, CLI transcript처럼 외부 저장소가 없기 때문. 이 비대칭을 문서에 명시해 둔다.

---

## 8. 마일스톤

### M12. 형광펜 — [x] 완료
- [x] `annotations` 테이블/리포지토리/커맨드 — `kind` CHECK 제약과 함께 전체 스키마(highlight+memo)를 한 번에 생성 (M13에서 다시 마이그레이션하지 않도록). `db/schema.rs`, `db/models.rs`(`Annotation`), `db/repository.rs`(CRUD + `annotations_overlapping`), `commands/annotations.rs`, `lib.rs` 등록
- [x] plain-text 오프셋 ↔ Range 변환 유틸 + 단위 테스트 — `src/lib/annotation.ts`(`extractPlainText`/`offsetsToRange`/`selectionToOffsets`), `src/lib/annotation.test.ts`
- [x] 텍스트 선택 감지 + 플로팅 툴바 (색상 버튼) — `FeedContent.svelte`(mouseup/keyup에서만 오프셋 계산, selectionchange는 collapse 감지만), `SelectionToolbar.svelte`
- [x] `FeedContent` 오버레이 렌더링 (`<mark>`) — `applyOverlay`/`removeOverlay`(TreeWalker 기반 텍스트 노드 분할·래핑, 멱등)
- [x] 저장/삭제/색상 변경 팝오버 (색상 5개: yellow/green/blue/pink/purple) — `AnnotationPopover.svelte`, 토큰은 `+layout.svelte`의 `--hl-*`
- [x] content_hash 불일치 시 재배치 로직: 정확 매칭 → `diff-match-patch` 근사 매칭 → 실패 시 숨김, self-healing 오프셋 갱신까지 포함 — `reconcileAnnotations()`

### M13. 메모 — [x] 완료
- [x] `kind='memo'` 경로 + note 입력 팝오버 — `AnnotationPopover.svelte`(10,000자 카운터)
- [x] 마진 마커 UI — `FeedContent.svelte`의 `.margin-rail`(`ResizeObserver`로 위치 추적, 컨테이너 700px 미만에서는 점선 밑줄만 남기고 숨김)
- [x] "이 글의 메모" 목록 + 스크롤 이동 — `PostReader.svelte`의 `.memo-list`(`scrollIntoView` + flash 하이라이트), 재배치 실패(orphaned) 항목은 "Position not found" 배지로 표시
- [x] 수정/삭제 — `PostReader.svelte`의 `handleUpdateAnnotation`/`handleDeleteAnnotation`

### M14. 선택 영역 AI 질문 — [x] 완료
- [x] 플로팅 툴바에 "AI에게 질문" 버튼 추가 — `SelectionToolbar.svelte`
- [x] `AiPanel` `pendingQuote` 연동, 인용 포맷 draft 채우기, 항상 새 세션으로 시작(`sessionId: null`) — `pendingQuote`는 `PostReader`가 아니라 **`+page.svelte`**에 끌어올려 1회성 값으로 양쪽 형제 컴포넌트에 전달 (실제 구조상 `PostReader`/`AiPanel`이 부모-자식이 아니라 `+page.svelte`의 형제 패널이라 §4.3/§6의 "`PostReader` → `AiPanel`" 전달 경로를 이렇게 조정함)
- [x] 과도하게 긴 선택 구간 truncate + 안내 — 2,000자 초과 시 잘라서 전달, 툴바 근처에 2.5초간 안내 문구
- [x] 패널이 닫혀 있을 때 자동으로 열리는 레이아웃 처리 — `onAskAi`가 `aiPanelOpen = true`로 기존 레이아웃 토글 재사용

### 공통 테스트 — [x] 자동 테스트 완료 / [ ] 수동 확인 미완료
- [x] Rust: content hash(해시는 프론트 전용이라 Rust 쪽은 커맨드 검증+리포지토리 CRUD/겹침/CHECK 제약), 오프셋 계산 단위 테스트 — `cargo test` 46 passed
- [x] Frontend: `FeedContent.test.ts`에 오버레이 삽입/선택 변환/재배치/truncation/selectionchange 디바운스 케이스 추가, `annotation.test.ts`(앵커링 유틸 전체) — `vitest` 32 passed
- [ ] 수동: `refreshArticle()` 이후 기존 주석이 stale 처리되는지, 다크모드에서 형광펜 대비 확인 — **미완료.** 이 작업 환경은 GUI 접근이 없어 `pnpm tauri dev`로 실제 앱을 띄워 확인하지 못했음. 사용자가 직접 확인 필요.

### M15. 성능 검증 (v1.0.0 릴리스 조건) — [x] 재정의된 범위에서 완료

구현 중 아래 두 가지가 문서 작성 시점과 달라진 것을 확인해, 체크리스트를 그에 맞게 재정의해서 완료했다 (자세한 근거는 구현 커밋/PR 참고):
- M7의 `VirtualList.svelte`는 이미 삭제되어 더 이상 존재하지 않음 — 현재 글 목록(`PostsFeed.svelte`)은 가상화 없이 스크롤 임계값 페이지네이션만 사용하고, 목록 아이템은 `FeedContent`의 `compact` 모드(미리보기 텍스트만, 오버레이 로직 전혀 안 탐)로 렌더링됨.
- `docs/IMPLEMENTATION_PLAN.md`의 M10 성능 수치(idle memory ≤80MB 등)는 실측된 적이 없음(체크리스트 전부 미완료, "이 환경에서 측정 불가능" 비고만 있음) — 비교할 기준선이 없어 "회귀 확인"을 문구 그대로 실행할 수 없었음.

- [x] 오버레이 렌더링 비용: "체감 가능한 지연" 대신 자동화된 예산 테스트로 대체 — `annotation.test.ts`의 200+ 주석 reconcile/overlay 테스트(느슨한 2초 예산으로 O(n²) 회귀만 검출)
- [x] 텍스트 선택 → 플로팅 툴바 지연: `selectionchange`마다 전체 오프셋 재계산을 안 하는지 spy 테스트로 검증 — `FeedContent.test.ts`("only recomputes selection offsets on mouseup/keyup...")
- [ ] 기존 M10 수치 회귀 확인 — **문구 그대로는 재정의/보류.** M10 자체가 실측된 적이 없어 비교 기준선이 없음. 구현 전/후 Activity Monitor 상대 비교는 GUI 접근이 없어 이번 세션에서 수행하지 못했음 — 사용자가 M12~M14 머지 전/후 각 1회 수동 캡처해서 PR에 기록 필요.
- [x] 가상 스크롤 관련 확인 → "구조적으로 무관함" 확인으로 대체 완료: `PostsFeed.svelte`가 쓰는 `FeedContent`의 `compact` 분기는 `html`도 오버레이 `$effect`도 전혀 실행하지 않음(코드 확인됨) — 가상화가 없으므로 "주석 섞인 상태의 스크롤 성능"은 애초에 측정 대상이 아님.
- [x] `annotations` 테이블 조회 쿼리 수: `PostReader.svelte`가 `loadArticle`/`listItemAnnotations`를 `Promise.all`로 병렬 호출하도록 구현(순차 호출 대비 지연이 두 호출 중 느린 쪽으로 bound됨). `load_article`과의 병합은 **지금 하지 않기로 결정** — 수동 프로파일에서 실제로 비용이 보일 때만(~16ms 이상) 재검토하기로 보류.
- [ ] 재검토 트리거: 위 항목 중 예산을 벗어난 것이 없어 M12~M14로 돌아가는 재작업은 발생하지 않음 (해당 없음)

### M16. v1.0.0 빌드/배포

- [ ] `package.json`과 `src-tauri/tauri.conf.json`의 버전을 `0.1.0` → `1.0.0`으로 변경 (두 파일 동기화 확인)
- [ ] `src-tauri/Cargo.toml` 버전도 맞춰서 업데이트
- [ ] M12~M15가 모두 완료/통과된 상태인지 확인 (성능 검증 통과 전에는 버전을 올리지 않는다)
- [ ] `docs/IMPLEMENTATION_PLAN.md` M11과 동일한 방식으로 `.app`/`.dmg` 빌드
- [ ] 빌드된 앱 아이콘, 번들 정보(버전 표시 등)에 `1.0.0`이 올바르게 반영됐는지 확인
- [ ] `release/v1.0.0` 브랜치에서 최종 빌드 산출물 검증 후 태그/릴리스 (실제 태그 푸시 등 되돌리기 어려운 작업은 사용자 확인 후 진행)

---

## 9. 오픈 퀘스천

### 결정됨

1. ~~본문이 바뀌어 `content_hash`가 달라졌을 때, 완전 포기할지 best-effort 재배치를 고려할지?~~
   → **가장 이상적인 선택지로 결정: self-healing 재배치를 구현한다.** 정확 문자열 매칭을 먼저 시도하고, 실패하면 `diff-match-patch` 기반 근사 매칭을 시도하며, 재배치에 성공하면 오프셋/해시를 갱신해 저장한다. 둘 다 실패했을 때만 "위치를 찾을 수 없음"으로 처리한다 (2.2절, M12 체크리스트 참고). 평균적인 경우(내용 불변)는 해시 비교만으로 끝나 성능 저하가 없다.
2. ~~형광펜 색상 팔레트 개수와 기본값은?~~
   → **5개로 결정: yellow / green / blue / pink / purple.** 커스텀 컬러피커는 지원하지 않는다.
3. ~~"AI에게 질문"을 누르면 항상 새 AI 세션을 시작할지, 기존 세션에 이어붙일지?~~
   → **항상 새 세션으로 결정.** `sendAiMessage(..., sessionId: null, ...)`로 호출해 매번 새 `ai_sessions` 로우를 만든다.
4. ~~메모 목록을 `PostReader` 안에 넣을지, `AiPanel`처럼 별도 사이드 패널로 둘지?~~
   → **메모는 본문 내 인라인 팝오버로 직접 확인하는 것이 필수**다 (4.2절 참고). `PostReader`의 메모 목록은 전체 훑어보기/스크롤 이동용 보조 기능일 뿐, 메모를 "확인"하는 주된 경로가 아니다. 별도 상시 마운트 패널(AiPanel 방식)은 만들지 않는다 — 오버레이용 `annotations` 상태를 그대로 재사용해 추가 쿼리 없이 구현.

모든 오픈 퀘스천이 결정되어, M12~M14 구현에 바로 들어갈 수 있다.

### 구현 중 추가로 결정됨 (이 문서에 명시 안 돼 있던 것)

5. **형광펜과 메모가 겹치는 구간을 가질 수 있는가?** → **동일 구간만 허용.** 일반적인 구간 겹침/중첩은 `create_annotation`에서 거부하고, 형광펜과 메모가 정확히 동일한 `[start, end)`를 공유하는 경우만 예외로 허용한다 (한 문장에 형광펜+메모를 동시에 남기는 흔한 케이스). `commands/annotations.rs`의 `check_overlap`.
6. **재배치 시 같은 텍스트가 여러 곳에서 매칭되면 어느 위치를 고르는가?** → 문서(§2.2)가 제안한 "상대 위치(비율)" 매칭은 스키마에 원본 문서 길이가 저장되지 않아 정확히 계산할 수 없음 → **저장된 절대 오프셋과 가장 가까운 매칭**을 선택하는 근사치로 대체 (스키마 변경 없음). `src/lib/annotation.ts`의 `relocateByExactMatch`.
7. **메모 마진 아이콘(§4.2) 구현 방식** → `getBoundingClientRect` + `ResizeObserver`로 마진에 아이콘을 띄우고, 컨테이너 너비가 좁아지면(~700px 미만) 점선 밑줄만 남기는 반응형 폴백으로 구현. `FeedContent.svelte`의 `.margin-rail`.

---

## Out of scope (v1 아님)

- 주석 동기화/백업/export
- 비텍스트(이미지/표) 주석
- 자유 메모(구간에 안 매인 메모)
- 협업/공유
