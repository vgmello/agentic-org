---
id: TASK-97
title: Milestone 1 Architecture & Code-Quality Audit Findings (Neon XHigh)
status: Done
assignee: []
created_date: '2026-11-23T05:37:00Z'
updated_date: '2026-11-23T13:57:00Z'
labels:
  - architecture
  - audit
  - backend
milestone: m-0
dependencies: []
priority: high
ordinal: 92000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Independent Milestone 1 architecture and code-quality audit executed read-only via `agy --model gemini-4-neon-xhigh --effort xhigh --mode plan` on branch `v1` (`7dcc5d7f`), followed by mandatory lead code-level verification (Real vs. False Positive / Scope Triage).

### Confirmed High Blockers — Fixing Immediately Before TASK-52
1. **ARCH-01 (Real — High):** `SqlitePrivateNotesStore` opened `<runtime>/<office-id>/private/task-notes.db` instead of HQ-authoritative `<durable>/private/task-notes.db` (`doc-79` §Private Task Notes: "not a disposable office cache"). Fix: move to `<durable>/private/task-notes.db` (`0700`/`0600` + `full_sync`), update tests and stale doc-72/doc-74/doc-93 references.
2. **ARCH-04 (Partially Real — High):**
   - *Real #1:* `SqliteTx::finish` (`store/sqlite/mod.rs:719-747`) commits `msg_db` then `db`; if `db` commit fails, `Drop` returns the diverged `(db, msg_db)` pair back into `self.writer`! Fix: poison/do not return diverged pair on partial commit and return `EngineError::ended`.
   - *Real #2:* `SqliteEngine::snapshot()` acquires `BEGIN` on `db` then `msg_db` while `finish()` commits `msg_db` then `db`, allowing torn cross-DB snapshots. Fix: add leaf `RwLock<()>` `commit_gate` held briefly in read mode across the two `BEGIN`s and write mode across the two `COMMIT`s.
   - *Already Handled (False Positive):* Open-time cursor parity rebuild already exists at `store/sqlite/mod.rs:322-338`.
3. **ARCH-05 (Real — High):** Companion stores (`budgets.rs`, `accounting.rs`, `providers.rs`, `org-office-sessions/src/credentials.rs`) used `File::sync_all()` instead of `org_platform::fs::full_sync` (`F_FULLFSYNC` on macOS), silently dropped parent-dir sync errors (`budgets.rs`, `providers.rs`), wrote `budget.json` before `versions/`/`events/`/`periods/`/`notices/`/`holds/`, failed hard on a torn trailing JSONL line in `accounting.rs`, omitted `-wal`/`-shm` tightening in `providers.rs`, and lacked schema-digest + corrupt-DB delete-and-rebuild in companion SQLite projections. Fix all immediately via shared `private_fs.rs` durability helpers.
4. **ARCH-06 Part B (Real — High):** `apply_imported_to_engine` (`store/open.rs:512-560`) mounted only `OrganizationFiles` and `ProjectFiles`, omitting `TaskFiles` and `ProcessFiles` so Git import (`R1`) failed closed on any org with tasks or processes! Fix: mount all shared `state/` projectors (`OrganizationFiles`, `ProjectFiles`, `TaskFiles`, `ProcessFiles`) and make `apply_imported_to_engine` generic over `E: OfficeEngine` (note: naming `SqliteEngine` inside `store/open.rs` itself is NOT an `ENG-1` violation since `store/open.rs` is the sole composition boundary).

### Confirmed High Findings Owned Directly by Upcoming m-0 Tasks
- **ARCH-02 (Real — High -> TASK-52):** Single-HQ assumptions in `Office::open_with_owner`, `SqliteProviderPoolStore::open` (mutating `durable/authority/`), and `org-office-server`. Solved directly in `TASK-52` Slices 2 & 4.
- **ARCH-03 (Real — High -> TASK-52):** `org-store/src/flusher.rs` releasing entries without replication consumer watermarks, missing outbox reader / `apply_replicated`, and legacy `org-sync` (`Ca=0`). Solved directly in `TASK-52` Slice 2.
- **ARCH-07 (Real — High -> TASK-80.1):** `builtin_key` hardcoded to `'hr'` with `may_hire = 0`. Generalized directly in `TASK-80.1` (Tess Chief of Staff).

