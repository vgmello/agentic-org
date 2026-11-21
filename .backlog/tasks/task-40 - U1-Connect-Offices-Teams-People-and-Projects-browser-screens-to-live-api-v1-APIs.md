---
id: TASK-40
title: >-
  U1: Connect Offices, Teams, People, and Projects browser screens to live
  /api/v1 APIs
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - ui
  - bff
  - m1
milestone: m-0
dependencies:
  - TASK-12
  - TASK-36
  - TASK-37
  - TASK-38
documentation:
  - docs/implementation-plan.md
  - docs/specs/offices-and-teams-ux.md
  - docs/specs/our-org-in-the-app.md
priority: high
type: feature
ordinal: 40000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Replace mock/unavailable production adapters for Offices, Teams, Members, Org Chart, and Projects screens with live /api/v1 calls and verify TEAM, PEOPLE, and REPORT criteria in real-instance browser tests (FC-003, FC-019, FC-025..027, FC-030, FC-032, FC-038).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Offices, Teams index/page, Members directory, Org Chart, and Projects screens run against live /api/v1 endpoints with zero mock fallbacks.
- [ ] #2 TEAM, PEOPLE, and REPORT criteria not requiring an agent runtime pass in real-instance Playwright tests.
- [ ] #3 FEATURES rows FC-003, FC-019, FC-025..027, FC-030, FC-032, and FC-038 are updated.
<!-- AC:END -->
