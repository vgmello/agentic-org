---
id: TASK-38
title: >-
  A6: Multi-person accounts — browser invites, password resets, access removal,
  and ownership handover
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - auth
  - m1
milestone: m-0
dependencies:
  - TASK-34
documentation:
  - docs/implementation-plan.md
  - docs/specs/server-installation-and-access-ux.md
  - docs/design/organization-data/04-agent-structure.md
priority: high
type: feature
ordinal: 38000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Enable inviting a second person in the browser via one-time registration links, password reset links, self-service password change (ending other sessions), access removal, admin and organization ownership handover, UUIDv7 account IDs, session refresh/touch batching (A3 B7/B8 leftovers), and SIGN-34 SSH refusal plus admin reset dialog wording (FC-012, FC-014, FC-029, FC-153, FC-159).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Two people can sign in concurrently from separate browsers using browser-issued registration links and each action carries their tied person record.
- [ ] #2 Set-password links, password change invalidating other sessions, access removal revoking sessions, and admin/owner handover work end-to-end.
- [ ] #3 Dead CLI account.create ops are removed, account IDs are UUIDv7, session touch batching is implemented, and SIGN-34 SSH refusal/dialog copy pass.
- [ ] #4 FEATURES rows FC-012, FC-014, FC-029, FC-153, and FC-159 are updated.
<!-- AC:END -->
