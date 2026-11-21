---
id: TASK-19
title: >-
  Synchronize SCHEMA-COVERAGE.md, project-structure.md, executable-migration.md,
  ADRs, and cross-document data-contract prose
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
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
- [ ] #1 SCHEMA-COVERAGE.md accurately marks the 10 active organization tables Implemented, removes deleted org.db/office.db notes, and documents the 4 store_* adapter tables.
- [ ] #2 project-structure.md includes org-platform-host and org-command, and executable-migration.md accurately reflects current implemented routes/entry points.
- [ ] #3 ADRs 0002, 0006, 0007, 0011, 0027, 0008, 0009, 0013, and 0017 are reconciled with ADR-0031, ADR-0029, v2 deferrals, and implemented status.
- [ ] #4 Stale prose items cited in audit finding M-10 across Docs 02, 05, 11, 12, 14, 15, 16, 17, README.md, and STATUS.md are corrected in one synchronized pass.
<!-- AC:END -->
