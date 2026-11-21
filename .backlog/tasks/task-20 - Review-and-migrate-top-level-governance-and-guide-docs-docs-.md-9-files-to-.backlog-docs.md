---
id: TASK-20
title: >-
  Review and migrate top-level governance and guide docs (docs/*.md, 9 files) to
  .backlog/docs/
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - docs
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/START-HERE.md
  - docs/FEATURES.md
  - docs/architecture-rules.md
  - docs/architecture-non-negotiables.md
priority: medium
type: docs
ordinal: 20000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Every existing documentation file in the repository needs an individual accuracy review against the current v1 codebase and migration into .backlog/docs/ via `backlog doc create` so Backlog.md becomes the single curated home for repository documentation.

Files in scope (9):
- docs/START-HERE.md
- docs/FEATURES.md
- docs/FEATURE-AREAS.md
- docs/implementation-plan.md
- docs/independent-delivery.md
- docs/architecture-non-negotiables.md
- docs/architecture-rules.md
- docs/rust-style-guide.md
- docs/rust-style-guide.LICENSE
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Each of the 9 top-level files in docs/ is individually reviewed against current v1 implementation reality, with stale claims or broken references flagged or corrected.
- [ ] #2 Every Markdown doc in this batch is migrated into .backlog/docs/ using `backlog doc create` with appropriate document type and relative path (and license attribution preserved for rust-style-guide).
- [ ] #3 A migration mapping records old path -> new .backlog doc ID/path and any review findings requiring follow-up.
<!-- AC:END -->
