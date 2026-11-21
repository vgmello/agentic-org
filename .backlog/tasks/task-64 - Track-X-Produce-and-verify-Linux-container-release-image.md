---
id: TASK-64
title: 'Track X: Produce and verify Linux container release image'
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - linux
  - container
  - release
  - late-v1
milestone: m-0
dependencies:
  - TASK-63
  - TASK-60
documentation:
  - docs/workstreams/linux.md
  - docs/design/linux-host.md
priority: low
type: chore
ordinal: 64000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Build and verify the production Linux container release image once the release pipeline (L9) lands (image signing remains outside Track X scope per linux.md).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Release container image builds reproducibly from pinned release artifacts and passes LNX-2 HTTPS verification.
- [ ] #2 Container run documentation matches the published release image behavior.
<!-- AC:END -->
