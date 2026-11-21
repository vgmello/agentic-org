---
id: TASK-4
title: >-
  Reconcile four hard data-contract and SQL schema contradictions (C-05, C-06,
  C-07, C-08)
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
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
- [ ] #1 Doc 04 and Non-Negotiable #9 publish one unambiguous authority-precedence rule covering owner, reports_to chain (person vs agent), and ancestor team leads, with conflicting prose removed.
- [ ] #2 release_progress computes totals and is_done from the deduplicated union of milestone-linked tasks and explicit release_tasks, matching task_releases in SQLite tests.
- [ ] #3 Doc 16 classifies storage trees cleanly into Git-published (state/), office-replicated over mTLS (messaging/, accounting/, selected authority/), and strictly node-local (local/, recovery/, private/), matching Docs 10 and 15.
- [ ] #4 Owner timezone changes succeed against schema.sql while preserving historical budget_period boundaries, verified by new test_schema.py cases.
<!-- AC:END -->
