---
id: TASK-29
title: >-
  Review and migrate Core App, Org, Work & Chat UX specs (docs/specs/, Batch A —
  11 files) to .backlog/docs/
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - docs
  - ui
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/design/documentation-authority.md
priority: medium
type: docs
ordinal: 29000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Individually review each core UI/UX specification in docs/specs/ against current apps/agentic-org-ui implementation, v1 scope, and data contracts, then migrate them into .backlog/docs/specs/ via `backlog doc create`.

Files in scope (11):
- docs/specs/app-shell-ux.md
- docs/specs/chat-ux.md
- docs/specs/dogfood-run.md
- docs/specs/feature-requests-ux.md
- docs/specs/features-ux.md
- docs/specs/offices-and-teams-ux.md
- docs/specs/our-org-in-the-app.md
- docs/specs/project-status-ux.md
- docs/specs/roles-ux.md
- docs/specs/server-installation-and-access-ux.md
- docs/specs/work-items-ux.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 All 11 core UX spec files are individually reviewed for alignment with Organization Data contracts and current v1 UI scope.
- [ ] #2 All 11 specifications are migrated into .backlog/docs/specs/ via `backlog doc create` with type specification.
- [ ] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->
