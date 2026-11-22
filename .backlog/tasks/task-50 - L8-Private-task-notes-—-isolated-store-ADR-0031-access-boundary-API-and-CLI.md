---
id: TASK-50
title: >-
  L8: Private task notes — isolated store, ADR-0031 access boundary, API, and
  CLI
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T03:38:00Z'
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
- [x] #1 Private task notes are stored outside shared task history and Git and pass PN-01..06.
- [x] #2 TASKAUTH-4, TASKAUTH-5, TASKAUTH-6, and TASKAUTH-7 pass: humans read agent notes read-only, authors edit/delete only their own, and other agents receive zero private note data across every surface.
- [x] #3 agent-os task note add|list|update|remove and /api/v1 private-note routes pass integration tests and FC-053 is updated.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Implemented isolated per-author private task notes (L8 / FC-053): verbatim private-notes-schema.sql SQLite store at runtime/<office-id>/private/task-notes.db (0700/0600, atomic DDL, WAL/SHM tightening) + MemoryPrivateNotesStore parity, ADR-0031 access boundary (PN-01..06, TASKAUTH-4..7 verified with restart, office.db rebuild, and durable/ zero-leak audit), 2-phase crash-safe publish-copy, 6 tasks.notes.* RPCs, /api/v1 BFF routes, agent-os/agentic-org task note CLI + session attach rewrite, and doc-3/doc-74 updates. Approved by Pass 2 fresh Neon Rust final review (8280a746).
<!-- SECTION:FINAL_SUMMARY:END -->
