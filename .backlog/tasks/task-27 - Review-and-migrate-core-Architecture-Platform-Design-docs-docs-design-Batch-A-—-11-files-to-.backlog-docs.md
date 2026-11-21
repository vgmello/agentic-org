---
id: TASK-27
title: >-
  Review and migrate core Architecture & Platform Design docs (docs/design/,
  Batch A — 11 files) to .backlog/docs/
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - docs
  - architecture
  - backlog-migration
milestone: m-5
dependencies:
  - TASK-19
documentation:
  - docs/design/documentation-authority.md
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 27000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Individually review each core architecture/platform design document in docs/design/ against current v1 code and the documentation authority map, then migrate them into .backlog/docs/design/ via `backlog doc create`.

Files in scope (11):
- docs/design/accounts-sandbox-and-extensions.md
- docs/design/documentation-authority.md
- docs/design/executable-migration.md
- docs/design/linux-host.md
- docs/design/office-host-boundary.md
- docs/design/office-store.md
- docs/design/offices-hq-and-satellites.md
- docs/design/project-structure.md
- docs/design/rust-browser-bff-and-static-sveltekit.md
- docs/design/standalone-server.md
- docs/design/web-api-v1.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 All 11 architecture/platform design documents are individually reviewed for drift against implemented crates, binaries, and ADRs.
- [ ] #2 All 11 documents are migrated into .backlog/docs/design/ via `backlog doc create` with reviewed updates applied.
- [ ] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->
