---
id: TASK-37
title: 'O1: Offices in the product — live office list, readiness, and host restart'
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
documentation:
  - docs/implementation-plan.md
  - docs/design/offices-hq-and-satellites.md
  - docs/specs/offices-and-teams-ux.md
priority: high
type: feature
ordinal: 37000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Expose the live HQ office list, readiness/freshness state reported by host and office, and authorized office restart through /api/v1 and CLI for single-office v1 milestone 1 (FC-019, FC-021).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Office list and readiness endpoints return live HQ office state via /api/v1 and CLI.
- [ ] #2 Administrator or organization owner can trigger an office restart through /api/v1 and verify it returns ready.
- [ ] #3 Single-office OFC acceptance criteria pass against a real instance and FC-019/FC-021 are updated.
<!-- AC:END -->
