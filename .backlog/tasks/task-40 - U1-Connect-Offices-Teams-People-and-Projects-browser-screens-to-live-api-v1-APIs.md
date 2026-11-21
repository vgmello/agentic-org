---
id: TASK-40
title: >-
  U1: Connect Offices, Teams, People, and Projects browser screens to live
  /api/v1 APIs
status: In Progress
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T14:57:00Z'
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

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Enable live organization shell routes (Offices, Teams, Team page, Members directory/profile, Org Chart, Projects, Access, Set-password) in capabilities.ts and root/shell routing while keeping unbuilt routes unavailable. 2. Implement production /api/v1 HTTP adapters (with unit tests) for offices/office-api, organization/http-api (+project team routes), teams, members (+A6 access/handover), org-chart, projects, access, and shell. 3. Support A6 invite-inspect/attach on /register and /set-password route. 4. Add real-instance Playwright e2e coverage in e2e-real/ for Offices, Teams, People, Org Chart, Projects, and Access, and run full checks.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Wired live /api/v1 HTTP production adapters (zero mock fallbacks) for Offices (offices/http-api.ts + office-api.ts), Organization project-team mutations (organization/http-api.ts + UUIDv7 newStableId), Teams (teams/http-api.ts + api.ts), Members & Access actions (members/http-api.ts + api.ts + ProfileFrame Remove person), Org Chart (org-chart/http-api.ts + api.ts), Projects (projects/http-api.ts + api.ts), Access overview (access/http-api.ts + api.ts), Shell navigation/search (shell/http-api.ts + api.ts), and A6 /register invite attach + /set-password. Enabled U1 live routes in platform/shell/capabilities.ts while keeping unbuilt routes unavailable with zero network requests. Verified check, lint, format:check, 164 unit test files, production build + check:build-assets, production-unavailable (7/7), and real-instance Playwright suite (6/6 including new e2e-real/org-screens.spec.ts).
<!-- SECTION:NOTES:END -->
