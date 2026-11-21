---
id: TASK-34
title: >-
  B2.2 & B2.3b: Complete B2 dry-run, operation.get, /api/v1 organization routes,
  and UI wire mirrors
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - bff
  - m1
milestone: m-0
dependencies:
  - TASK-5
  - TASK-14
documentation:
  - .agent-kit/slices/b2-organization-teams-people-plan.md
  - docs/design/web-api-v1.md
  - docs/workstreams/backend.md
priority: high
type: feature
ordinal: 34000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Complete the remaining B2.2 and B2.3b scope not covered by TASK-5 (B2.3a browser-to-office bridge) or TASK-14 (core OrgCommand/OrgSnapshot wire cutover): add org_store::Store::check dry-run, command_id expired/clock_skew validation, operation.get backed by store receipts, /api/v1 organization mutation and operation lookup routes, concurrency test for B2 risk C1 (team move racing descendant rename), and coordinated UI wire mirror updates.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 org_store::Store::check executes dry-run decide+apply+validate+undo with zero journal frames, files, or receipts, and command_id expired/clock_skew rules pass.
- [ ] #2 operation.get returns committed and rejected organization receipts for the acting principal and refuses another actor's receipt as not_found.
- [ ] #3 All /api/v1/organizations/{org}/* mutation routes and GET /api/v1/operations/org/{id} pass real-process HTTP tests including replay, conflict, and restart.
- [ ] #4 Stale pre-B2.1 test vectors are replaced/removed, UI wire mirrors are updated in sync, team-move vs descendant-rename concurrency (risk C1) is tested, and FEATURES FC-025..029 are updated.
<!-- AC:END -->
