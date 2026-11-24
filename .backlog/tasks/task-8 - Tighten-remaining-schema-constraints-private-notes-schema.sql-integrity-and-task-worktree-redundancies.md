---
id: TASK-8
title: >-
  Tighten remaining schema constraints, private-notes-schema.sql integrity, and
  task/worktree redundancies
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:57:00Z'
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
- [x] #1 Missing cycle, uniqueness, immutability, monotonicity, and consistent slug/handle/key/sha CHECK constraints are enforced in SQL (or explicitly documented as service-layer invariants in SCHEMA-COVERAGE.md so prose and SQL match).
- [x] #2 private-notes-schema.sql rejects initial inserts with revision > 1 or deleted state, enforces UNIQUE(organization_id, shared_comment_id) and no-replace on publish receipts, and prevents is_deleted flipping from 1 to 0.
- [x] #3 Redundant task_details.branch and overlapping closure reason fields are consolidated, task_events avoids triple JSON/BLOB duplication in SQLite, and task_workspaces PK includes employee_id so multiple agents can hold worktrees on one task.
- [x] #4 All updated invariants are verified in test_schema.py.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. M-07: Add cycle triggers for features.replaced_by_feature_id and deployments.rollback_of_id; add partial UNIQUE indexes one_project_brief, documents_key_unique, structure_documents_key_unique; add hire_requests agent-only and terminal immutability triggers; add office_codes immutability/no-delete and reference_counters/office_task_counters monotonicity triggers; add state-pair CHECKs on checklist_steps/owner_questions/waiting_spans.
2. M-08: Tighten private-notes-schema.sql with initial revision=1/non-deleted guard, UNIQUE(organization_id, shared_comment_id), BEFORE INSERT no-replace on receipts, and monotonic private_note_tasks.is_deleted guard.
3. M-09: Remove redundant task_details.branch and undocumented task_details.decline_reason (updating Doc 03 SQL+Mermaid+prose and schema.sql); widen task_workspaces PK to include employee_id (Doc 04 SQL+Mermaid+schema.sql); make task_events.event_bytes optional/nullable in SQLite projection so SQLite avoids mandatory 3x JSON+BLOB duplication while keeping canonical bytes on disk (updating Doc 05+schema.sql).
4. L-03: Align slug/handle/key/kind/sha CHECK patterns across organizations, offices, roles, feature_flags, employees, documents, release_repository_commits, and add UNIQUE(organization_id, process_id, position) on development_process_{roles,skills,folders}.
5. Verify test_schema.py + cargo test -p org-office -- store::sqlite, finalize TASK-8, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Verified python3 -B docs/design/organization-data/test_schema.py (116/116 PASS) and cargo test -p org-office -- store::sqlite (26/26 PASS).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Tightened remaining schema constraints, private-notes-schema.sql integrity, and data-model redundancies (M-07, M-08, M-09, L-03) across schema.sql, private-notes-schema.sql, and synchronized Docs 01, 02, 03, 04, 05, 09, 18: (1) added recursive cycle guards on features.replaced_by_feature_id and deployments.rollback_of_id, partial UNIQUE indexes one_project_brief / documents_key_unique / structure_documents_key_unique, hire_requests agent-only and terminal-immutability triggers, office_codes immutability/no-delete and reference_counters/office_task_counters monotonicity triggers, and state-pair/timestamp-order CHECKs on checklist_steps/owner_questions/waiting_spans; (2) tightened private-notes-schema.sql with initial revision=1/non-deleted guard, UNIQUE(organization_id, shared_comment_id), BEFORE INSERT no-replace on receipts, create/delete receipt matching, and monotonic private_note_tasks.is_deleted guard; (3) removed redundant task_details.branch and undocumented decline_reason, widened task_workspaces PK to include employee_id, and made task_events.event_bytes optional in SQLite projection; (4) aligned slug/handle/key/kind/sha CHECK patterns and added UNIQUE(organization_id, process_id, position) on dev-process roles/skills/folders. Added test_task_8_secondary_constraints_and_redundancies in test_schema.py.
<!-- SECTION:FINAL_SUMMARY:END -->
