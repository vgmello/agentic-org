---
id: TASK-19
title: >-
  Synchronize SCHEMA-COVERAGE.md, project-structure.md, executable-migration.md,
  ADRs, and cross-document data-contract prose
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:40:00Z'
labels:
  - docs
  - architecture
  - p2
milestone: m-4
dependencies:
  - TASK-4
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/SCHEMA-COVERAGE.md
  - docs/design/project-structure.md
  - docs/design/executable-migration.md
  - docs/design/organization-data/STATUS.md
priority: medium
type: docs
ordinal: 19000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings H-02, M-10, M-12 (docs), L-06: Multiple authoritative docs contradict current code or newer ADRs: SCHEMA-COVERAGE.md still claims 0/91 tables implemented while 10 org tables are live on verbatim schema.sql; project-structure.md omits org-platform-host and org-command; executable-migration.md claims web serves only /version (9 routes live); ADRs 0002 and 0006 contradict ADR-0031; ADRs 0007 and 0011 omit v2 deferral; ADR-0027 retains 65 lines of superseded hiring approval rules; ADRs 0008, 0009, 0013, and 0017 still show 'Implementation pending'; and Docs 02, 05, 11, 12, 14, 15, 16, 17, README.md, and STATUS.md contain verified stale/contradictory text.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 SCHEMA-COVERAGE.md accurately marks the 10 active organization tables Implemented, removes deleted org.db/office.db notes, and documents the 4 store_* adapter tables.
- [x] #2 project-structure.md includes org-platform-host and org-command, and executable-migration.md accurately reflects current implemented routes/entry points.
- [x] #3 ADRs 0002, 0006, 0007, 0011, 0027, 0008, 0009, 0013, and 0017 are reconciled with ADR-0031, ADR-0029, v2 deferrals, and implemented status.
- [x] #4 Stale prose items cited in audit finding M-10 across Docs 02, 05, 11, 12, 14, 15, 16, 17, README.md, and STATUS.md are corrected in one synchronized pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Update SCHEMA-COVERAGE.md to mark the 10 active organization tables Implemented on verbatim schema.sql, remove deleted org.db/office.db notes, and document the 4 store_* adapter tables (H-02).
2. Update project-structure.md (add org-platform-host and org-command) and executable-migration.md (reflect live agentic-org-web 9 host routes and agentic-org setup) (M-12).
3. Reconcile ADRs 0002 (line 105 vs ADR-0031), 0006 (line 294 crate name & line 327 vs ADR-0031), 0007 & 0011 (v2 deferral status), 0027 (remove superseded approval prose beside ADR-0029), and 0008/0009/0013/0017 (Implemented status headers) (M-12, L-06).
4. Fix M-10 cross-document stale prose across Docs 02, 05, 11, 12, 14, 15, 16, 17, README.md, and STATUS.md.
5. Run test_schema.py, finalize TASK-19, and commit (unblocking TASK-22..TASK-27).
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Verified python3 -B docs/design/organization-data/test_schema.py (all tests passing, including coverage, SQL sync, and relative links).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Synchronized SCHEMA-COVERAGE.md (10 active org tables Implemented, legacy org.db/office.db notes removed, 4 store_* adapter tables documented), project-structure.md (added org-platform-host, org-host-control, org-agent-runtime-claude, updated current code state), executable-migration.md (live agentic-org-web 9 host routes and B1b/B2.1 office store state), ADRs 0002/0006/0007/0008/0009/0011/0013/0017/0027 (ADR-0031 private-note/task rules, ADR-0029 hiring supersession, v2 deferrals, implemented headers), and all M-10 cross-document stale prose items across Docs 02, 05, 11, 12, 14, 15, 16, 17, README.md, and STATUS.md. Unblocks TASK-22 through TASK-27.
<!-- SECTION:FINAL_SUMMARY:END -->
