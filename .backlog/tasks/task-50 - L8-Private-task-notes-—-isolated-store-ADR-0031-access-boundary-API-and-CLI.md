---
id: TASK-50
title: >-
  L8: Private task notes — isolated store, ADR-0031 access boundary, API, and
  CLI
status: In Progress
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T02:56:00Z'
labels:
  - backend
  - rust
  - security
  - work
  - m1
milestone: m-0
dependencies:
  - TASK-8
  - TASK-11
  - TASK-49
  - TASK-42
documentation:
  - docs/implementation-plan.md
  - docs/adr/0031-any-employee-changes-tasks.md
  - docs/design/organization-data/05-operational-data.md
  - docs/design/organization-data/13-private-task-notes.md
priority: high
type: feature
ordinal: 50000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement per-author private task notes in dedicated protected storage apart from shared task history and Git, enforcing ADR-0031 visibility (author and people read; zero leakage to other agents across reads, search, feeds, summaries, exports, and turn inputs; author-only edit/delete), agent-os task note CLI, and /api/v1 endpoints for the browser private notes pane (FC-053).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Private task notes are stored outside shared task history and Git and pass PN-01..06.
- [ ] #2 TASKAUTH-4, TASKAUTH-5, TASKAUTH-6, and TASKAUTH-7 pass: humans read agent notes read-only, authors edit/delete only their own, and other agents receive zero private note data across every surface.
- [ ] #3 agent-os task note add|list|update|remove and /api/v1 private-note routes pass integration tests and FC-053 is updated.
<!-- AC:END -->
