---
id: TASK-105
title: >-
  Task Query Hygiene & Performance: SQL Pushdown, Batched Hydration (Fix 9N N+1
  Storm), Open-by-Default / done_since Filtering & Partial Indexes
status: To Do
assignee: []
created_date: '2026-11-24T04:52:00Z'
updated_date: '2026-11-25T00:16:00Z'
labels:
  - backend
  - rust
  - performance
  - storage
  - tasks
  - cli
  - ui
  - docs
milestone: m-0
dependencies:
  - TASK-49
  - TASK-85.3
documentation:
  - >-
    .backlog/docs/organization-data/doc-77 -
    Organization-Data-03-—-Features-and-Work.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - >-
    .backlog/docs/design/doc-100 -
    Design-Storage-Scalability-DB-Split-Verdict-and-Two-Tier-Task-Archive.md
priority: high
type: enhancement
ordinal: 120000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Fixes a severe live O(N) + 9N N+1 SQL query storm in tasks.list (crates/org-office/src/tasks/features/queries.rs:59-83, store/sqlite/tasks.rs:279-291, tasks/projector.rs:131-170) that loads every task in the organization and executes 4–9 SQL queries per task before applying any filter or LIMIT (~40k–90k queries at 10k tasks, causing 2–4s stalls and starving WAL checkpoints), and pulls lightweight task-board/CLI query hygiene into v1 with zero schema.sql table changes so full archiving (TASK-86) stays cleanly in v2.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Extends TasksListParams (crates/org-proto/src/tasks.rs:470-501) with status_scope (open | closed | all) and done_since: Option<String>; defaults agent-os task list to open tasks unless --status or --all is passed, and pushes the Kanban Board's 7-day Done cutoff (WorkBoard.svelte / doc-49 TSK-1) to the server query so historical closed tasks are not downloaded over HTTP only to be hidden client-side.
- [ ] #2 Replaces full-table view.tasks() loading in list_tasks (queries.rs:59-83) and validate_tasks (projector.rs:131-170) with SQL WHERE/ORDER BY/LIMIT pushdown and batched IN-clause child hydration (task_labels, task_files, task_repositories, task_details, checklist_steps, comments, watchers, links, dependents) in crates/org-office/src/store/sqlite/tasks.rs, keeping MemoryEngine parity (ADR-0018).
- [ ] #3 Adds partial open-task indexes in schema.sql (tasks_open_by_project and tasks_open_by_team WHERE status NOT IN ('done', 'cancelled')) with test_schema.py & doc-74/doc-77 alignment.
- [ ] #4 Adds benchmark/regression tests proving constant bounded SQL statement count (O(1) batched queries regardless of page/org size) and <5 ms filtered task list latency at 2,000+ seeded tasks without starving PASSIVE WAL checkpoints; updates doc-28 and doc-49.
<!-- AC:END -->
