---
id: TASK-25
title: >-
  Review and migrate core Organization Data contracts (15 files: README, STATUS,
  SCHEMA-COVERAGE, Docs 01-09, 12, 13, 18) to .backlog/docs/
status: Done
assignee: []
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T01:30:00Z'
labels:
  - docs
  - data-model
  - backlog-migration
milestone: m-5
dependencies:
  - TASK-4
  - TASK-19
documentation:
  - docs/design/organization-data/README.md
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 25000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Review each core Organization Data specification against current schema.sql, private-notes-schema.sql, and audit findings, then migrate all 15 Markdown files into .backlog/docs/organization-data/ via `backlog doc create`.

Files in scope (15):
- docs/design/organization-data/README.md
- docs/design/organization-data/STATUS.md
- docs/design/organization-data/SCHEMA-COVERAGE.md
- docs/design/organization-data/01-organization-structure.md
- docs/design/organization-data/02-project-structure.md
- docs/design/organization-data/03-features-and-work.md
- docs/design/organization-data/04-agent-structure.md
- docs/design/organization-data/05-operational-data.md
- docs/design/organization-data/06-identity-and-numbering.md
- docs/design/organization-data/07-release-deployments.md
- docs/design/organization-data/08-record-contracts.md
- docs/design/organization-data/09-release-content-and-flags.md
- docs/design/organization-data/12-task-assignment-and-history.md
- docs/design/organization-data/13-private-task-notes.md
- docs/design/organization-data/18-development-processes-and-project-documentation.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All 15 core organization-data Markdown contracts are individually reviewed against current SQL and resolved P0/P2 doc-sync fixes.
- [x] #2 All 15 documents are migrated into .backlog/docs/organization-data/ via `backlog doc create` with document type specification/readme and preserved cross-references.
- [x] #3 No contradictions or stale coverage metrics are carried into the migrated Backlog docs.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed all 15 core organization-data contracts against current SQL and resolved TASK-2..19 fixes (caught and fixed stale 91-table count in README.md, stale CA-key Entry 1 note in Doc 01, and added set rev 2.24 in STATUS.md) and migrated all 15 into .backlog/docs/organization-data/ via backlog doc create.
<!-- SECTION:FINAL_SUMMARY:END -->
