---
id: TASK-52
title: >-
  S1: Milestone 3 — Second office enrollment, replication, cross-office relay,
  and HQ re-enrollment
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - sync
  - m3
milestone: m-0
dependencies:
  - TASK-45
  - TASK-49
  - TASK-42
  - TASK-39
documentation:
  - docs/implementation-plan.md
  - docs/design/offices-hq-and-satellites.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
priority: medium
type: feature
ordinal: 52000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Deliver multi-office v1 milestone 3: satellite office invitation and check-code approval, per-office storage roots, durable replication between HQ and satellites using org-sync (ENG-3 replication suite), cross-office message relay through HQ, one-command satellite re-enrollment after HQ replacement (owner D25), and full fresh-HQ + satellite release acceptance (FC-020, FC-024, FC-145).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A second office enrolls on another machine via invitation and check code and runs agents offline while HQ is unreachable.
- [ ] #2 org-sync durable replication passes the ADR-0018 ENG-3 replication suite and relays messages between offices through HQ.
- [ ] #3 After a lost HQ is imported via R1, satellite offices re-enroll with one command (D25), and FC-020/024/145 are updated.
<!-- AC:END -->
