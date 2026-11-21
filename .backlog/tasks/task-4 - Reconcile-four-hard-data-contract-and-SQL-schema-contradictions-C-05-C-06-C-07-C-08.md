---
id: TASK-4
title: >-
  Reconcile four hard data-contract and SQL schema contradictions (C-05, C-06,
  C-07, C-08)
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:34:00Z'
labels:
  - architecture
  - data-model
  - p0
milestone: m-2
dependencies: []
references:
  - docs/design/organization-data/schema.sql
  - docs/design/organization-data/test_schema.py
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/01-organization-structure.md
  - docs/design/organization-data/02-project-structure.md
  - docs/design/organization-data/04-agent-structure.md
  - docs/design/organization-data/09-release-content-and-flags.md
  - docs/design/organization-data/10-usage-and-cost-accounting.md
  - docs/design/organization-data/15-messaging-and-chat.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
  - docs/design/organization-data/17-budgets-and-controls.md
priority: high
type: bug
ordinal: 4000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings C-05, C-06, C-07, C-08: Four verified contradictions currently block unambiguous implementation: (1) Doc 04:109 vs Doc 04:153 vs Doc 01:139 conflict on instruction authority precedence between owner, cross-team reports_to_employee_id (person/agent), and ancestor team leads; (2) release_progress ignores explicit release_tasks whenever milestones exist, disagreeing with task_releases and falsely reporting incomplete releases as done; (3) Doc 16:238-243 claims accounting/ and messaging/ never leave an office, flatly contradicting Doc 10 and Doc 15 replication; (4) Doc 17 requires supporting owner timezone changes, which is blocked by budget_target_immutable, budget_agent_identity, and budgets_no_delete triggers.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Doc 04 and Non-Negotiable #9 publish one unambiguous authority-precedence rule covering owner, reports_to chain (person vs agent), and ancestor team leads, with conflicting prose removed.
- [x] #2 release_progress computes totals and is_done from the deduplicated union of milestone-linked tasks and explicit release_tasks, matching task_releases in SQLite tests.
- [x] #3 Doc 16 classifies storage trees cleanly into Git-published (state/), office-replicated over mTLS (messaging/, accounting/, selected authority/), and strictly node-local (local/, recovery/, private/), matching Docs 10 and 15.
- [x] #4 Owner timezone changes succeed against schema.sql while preserving historical budget_period boundaries, verified by new test_schema.py cases.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. C-05: Publish one unambiguous authority-precedence rule in Doc 04 (04-agent-structure.md), Doc 01, and Non-Negotiable #9 covering owner, reports_to chain (person vs agent), and ancestor team-lead chain, removing conflicting prose in Doc 04:109.
2. C-06: Rewrite release_progress in Doc 02 (02-project-structure.md) and schema.sql to compute totals and is_done from the single-pass deduplicated union in task_releases while preserving the empty-milestone open rule, and add regression test in test_schema.py proving mixed milestone + explicit release_tasks releases work accurately.
3. C-07: Update Doc 16 (16-recovery-and-office-continuity.md §What leaves an office) to cleanly distinguish Git-published (state/), office-to-HQ/peer mTLS replicated (messaging/, accounting/, selected authority/), and strictly node-local (local/, recovery/, private/), matching Docs 10 and 15.
4. C-08: Allow owner timezone transitions in Doc 17 (17-budgets-and-controls.md) and schema.sql by storing time_zone / time_zone_source / time_zone_subject_ref on budget_policy_versions (and snapshotting effective bounds on budget_periods), allowing budgets.time_zone* to advance with a new revision while preserving historical periods, updating Mermaid ERD and adding test_schema.py regression tests.
5. Run test_schema.py and cargo test -p org-office -- store::sqlite, finalize TASK-4, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Resolved all four core contradictions: (C-05) Published canonical instruction-precedence table in Doc 04 (owner -> reports_to_employee_id -> ancestor team lead -> others) distinct from office structural authority, updated Non-Negotiable #9; (C-06) Rewrote release_progress in Doc 02 and schema.sql using single-pass CTEs over task_releases and milestone_progress (preserving empty-milestone open rule); (C-07) Updated Doc 16 §What leaves an office into 3 classes (Git state/, mTLS replicated messaging/accounting/authority, strictly node-local local/recovery/private/budgets) matching Docs 10 & 15; (C-08) Added time_zone/time_zone_source/time_zone_subject_ref to budget_policy_versions and relaxed budget_target_immutable to permit timezone changes when current_revision strictly advances while keeping prior budget_periods immutable. Verified test_schema.py (113/113 PASS) and cargo test -p org-office -- store::sqlite (26/26 PASS).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reconciled C-05 (authority precedence), C-06 (release_progress vs task_releases), C-07 (Doc 16 vs Doc 10/15 replication classes), and C-08 (Doc 17 owner timezone transitions) with SQL/doc/Mermaid parity and regression tests.
<!-- SECTION:FINAL_SUMMARY:END -->
