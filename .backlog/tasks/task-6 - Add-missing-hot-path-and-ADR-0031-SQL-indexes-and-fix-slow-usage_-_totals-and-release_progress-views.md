---
id: TASK-6
title: >-
  Add missing hot-path and ADR-0031 SQL indexes and fix slow usage_*_totals and
  release_progress views
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:44:00Z'
labels:
  - data-model
  - performance
  - p1
milestone: m-3
dependencies:
  - TASK-4
references:
  - docs/design/organization-data/schema.sql
  - docs/design/organization-data/private-notes-schema.sql
  - docs/design/organization-data/test_schema.py
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/10-usage-and-cost-accounting.md
  - docs/design/organization-data/02-project-structure.md
priority: high
type: enhancement
ordinal: 6000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings H-10, H-13: SQLite introspection identified 169 unindexed non-trivial FKs and 7 missing critical composite indexes (including private_task_notes(org, task_id) for ADR-0031 person reads and usage_records(org, release_id, occurred_at) for release budgets), causing full-org scans on common reads and parent updates. In addition, usage_{task,agent,project}_totals run O(groups * rows) correlated subqueries over usage_stream_coverage and release_progress invokes the milestone_progress co-routine 3 times per query.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Targeted composite indexes exist for private_task_notes(organization_id, task_id), task_document_versions(organization_id, document_id, version), tasks(organization_id, feature_id), document_code_links(organization_id, document_id), document_version_sources(organization_id, source_document_id, source_version), hire_requests(organization_id, home_team_id, state), messages(organization_id, thread_root_id, created_at), usage_records(organization_id, release_id, occurred_at), active budget_holds, and high-fanout FKs targeting employees, tasks, and documents.
- [x] #2 usage_{task,agent,project}_totals pre-aggregate org stream coverage once via CTE/JOIN instead of per-group correlated subqueries.
- [x] #3 release_progress executes over a single CTE scan without redundant milestone_progress co-routine evaluations.
- [x] #4 EXPLAIN QUERY PLAN checks in test_schema.py confirm indexed lookups and eliminate quadratic/3x view scans.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Inspect H-10 and H-13 audit details, schema.sql views/indexes, Doc 10 SQL, and private-notes-schema.sql.
2. Add targeted composite and high-fanout FK indexes to schema.sql, private-notes-schema.sql, and corresponding numbered docs (03, 04, 05, 10, 15, 17, 18) so test_consolidated_tables_and_views_match_structural_documents stays exact.
3. Rewrite usage_task_totals, usage_agent_totals, and usage_project_totals in Doc 10 and schema.sql to pre-aggregate usage_stream_coverage once via CTE/JOIN instead of correlated per-group subqueries.
4. Add EXPLAIN QUERY PLAN assertions in test_schema.py for all new indexes, usage_*_totals, and release_progress.
5. Verify test_schema.py and cargo test -p org-office -- store::sqlite, finalize TASK-6, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Verified python3 -B docs/design/organization-data/test_schema.py (114/114 PASS including EXPLAIN QUERY PLAN assertions) and cargo test -p org-office -- store::sqlite (26/26 PASS).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added targeted covering/composite hot-path indexes (including private_notes_by_task for ADR-0031 person reads, usage_by_release_time, active_budget_holds, tasks_by_feature, task_document_versions_by_document, document_code_links_by_document, document_version_sources_by_source, hire_requests_by_team_state, messages_by_thread) and 17 high-fanout FK indexes across employees/tasks/documents/private receipts. Rewrote usage_task_totals, usage_agent_totals, and usage_project_totals in Doc 10 and schema.sql to materialize org_incomplete_streams once via CTE/JOIN instead of per-group correlated subqueries. Added EXPLAIN QUERY PLAN regression test verifying all hot-path indexes, non-correlated usage_*_totals, and single-pass release_progress.
<!-- SECTION:FINAL_SUMMARY:END -->
