---
id: TASK-7
title: >-
  Close core SQL integrity, FK, and immutability holes across shared schema
  tables
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - data-model
  - p1
milestone: m-3
dependencies:
  - TASK-4
references:
  - docs/design/organization-data/schema.sql
  - docs/design/organization-data/test_schema.py
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/03-features-and-work.md
  - docs/design/organization-data/06-identity-and-numbering.md
  - docs/design/organization-data/17-budgets-and-controls.md
  - >-
    docs/design/organization-data/18-development-processes-and-project-documentation.md
priority: high
type: bug
ordinal: 7000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings H-08, H-11, H-12, H-14: Live SQLite verification found four integrity holes in schema.sql: (1) budgets.task_id FKs live tasks instead of record_refs while budgets_no_delete blocks deletion, preventing tombstoning of any task that ever had a budget; (2) task_affects_feature_same_project_insert/update can be bypassed via deferred inserts or updating task_details.task_id / feature.project_id; (3) document_versions omits a record_refs FK, project_id superkey check, and BEFORE INSERT no-replace guard, while document_version_sources allows UPDATE/DELETE on immutable provenance; (4) tombstones omits office_code, record_refs/tombstones/links lack CHECK(kind IN (...)), release_tasks drops same-project enforcement, and operational children can attach to tombstoned tasks.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 budgets.task_id references record_refs(organization_id, kind, id) so budgeted tasks can be tombstoned cleanly.
- [ ] #2 Cross-project affects_feature_id links are rejected across deferred inserts, task_details.task_id updates, and feature/task project updates.
- [ ] #3 document_versions enforces FK to record_refs, same-project superkey invariants, and no-replace protection; document_version_sources aborts UPDATE and DELETE.
- [ ] #4 tombstones includes office_code; record_refs.kind, tombstones.kind, and links.target_kind enforce allowed entity kinds; release_tasks enforces same-project membership; new operational children on tombstoned tasks are rejected.
- [ ] #5 test_schema.py includes negative regression tests proving every closed hole is enforced.
<!-- AC:END -->
