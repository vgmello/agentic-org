---
id: TASK-8
title: >-
  Tighten remaining schema constraints, private-notes-schema.sql integrity, and
  task/worktree redundancies
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - data-model
  - p1
milestone: m-3
dependencies:
  - TASK-7
references:
  - docs/design/organization-data/schema.sql
  - docs/design/organization-data/private-notes-schema.sql
  - docs/design/organization-data/test_schema.py
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/03-features-and-work.md
  - docs/design/organization-data/05-operational-data.md
  - docs/design/organization-data/13-private-task-notes.md
priority: medium
type: enhancement
ordinal: 8000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings M-07, M-08, M-09, L-03: Several secondary schema invariants lag prose or introduce unnecessary redundancy: 2-cycles are allowed in features.replaced_by_feature_id and deployments.rollback_of_id; one_project_brief, documents.key, structure_documents.key, and dev-process positions lack uniqueness; hire_requests/office_codes/counters lack immutability/monotonicity guards; private-notes-schema.sql allows invalid initial revision/deleted states, duplicate shared_comment_id receipts, and non-monotonic is_deleted flips; task_details.branch duplicates task_repositories.branch; closure reasons overlap; task_events triples JSON+BLOB storage; task_workspaces PK blocks two agents (author + reviewer) from holding worktrees on one task; slug/handle/sha CHECK patterns are inconsistent.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Missing cycle, uniqueness, immutability, monotonicity, and consistent slug/handle/key/sha CHECK constraints are enforced in SQL (or explicitly documented as service-layer invariants in SCHEMA-COVERAGE.md so prose and SQL match).
- [ ] #2 private-notes-schema.sql rejects initial inserts with revision > 1 or deleted state, enforces UNIQUE(organization_id, shared_comment_id) and no-replace on publish receipts, and prevents is_deleted flipping from 1 to 0.
- [ ] #3 Redundant task_details.branch and overlapping closure reason fields are consolidated, task_events avoids triple JSON/BLOB duplication in SQLite, and task_workspaces PK includes employee_id so multiple agents can hold worktrees on one task.
- [ ] #4 All updated invariants are verified in test_schema.py.
<!-- AC:END -->
