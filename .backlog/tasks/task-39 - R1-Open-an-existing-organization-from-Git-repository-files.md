---
id: TASK-39
title: 'R1: Open an existing organization from Git repository files'
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - recovery
  - m1
milestone: m-0
dependencies:
  - TASK-38
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
  - docs/design/accounts-sandbox-and-extensions.md
priority: high
type: feature
ordinal: 39000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement 'Open an existing organization' beside Create as v1's lost-HQ recovery path (owner D25): validate durable files against the data contract, register at the host under the existing UUIDv7 org ID with a new HQ CA, rebuild the office projection, bind the administrator to their person record as owner, and clearly state local-only loss boundaries (FC-167).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Deleting runtime/ and the host database and opening the organization from its Git repo restores all published teams, people, projects, and records.
- [ ] #2 Invalid organization files refuse the open and name the offending file.
- [ ] #3 Administrator selects their person record to become owner and sees an explicit summary of local-only data that did not come back.
- [ ] #4 FEATURES row FC-167 is updated with real-process verification evidence.
<!-- AC:END -->
