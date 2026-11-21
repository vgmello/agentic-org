---
id: TASK-6
title: >-
  Add missing hot-path and ADR-0031 SQL indexes and fix slow usage_*_totals and
  release_progress views
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
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
- [ ] #1 Targeted composite indexes exist for private_task_notes(organization_id, task_id), task_document_versions(organization_id, document_id, version), tasks(organization_id, feature_id), document_code_links(organization_id, document_id), document_version_sources(organization_id, source_document_id, source_version), hire_requests(organization_id, home_team_id, state), messages(organization_id, thread_root_id, created_at), usage_records(organization_id, release_id, occurred_at), active budget_holds, and high-fanout FKs targeting employees, tasks, and documents.
- [ ] #2 usage_{task,agent,project}_totals pre-aggregate org stream coverage once via CTE/JOIN instead of per-group correlated subqueries.
- [ ] #3 release_progress executes over a single CTE scan without redundant milestone_progress co-routine evaluations.
- [ ] #4 EXPLAIN QUERY PLAN checks in test_schema.py confirm indexed lookups and eliminate quadratic/3x view scans.
<!-- AC:END -->
