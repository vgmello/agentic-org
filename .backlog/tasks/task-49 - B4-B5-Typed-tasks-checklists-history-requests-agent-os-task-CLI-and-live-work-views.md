---
id: TASK-49
title: >-
  B4 & B5: Typed tasks, checklists, history, requests, agent-os task CLI, and
  live work views
status: In Progress
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T02:56:00Z'
labels:
  - backend
  - rust
  - ui
  - work
  - m1
milestone: m-0
dependencies:
  - TASK-3
  - TASK-4
  - TASK-6
  - TASK-7
  - TASK-8
  - TASK-36
  - TASK-35
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/03-features-and-work.md
  - docs/design/organization-data/12-task-assignment-and-history.md
  - docs/adr/0019-office-coded-task-numbers.md
  - docs/adr/0031-any-employee-changes-tasks.md
priority: high
type: feature
ordinal: 49000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Build unified typed tasks (task, bug, request) on org-store, deleting separate legacy bug/request/area tables, minting office-coded task references (T-B37) locally at commit, tracking accountable team + independent assigned team/assignee, immutable task history, ADR-0031 open active-employee mutations (TASKAUTH-1..3), plain checklists, shared comments, task/checklist CLI rows, and live SSE-backed browser board/list/detail views (FC-039..052, FC-054..055, FC-142).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Engine-independent work conformance suite passes on memory and SQLite, minting final office-coded task references even while HQ is unreachable.
- [x] #2 Legacy bug, request, and area tables/calls are deleted; TASKAUTH-2 and TASKAUTH-3 pass (and TASKAUTH-1 once C3 lands).
- [x] #3 agent-os and agentic-org task create|list|inspect|update|transition|assign|comment|watch and checklist commands pass K1 contract tests (CLI-15).
- [ ] #4 Browser work board, list, detail, bug, and request flows sync live without reload against a real instance (TSK, BUG, CHK, WORK criteria).
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Prerequisite from TASK-15 (UUIDv7 wire cutover): org-proto::web::CommandEnvelope now emits canonical lowercase UUIDv7 for OperationKind::Work (change_id), while legacy crates/org-work/src/engine.rs still checks ulid_time_ms(change_id). When wiring B4/B5 tasks onto org-store, validate work change_id via org_proto::uuid_v7_time_ms.

Backend B4/B5 vertical slice completed in commit c8a11ca9 (approved by Pass 4 fresh Neon Rust final review 8d65b26b): schema/docs parity (118/118), 20 tasks.* RPCs, engine-independent Memory+SQLite org-store with rebuild parity, ADR-0019 local T-<CODE><N> numbering, ADR-0031 active-employee mutations (TASKAUTH-1..3), checklists, shared comments, watchers, immutable task_events, AVAIL-2 agent wake, /api/v1 BFF routes, and agent-os/agentic-org CLI (CLI-15, K1 passing). Remaining for AC #4 only: browser UI workItemsApi wiring (deferred while prioritizing backend track per owner direction).
<!-- SECTION:NOTES:END -->
