---
id: TASK-36
title: >-
  B3: Projects minimum — accountable team, contributing teams, /api/v1 routes,
  and CLI
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - m1
milestone: m-0
dependencies:
  - TASK-34
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/02-project-structure.md
priority: high
type: feature
ordinal: 36000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement the projects record family on org-store with HQ numbering and slugs, accountable team (auto-creating a new team under TEAM-1 by default or binding an existing team), contributing teams, /api/v1 project routes, and project list|inspect|create CLI rows (FC-030, FC-038).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Project records, HQ number/slug allocation, default owning-team creation, and contributing teams pass the engine-independent suite on memory and SQLite.
- [ ] #2 Creating, listing, and inspecting projects work through /api/v1 and CLI and survive office restart and runtime projection rebuild.
- [ ] #3 FEATURES rows FC-030 and FC-038 are updated with verified evidence.
<!-- AC:END -->
