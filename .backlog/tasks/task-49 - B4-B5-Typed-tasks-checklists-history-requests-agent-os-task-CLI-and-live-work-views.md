---
id: TASK-49
title: >-
  B4 & B5: Typed tasks, checklists, history, requests, agent-os task CLI, and
  live work views
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
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
- [ ] #1 Engine-independent work conformance suite passes on memory and SQLite, minting final office-coded task references even while HQ is unreachable.
- [ ] #2 Legacy bug, request, and area tables/calls are deleted; TASKAUTH-2 and TASKAUTH-3 pass (and TASKAUTH-1 once C3 lands).
- [ ] #3 agent-os and agentic-org task create|list|inspect|update|transition|assign|comment|watch and checklist commands pass K1 contract tests (CLI-15).
- [ ] #4 Browser work board, list, detail, bug, and request flows sync live without reload against a real instance (TSK, BUG, CHK, WORK criteria).
<!-- AC:END -->
