---
id: TASK-53
title: >-
  L1, L2 & L3: Multi-org switching, immutable usage/cost ledger, and budget
  controls/holds
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - costs
  - late-v1
milestone: m-0
dependencies:
  - TASK-6
  - TASK-10
  - TASK-42
  - TASK-49
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/10-usage-and-cost-accounting.md
  - docs/design/organization-data/17-budgets-and-controls.md
  - docs/specs/costs-and-insights-ux.md
priority: medium
type: feature
ordinal: 53000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Deliver multi-organization switching on one instance (L1, FC-015), durable usage ledger with versioned pricing, account timezone persistence, scoped cost views, and CSV export (L2, FC-008, FC-084..088), and task/agent/release budgets with 80/100 notices, independent execution holds, raises, and own-agent CLI (L3, FC-089..093, ASK-8, budget part of LIVE-3).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Users switch cleanly between multiple organizations on one instance (OPEN criteria, FC-015).
- [ ] #2 Immutable usage ledger, historical attribution, versioned USD pricing, account timezone queries, and CSV export pass verification (FC-008, FC-084..088).
- [ ] #3 Task, agent daily/monthly, and release budgets enforce 80/100 notices, independent holds, raises, ASK-8, and own-agent budget CLI (FC-089..093).
<!-- AC:END -->
