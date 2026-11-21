---
id: TASK-33
title: 'B1c: Measure and guard office store latency and engine independence'
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - performance
  - m1
milestone: m-0
dependencies:
  - TASK-1
documentation:
  - docs/implementation-plan.md
  - docs/design/office-store.md
  - docs/adr/0014-acknowledgement-classes-and-agent-os-latency.md
  - docs/adr/0018-storage-engine-independence.md
priority: medium
type: enhancement
ordinal: 33000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Measure office store latency on Linux CI and reference Mac, fix the flush interval from measurements, guard regressions in CI, surface persistent SQLite checkpoint failures instead of dropping errors, and close ADR-0018 ENG-4/ENG-5 and receipt retention/pruning follow-ups (FC-160, FC-165).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Office store benchmark target runs on Linux CI and reference Mac with numbers recorded in ADR-0014 and a CI regression check enabled.
- [ ] #2 ACK-2, ACK-3, ACK-5, ACK-6, and ACK-7 pass for organization operations and the flush-bound load test is verified.
- [ ] #3 Repeated SQLite checkpoint failures are surfaced rather than silently dropped, and receipt retention/pruning bounds are documented or implemented.
- [ ] #4 ENG-4 and ENG-5 wording and trigger/generated-column coverage checks pass across docs and schema coverage.
<!-- AC:END -->
