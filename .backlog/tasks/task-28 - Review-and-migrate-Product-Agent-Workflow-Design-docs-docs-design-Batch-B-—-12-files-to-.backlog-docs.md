---
id: TASK-28
title: >-
  Review and migrate Product, Agent & Workflow Design docs (docs/design/, Batch
  B — 12 files) to .backlog/docs/
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
  - docs/design/documentation-authority.md
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 28000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Individually review each product, agent, and workflow design document in docs/design/ against the authoritative Organization Data contracts and current v1 scope, then migrate them into .backlog/docs/design/ via `backlog doc create`.

Files in scope (12):
- docs/design/agent-instructions.md
- docs/design/agent-os-command-line.md
- docs/design/agent-sessions.md
- docs/design/create-organization.md
- docs/design/desired-state-specifications.md
- docs/design/development-processes.md
- docs/design/employee-supervision.md
- docs/design/processes.md
- docs/design/projects-git-github.md
- docs/design/smart-features.md
- docs/design/tasks-projects-docs.md
- docs/design/teams-and-org-chart.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 All 12 product/agent/workflow design documents are individually reviewed for consistency with Organization Data contracts and current v1 scope (clearly marking deferred/v2 sections).
- [ ] #2 All 12 documents are migrated into .backlog/docs/design/ via `backlog doc create`.
- [ ] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->