### Confirmed High Drift — Recorded Decision / Tracked Follow-Up
- **ARCH-06 Part A (Real — High, tracked):** `tasks/records.rs` uses flat JSON (`state/work/tasks/<id>/*.json`) rather than `doc-77`/`doc-79` scoped Markdown/YAML (`projects/<pid>/work/tasks/<id>/task.md`). Keep existing tested JSON format stable across `TASK-84`/`TASK-55`/`TASK-56` and align canonical Markdown/YAML task serialization in a dedicated migration before external Git interop.

### Medium & Low Findings Triage (`ARCH-08`..`ARCH-21`)
- **ARCH-15 (Medium -> TASK-85.2/85.3):** `instructions.rs` prefix-cache ordering & CLI catalogue > 4 KiB (already scheduled).
- **ARCH-12 & ARCH-14 (Medium -> TASK-55):** `office/budgets.rs` read-path O(N) scans + disk writes + `By::System` auto-hold attribution, and `MemoryBudgetStore` invariant parity (fix alongside `TASK-55` release-budget auth work).
- **ARCH-18 (Low — Fixing now):** Unpinned `ring`/`zeroize` in `org-office-sessions/Cargo.toml` -> align with workspace versions.
- **ARCH-08, ARCH-09, ARCH-10, ARCH-11, ARCH-13, ARCH-16, ARCH-17, ARCH-19, ARCH-20, ARCH-21 (Real Medium/Low — Tracked Debt):** Retired `org-work` removal, `org-office-sessions` adapter inversion, remaining `#[cfg(unix)]` cleanup (companion-store duplicates removed now in ARCH-05), god-module decomposition, central-vs-domain auth boundary cleanup, `CliEnrollmentDriver` synthetic device code / global static removal, hardcoded `"This Mac"`, JSON in `binding.yaml`, error boilerplate & stale comments.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Fix ARCH-01: move private task-notes.db to HQ-authoritative <durable>/private/task-notes.db (0700/0600 + full_sync) and align tests and doc references
- [x] #2 Fix ARCH-04: prevent diverged (db, msg_db) reuse after partial commit in SqliteTx::finish and eliminate cross-DB torn snapshots via leaf commit_gate RwLock
- [x] #3 Fix ARCH-05 & ARCH-18: route companion store durability through org_platform::fs::full_sync with propagated dir-sync errors, child-before-root budget writes, torn trailing JSONL tolerance, corrupt/digest-mismatch companion DB rebuild, providers.db sidecar tightening, and pinned ring/zeroize deps
- [x] #4 Fix ARCH-06 Part B: mount TaskFiles and ProcessFiles alongside OrganizationFiles and ProjectFiles in apply_imported_to_engine (generic over OfficeEngine) with regression test
- [x] #5 Track remaining Medium/Low architectural debt (ARCH-06 Part A, ARCH-08..17, ARCH-19..21) and fold ARCH-02/03 -> TASK-52, ARCH-07 -> TASK-80.1, ARCH-12/14 -> TASK-55, ARCH-15 -> TASK-85
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed and verified all Milestone 1 high-severity architecture audit blockers (ARCH-01, ARCH-04, ARCH-05, ARCH-06 Part B, ARCH-18) plus unified atomic session usage-capture durability and clean shutdown flush. Approved by tiered strictly read-only Codex review: Sol xhigh (01a20700-7fb1-7df2-9dea-e855bf4e90a5, VERDICT: APPROVE) and separate fresh Sol max Final Gate (01a20778-1967-73d1-bbbd-ee346e72f7fe, Pass 27 VERDICT: APPROVE). Checks: cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, org-office-sessions (44/44), org-office (accounting_budget_tests, store::sqlite::tests, engine_boundary), org-office-server (284/284), test_schema.py (118/118).
<!-- SECTION:NOTES:END -->
