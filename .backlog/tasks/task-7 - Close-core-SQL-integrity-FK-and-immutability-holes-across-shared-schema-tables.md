---
id: TASK-7
title: >-
  Close core SQL integrity, FK, and immutability holes across shared schema
  tables
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:51:00Z'
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
- [x] #1 budgets.task_id references record_refs(organization_id, kind, id) so budgeted tasks can be tombstoned cleanly.
- [x] #2 Cross-project affects_feature_id links are rejected across deferred inserts, task_details.task_id updates, and feature/task project updates.
- [x] #3 document_versions enforces FK to record_refs, same-project superkey invariants, and no-replace protection; document_version_sources aborts UPDATE and DELETE.
- [x] #4 tombstones includes office_code; record_refs.kind, tombstones.kind, and links.target_kind enforce allowed entity kinds; release_tasks enforces same-project membership; new operational children on tombstoned tasks are rejected.
- [x] #5 test_schema.py includes negative regression tests proving every closed hole is enforced.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. H-08: Add task_kind='task' to budgets (Doc 17 SQL+Mermaid+schema.sql) and FK (organization_id, task_kind, task_id) -> record_refs(organization_id, kind, id) so budgeted tasks can be tombstoned cleanly.
2. H-11: Broaden task_affects_feature_same_project_update to cover task_id, and add reciprocal AFTER INSERT/UPDATE triggers on tasks and features so deferred inserts and project updates cannot commit cross-project affects_feature_id links.
3. H-12: Add document_kind='document' + project_id to document_versions (Doc 18 SQL+Mermaid+schema.sql) with FK to record_refs and same-project superkey enforcement on document_version_sources/task_document_versions; add BEFORE INSERT no-replace trigger on document_versions and BEFORE UPDATE/DELETE abort triggers on document_version_sources; reconcile Doc 18:402/748/750 prose.
4. H-14: Add office_code to tombstones (Doc 03 SQL+Mermaid+schema.sql); add CHECK(kind IN (...)) on record_refs, tombstones, and links.target_kind; enforce same-project on release_tasks; add triggers rejecting new operational children (comments, checklist_steps, watchers, owner_questions, waiting_spans) on tombstoned tasks.
5. Add comprehensive negative regression tests in test_schema.py, verify test_schema.py + cargo test -p org-office -- store::sqlite, finalize TASK-7, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Verified python3 -B docs/design/organization-data/test_schema.py (115/115 PASS) and cargo test -p org-office -- store::sqlite (26/26 PASS).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Closed all four core SQL integrity holes (H-08, H-11, H-12, H-14) across schema.sql and synchronized Docs 02, 03, 08, 17, 18 (SQL + Mermaid + prose): (1) budgets.task_id now references record_refs via task_kind='task' so budgeted tasks can be tombstoned cleanly; (2) broadened task_affects_feature_same_project_update and added reciprocal AFTER INSERT/UPDATE triggers on tasks and features closing deferred-insert and project-move bypasses; (3) added record_refs FK, project_id superkey, BEFORE INSERT no-replace trigger, same-project source/task triggers on document_versions/document_version_sources/task_document_versions, and UPDATE/DELETE immutability triggers on document_version_sources; (4) added tombstones.office_code, enforced CHECK(kind IN (...)) on record_refs/tombstones/links, enforced same-project on release_tasks, and blocked new operational children on tombstoned tasks. Added comprehensive regression test test_task_7_integrity_holes_closed in test_schema.py.
<!-- SECTION:FINAL_SUMMARY:END -->
