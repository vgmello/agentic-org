---
id: TASK-62
title: >-
  X7 & ADR-0033: Default HTTP hosting between browser and office with opt-in
  in-app HTTPS enablement
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T05:11:00Z'
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
Implement owner requirement (ADR-0033, 2026-11-21) and finish Track X X7: allow the server (macOS, Linux, and container) to start hosted via HTTP between the browser and the office by default to reduce complexity, and let teams opt in to HTTPS inside the app (and via admin tls set). Keep X7 P3 fixes (re-announce tls.renew after failed web fetch and tighten gate cache doc wording) and update deploy/container/setup.json, README, and lnx2.sh accordingly (FC-017, FC-164).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 tls.renew is re-announced after a failed web fetch and gate cache wording is updated.
- [ ] #2 Server and container deployments start cleanly over HTTP by default between browser and office without requiring TLS setup up front.
- [ ] #3 Teams can enable HTTPS opt-in from the app (via /api/v1 instance TLS settings endpoint + UI settings control) as well as admin tls set on macOS/Linux/containers.
<!-- AC:END -->
