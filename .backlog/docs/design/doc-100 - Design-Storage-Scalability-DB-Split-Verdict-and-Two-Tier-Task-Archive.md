---
id: doc-100
title: 'Design: Storage Scalability, DB Split Verdict and Two-Tier Task Archive'
type: specification
created_date: '2026-11-25T00:13:00Z'
updated_date: '2026-11-25T00:14:00Z'
tags:
  - design
  - architecture
  - storage
  - sqlite
  - performance
  - archive
  - v1
  - v2
---

# Design: Storage Scalability, DB Split Verdict & Two-Tier Task Archive

> [!NOTE]
> **Principal Systems Architect Review** against live `crates/org-office`, `crates/org-store`, `docs/design/organization-data/schema.sql`, `ADR-0006`, `ADR-0014`, `ADR-0016`, `ADR-0018`, `ADR-0019`, and `ADR-0021`.  
> **Backlog Tasks:** `TASK-105` (**`v1 / m-0`**) · `TASK-87` (**`v1 / m-0`**) · `TASK-86` (**`v2 / m-1`**) · `TASK-106` (**`v2 / m-1`**)

---

## 1. Executive Verdicts

| Question | Verdict | Bottom Line |
| --- | --- | --- |
| **1. Splitting `tasks` into its own `tasks.db` (3-DB split)** | **DO NOT DO THIS** | Agentic Org **already runs 6 per-office SQLite DBs** (including `protected/messaging.db` and `durable/private/notes.db`). Splitting `tasks.db` out of `office.db` gives **0% write parallelism** (all writes still serialize through 1 `org-store` writer thread, 1 acceptance journal, 1 `SqliteEngine` writer mutex, and 1 `commit_gate`), **breaks 9 outgoing FKs, incoming FKs/`record_refs`, 5 SQL views, 15 cross-table triggers, and 13 `test_schema.py` tests**, triples partial-commit failure windows (`TASK-97` ARCH-04), slows every batch, and does not speed up `Delete + Rebuild`. |
| **2. Task Archiving (`TASK-86`)** | **PULL QUERY HYGIENE INTO `v1` (`TASK-105`); KEEP TWO-TIER ARCHIVE IN `v2` (`TASK-86`)** | Pull server-side SQL pushdown + batched hydration + open-by-default/7d cutoff + partial open indexes into `v1` (`TASK-105`, zero schema churn, **1000× faster reads at 10k tasks**). Keep reversible Warm Archive (`archived_at`) + Cold Archive bundles (`durable/archive/work/`) in `v2` (`TASK-86`). |
| **3. Real Write-Path & Scale Bottlenecks** | **FIX THE REAL BOTTLENECKS (`TASK-105`, `TASK-87`, `TASK-106`)** | Four verified bottlenecks actually govern latency and scale: (1) `tasks.list` 9N N+1 full-table scan, (2) task ops hardcoding `Ack::Durable` instead of `ADR-0014` `Ack::Committed`, (3) `org-store` flusher running per-file `F_FULLFSYNC` on the durable journal flush thread, and (4) messaging rewriting full `received/snapshot.json` instead of appending JSONL segments. |

---

## 2. Ground Truth: 7 SQLite Databases Already Exist Per Host/Office

1. `<runtime>/<office-id>/office.db` — Shared rebuildable projection (`schema.sql`: 79 tables, 12 views, 86 triggers, 65 indexes)
2. `<runtime>/<office-id>/protected/messaging.db` — **Already separate** (`messaging-schema.sql`, confidentiality isolation under `ADR-0016` §7 / `TASK-10`)
3. `<durable>/private/notes.db` (formerly `task-notes.db`) — **Already separate**, HQ-authoritative (`private-notes-schema.sql`, `ADR-0031`)
4. `<runtime>/<office-id>/protected/accounting.db` — **Already separate** (usage & cost ledger)
5. `<runtime>/<office-id>/protected/budgets.db` — **Already separate** (`budgets-schema.sql`)
6. `<runtime>/<office-id>/protected/providers.db` — **Already separate** (provider bindings, discovery, quota permits)
7. `<host>/registry.db` — Host accounts, browser sessions, office registry

---

## 3. Why Splitting `tasks.db` Gives 0% Write Parallelism & Is Structurally Toxic

### Shared 6-Stage Serialization Path
Every org, task, and chat write currently flows through:
1. `Store::jobs` Mutex + `mpsc::channel` (`org-store/src/store.rs:331,540`)
2. Single `store-writer` thread batching up to `MAX_BATCH = 64` (`writer.rs:130-165`)
3. Single `SqliteEngine::writer` Mutex (`BEGIN IMMEDIATE` + `SAVEPOINT` + `check_references()` on every paired DB)
4. Single `org-store` Acceptance Journal + `Core::flow` Mutex (strictly ordered `ordinal` + SHA-256 hash chain)
5. `SqliteTx::finish()` under `commit_gate.write()` (blocking concurrent reader `snapshot()` BEGINs while committing each DB sequentially)
6. Single `store-flusher` thread (`journal.flush()` + `custody.confirm()` + per-file `F_FULLFSYNC` in `Files::materialize()` + `PASSIVE` WAL checkpoint)

