---
id: TASK-54
title: >-
  L4: Provider subscription pools, per-connection model discovery, quota
  permits, cooldown, and failover
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - agents
  - late-v1
milestone: m-0
dependencies:
  - TASK-2
  - TASK-42
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/11-model-profiles-and-agent-execution.md
  - docs/design/organization-data/14-model-providers-and-connection-pools.md
priority: medium
type: feature
ordinal: 54000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement ordered provider connection pools, per-connection endpoint_url/env plumbing and model discovery, HQ quota permit/cooldown coordination, connection-switch stop/relaunch failover, and remaining ASK-4/LIVE-2 turn-permit rules (FC-098, FC-099).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Ordered provider connection pools, bindings, endpoint_url plumbing, and model discovery work per office (FC-098).
- [ ] #2 Shared quota permits, rate-limit cooldowns, and connection failover operate cleanly while preserving offline execution per TASK-2 (FC-099).
- [ ] #3 Remaining turn-permit clauses of ASK-4 and LIVE-2 pass.
<!-- AC:END -->
