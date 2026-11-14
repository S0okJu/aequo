# 개발 워크플로우 가이드라인

이 문서는 Aequo v1(`docs/IMPLEMENTATION_PLAN.md`) 구현을 진행할 때 따르는 브랜치/커밋 규칙이다.

## 브랜치 전략

- v1 기능 구현은 모두 **`release`** 브랜치에서 진행한다. `main`에는 직접 커밋하지 않는다.
- `release`는 `main`에서 분기한다.
- v1(M0~M11) 전체가 끝나고 `docs/IMPLEMENTATION_PLAN.md`의 체크리스트가 모두 `[x]`가 되면, `release` → `main` PR을 올려 리뷰 후 머지한다.
- `main`에는 문서(`docs/`, `CONTRIBUTING.md`, `README.md` 등)만 올라가며, 애플리케이션 코드는 `release`에서 머지될 때 합류한다.

## 커밋 단위

- **`docs/IMPLEMENTATION_PLAN.md`의 체크리스트 항목 1개 = 커밋 1개.**
- 한 커밋에 여러 체크리스트 항목을 묶지 않는다. 항목이 너무 작아 보여도(예: "인덱스 생성") 별도 커밋으로 남긴다 — 리뷰·되돌리기(revert)가 쉬워짐.
- 커밋을 완료하면 같은 커밋(또는 바로 다음 커밋)에서 해당 체크리스트 항목을 `[ ]` → `[x]`로 갱신한다. 코드와 계획서 상태가 항상 일치해야 한다.
- 마일스톤(M0~M11) 순서를 따른다. 다만 M3(백엔드 fetch)와 M7(UI 뼈대)처럼 계획서에 병행 가능하다고 표시된 경우는 순서를 바꿔도 된다.
- 13장 로드맵(범위 밖) 항목은 이 브랜치에서 커밋하지 않는다.

## 커밋 메시지 (Conventional Commits)

형식: `<type>(<scope>): <설명> (M<번호>)`

- **type**: `feat` / `fix` / `test` / `docs` / `chore` / `refactor` 중 하나
- **scope**: 해당 작업이 속한 영역 (`db`, `fetch`, `scheduler`, `ui`, `commands` 등). 계획서의 모듈 구조(`src-tauri/src/{commands,db,fetch,scheduler}`)를 그대로 쓴다.
- **설명**: 한글/영문 무관, 명령형("추가한다"가 아니라 "추가")
- 끝에 해당 마일스톤 번호를 `(M숫자)`로 표기해 계획서와 추적 가능하게 한다.

예시:
```
feat(db): feeds/items 테이블 스키마 작성 (M1)
feat(db): idx_items_published, idx_items_unread 인덱스 추가 (M1)
feat(fetch): ETag/Last-Modified 조건부 요청 적용 (M3)
test(fetch): wiremock으로 304/200/timeout 시나리오 검증 (M3)
fix(scheduler): 유휴 시 busy-loop 대신 sleep_until 사용 (M4)
docs: M1 체크리스트 완료 표시
```

## PR 전 체크

- [ ] 관련 체크리스트 항목이 전부 `[x]`인가
- [ ] 해당 마일스톤의 단위/통합 테스트가 통과하는가 (`cargo test`, 프론트엔드 테스트)
- [ ] `cargo clippy`, lint 경고 없는가
- [ ] 범위 밖(13장 로드맵) 기능이 섞여 들어가지 않았는가
