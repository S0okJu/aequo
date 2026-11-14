# Development Workflow Guideline

This document defines the branch/commit rules to follow while implementing Aequo v1 (`docs/IMPLEMENTATION_PLAN.md`).

## Branch Strategy

- All v1 feature work happens on the **`release/v1.0.0`** branch. Do not commit directly to `main`.
- `release/v1.0.0` branches off of `main`.
- Once all of v1 (M0–M11) is complete and every checkbox in `docs/IMPLEMENTATION_PLAN.md` is `[x]`, open a `release/v1.0.0` → `main` PR, review it, and merge.
- `main` holds only documentation (`docs/`, `CONTRIBUTING.md`, `README.md`, etc.); application code joins `main` only when `release/v1.0.0` is merged in.
- Future releases (v1.1.0, etc.) follow the same pattern (`release/vX.Y.Z`).

## Commit Granularity

- **One checklist item in `docs/IMPLEMENTATION_PLAN.md` = one commit.**
- Don't bundle multiple checklist items into a single commit. Even if an item looks tiny (e.g., "create indexes"), give it its own commit — this keeps review and reverts easy.
- When a commit completes an item, update that checklist item from `[ ]` to `[x]` in the same commit (or the very next one). Code state and plan state must always stay in sync.
- Follow the milestone order (M0–M11). Exceptions are milestones explicitly marked as parallelizable in the plan (e.g., M3 backend fetch and M7 UI skeleton).
- Do not commit anything from the Chapter 13 roadmap (out-of-scope items) on this branch.

## Commit Message Format (Conventional Commits)

Format: `<type>(<scope>): <description> (M<number>)`

- **type**: one of `feat` / `fix` / `test` / `docs` / `chore` / `refactor`
- **scope**: the area the work belongs to (`db`, `fetch`, `scheduler`, `ui`, `commands`, etc.) — use the module structure from the plan (`src-tauri/src/{commands,db,fetch,scheduler}`)
- **description**: imperative mood (e.g., "add" not "added")
- End with the milestone number as `(M<number>)` so commits stay traceable against the plan

Examples:
```
feat(db): add feeds/items table schema (M1)
feat(db): add idx_items_published, idx_items_unread indexes (M1)
feat(fetch): apply ETag/Last-Modified conditional requests (M3)
test(fetch): verify 304/200/timeout scenarios with wiremock (M3)
fix(scheduler): use sleep_until instead of busy-loop when idle (M4)
docs: check off M1 checklist items
```

## Pre-PR Checklist

- [ ] Are all related checklist items marked `[x]`?
- [ ] Do the milestone's unit/integration tests pass (`cargo test`, frontend tests)?
- [ ] No `cargo clippy`/lint warnings?
- [ ] No out-of-scope (Chapter 13 roadmap) features snuck in?
