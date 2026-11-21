---
id: TASK-18
title: >-
  Reduce SQLite-specific trigger overhead (ADR-0018), unify pre-v1 JSON hash
  codecs, and fix minor schema boundary inconsistencies
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - data-model
  - architecture
  - p2
milestone: m-4
dependencies:
  - TASK-7
  - TASK-8
references:
  - docs/design/organization-data/schema.sql
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/adr/0018-storage-engine-independence.md
  - docs/design/organization-data/08-record-contracts.md
  - docs/design/organization-data/17-budgets-and-controls.md
priority: low
type: enhancement
ordinal: 18000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings M-06, L-01, L-02: schema.sql relies on 71 SQLite-specific triggers (recursive_triggers, GLOB, json_type, and redundant BEFORE INSERT duplicate-key checks) that hurt projection write latency and future PostgreSQL portability under ADR-0018. Doc 08 freezes 3 redundant JSON hash codecs pre-v1 despite Non-Negotiable #14, budgets.time_zone_subject_ref leaks host account refs into org data, and messages.org_epoch allows 0 while office_recovery_checkpoints.authority_epoch requires > 0.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Redundant BEFORE INSERT duplicate-key triggers are removed and pure admission/immutability checks are evaluated for migration into the single Rust writer/projector while keeping declarative PK/FK/UNIQUE/CHECK constraints in SQL.
- [ ] #2 Pre-v1 canonical JSON hashing is consolidated onto a single codec in Doc 08 and corresponding Rust codecs/test vectors.
- [ ] #3 budgets.time_zone_subject_ref references an org-scoped person employee_id (or org timezone setting) rather than a host account ref, and org_epoch > 0 is consistent across tables.
- [ ] #4 test_schema.py and workspace Rust tests pass.
<!-- AC:END -->
