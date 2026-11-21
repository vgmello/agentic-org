---
id: TASK-62
title: >-
  X7: Switch container deployment to HTTPS and resolve admin tls set on
  Linux/containers
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - linux
  - container
  - security
  - m2
milestone: m-0
dependencies:
  - TASK-1
documentation:
  - docs/workstreams/linux.md
  - docs/implementation-plan.md
  - docs/adr/0017-linux-and-container-hosting.md
priority: high
type: feature
ordinal: 62000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Finish Track X increment X7: switch deploy/container/setup.json, README, and deploy/container/lnx2.sh to HTTPS; resolve owner decision on Linux/container route for `agentic-org admin tls set` (browser-approved CLI sign-in FC-173 vs root-local tls.set); and fix X7 P3 follow-ups (re-announce tls.renew after failed web fetch and tighten gate cache doc wording) (FC-017, FC-164).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 deploy/container/setup.json, container README, and deploy/container/lnx2.sh run over HTTPS and pass in CI's container job.
- [ ] #2 Owner decision for Linux/container `agentic-org admin tls set` is recorded and implemented.
- [ ] #3 tls.renew is re-announced after a failed web fetch and gate cache wording is updated.
<!-- AC:END -->
