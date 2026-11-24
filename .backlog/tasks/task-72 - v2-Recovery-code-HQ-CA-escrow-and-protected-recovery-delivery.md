---
id: TASK-72
title: 'v2: Recovery code, HQ CA escrow, and protected recovery delivery'
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - v2
  - security
  - recovery
milestone: m-1
dependencies: []
documentation:
  - docs/adr/0007-hq-creation-escrow.md
  - docs/adr/0011-protected-recovery-delivery.md
  - docs/design/accounts-sandbox-and-extensions.md
priority: low
type: feature
ordinal: 72000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Deferred from v1 by owner decision D24 (2026-11-20): complete recovery code release on the normal pinned service (ADR-0011 AC 5), browser recovery-state flow, REC-1 acceptance, and service-account presence/authority handling for HQ CA escrow (FC-016).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Protected recovery code release works over the normal pinned service without terminal passwords (ADR-0011 AC 5).
- [ ] #2 Browser recovery-state UX, off-HQ acknowledgement, and REC-1 acceptance pass under dedicated service accounts.
- [ ] #3 FEATURES row FC-016 is updated to Done.
<!-- AC:END -->