Adding a 3rd DB inside `SqliteEngine` adds a 3rd `BEGIN IMMEDIATE`, 3rd `SAVEPOINT`, 3rd `COMMIT` inside `commit_gate.write()` (~50% longer reader stalls), triples partial-commit divergence windows, violates `ADR-0006` and `ADR-0021`, and diverges from single-ACID PostgreSQL under `ADR-0018`.

### Relational Coupling Broken by a `tasks.db` Split
- **9 outgoing FKs** from `tasks` into 6 `office.db` tables (including 3 composite parent-attribute FKs on `projects.owner_team_id`, `features.project_id`, `milestones.project_id`).
- **Incoming FKs into `tasks`** from `record_refs` (15-kind polymorphic hub), `task_workspaces`, `release_tasks`, and `task_document_versions`.
- **5 SQL views destroyed:** `entity_relationships`, `milestone_progress`, `release_progress`, `task_releases`, `document_version_progress`.
- **15 cross-table triggers destroyed** and **13 of 118 `test_schema.py` tests broken**.
- **Search (`TASK-91`) & Attachments (`TASK-98`):** Shared entities stay in one `office.db` FTS5 index; splitting forces cross-DB BM25 score merging.

---

## 4. Scale Math at 10k Tasks / 100k Comments / 200k Chat Messages

- **Footprint:** `office.db` is **~335 MB** (~410 MB with `TASK-91` FTS5); `protected/messaging.db` is **~620 MB**. Well inside OS page cache (B-tree depth 3–4; indexed reads `< 0.5 ms`). **DB size is not a bottleneck.**
- **Real Bottleneck #1 — `tasks.list` 9N N+1 Query Storm (`TASK-105`, v1):** `list_tasks` (`queries.rs:59-83`) loads every task in the org and runs 4 queries/task in `hydrate_task()` (`sqlite/tasks.rs:124-171`) + 5 queries/matched task in `build_task_view()` = **40k–90k SQL statements** (~1.5–4s at 10k tasks), starving WAL checkpoints. `validate_tasks()` (`tasks/projector.rs:131-170`) repeats the 4N storm during rebuild.
- **Real Bottleneck #2 — Wrong `Ack` Class on Hot Task Ops (`TASK-105` / `TASK-87`, v1):** `CommentTask`, `WatchTask`, `UnwatchTask`, and `ChecklistCheck` hardcode `Ack::Durable` instead of `ADR-0014` `Ack::Committed`; `CreateTask` needs `ADR-0019` durable block reservation on `office_task_counters.reserved_through` to safely use `Ack::Committed` (< 1 ms vs 10–100 ms).
- **Real Bottleneck #3 — `org-store` Flusher Head-of-Line Blocking & WAL Pressure (`TASK-106`, v2):** `flusher::cycle()` runs per-file `F_FULLFSYNC` in `Files::materialize()` on the same thread that flushes the journal for durable callers; `wal_autocheckpoint = 0` relies solely on `PASSIVE` checkpoints without a `64 MiB` pressure valve.
- **Real Bottleneck #4 — Messaging O(M²) Snapshot Rewrite (`TASK-106`, v2):** `upsert_participant_snapshot()` rewrites the entire `received/snapshot.json` on every send/read/triage instead of appending to `received/segments/*.jsonl` per `doc-90:50` & `doc-91:316`.

---

## 5. Two-Tier Task Archive & Priority Roadmap

| Priority | Task & Target | Scope & Impact |
| --- | --- | --- |
| **P1 (`v1 / m-0`)** | **`TASK-105`** (Task Query Hygiene & Performance) | SQL `WHERE`/`ORDER BY`/`LIMIT` pushdown + batched child hydration + `open`-by-default & `done_since` 7d cutoff + partial open-task indexes + align hot task `Ack::Committed` per `ADR-0014`. **1000× faster reads (~2,000 ms → < 2 ms)** and stops WAL checkpoint starvation with zero schema churn. |
| **P1 (`v1 / m-0`)** | **`TASK-87`** (Task Storage Reconciliation) | Implement `ADR-0019` durable counter block reservation (`office_task_counters.reserved_through`) so `CreateTask` safely runs `Ack::Committed` (< 1 ms). |
| **P2 (`v2 / m-1`)** | **`TASK-106`** (Storage Write-Throughput Hardening) | Decouple `org-store` journal group-flush from background file materialization (`files.rs:244`), release `Core::flow` during journal append, switch messaging states to append-only `received/segments/*.jsonl`, and add a bounded WAL checkpoint pressure valve. |
| **P3 (`v2 / m-1`)** | **`TASK-86`** (Two-Tier Warm + Cold Task Archive) | **Tier 1 Warm Archive:** nullable `archived_at` + partial indexes `WHERE archived_at IS NULL` (reversible, read-only in `office.db`, FKs/views intact). **Tier 2 Cold Archive:** age-based maintenance packing warm-archived tasks (`> 180/365d`) into immutable `durable/archive/work/<YYYY-MM>/` bundles excluded from hot `office.db` rebuild walks while keeping lightweight stubs/`record_refs` so keys, deps, usage, and private-note mirrors never break. |
| **P4 (Future, only if profiled)** | Standalone Messaging `Store` | If profiling *ever* proves >5,000+ sustained writes/sec single-writer saturation, decouple **`protected/messaging.db`** into its own independent `Store` — **never** split `tasks.db` out of `office.db`. |
