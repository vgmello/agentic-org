---
id: TASK-21
title: >-
  Review and migrate docs/architecture/ overview and audit reports (4 files) to
  .backlog/docs/
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - docs
  - architecture
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/architecture/README.md
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 21000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Review and migrate the architecture overview and audit documents under docs/architecture/ into .backlog/docs/ via `backlog doc create` so architectural baselines and audit reports live inside .backlog/.

Files in scope (4):
- docs/architecture/README.md
- docs/architecture/architecture-overview.html
- docs/architecture/system-architecture.html
- docs/architecture/architecture-and-data-audit.html
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 All 4 files in docs/architecture/ are individually reviewed for current accuracy and overlap.
- [ ] #2 Markdown summaries/canonical docs are created in .backlog/docs/architecture/ via `backlog doc create`, and an explicit disposition is recorded for the 3 interactive HTML reports (Markdown conversion vs static asset retention linked from Backlog docs).
- [ ] #3 Cross-references from tasks and architecture docs resolve cleanly.
<!-- AC:END -->
