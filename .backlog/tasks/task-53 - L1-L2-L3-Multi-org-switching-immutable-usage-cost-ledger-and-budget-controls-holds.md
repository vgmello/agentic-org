---
id: TASK-53
title: >-
  L1, L2 & L3: Multi-org switching, immutable usage/cost ledger, and budget
  controls/holds
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T12:21:00Z'
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
- [x] #1 Users switch cleanly between multiple organizations on one instance (OPEN criteria, FC-015).
- [x] #2 Immutable usage ledger, historical attribution, versioned USD pricing, account timezone queries, and CSV export pass verification (FC-008, FC-084..088).
- [x] #3 Task, agent daily/monthly, and release budgets enforce 80/100 notices, independent holds, raises, ASK-8, and own-agent budget CLI (FC-089..093).
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed L1/L2/L3 backend across org-proto, org-host-control, org-host, org-office, org-office-sessions, org-office-server, agentic-org-web, and org-agent-cli: persisted account time_zone & last_org_id (POST /api/v1/account/preferences + account.changed invalidation), durable accounting/usage/*.jsonl + isolated 0700/0600 protected/accounting.db (delete+rebuild), versioned USD pricing, leaf attribution corrections, DST-aware IANA timezone queries & formula-guarded RFC 4180 CSV export, durable budgets/*.json + isolated 0700/0600 protected/budgets.db (delete+rebuild), task/agent/release budgets, 80/100 notices, one-time +20% extensions, independent stop_pending->held->released holds & session wake gating, owner Inbox notices, and agent-os budget status CLI. Verified across all 8 crates, test_schema.py (118/118), workspace clippy (0 warnings), Repowise staged health (exit 0), and 4-pass fresh Neon reviews ending in VERDICT: APPROVE (0 High, 0 Medium). Remaining browser UI screen wiring tracked in UI track (FC-008, FC-015, FC-086..088).
<!-- SECTION:NOTES:END -->
