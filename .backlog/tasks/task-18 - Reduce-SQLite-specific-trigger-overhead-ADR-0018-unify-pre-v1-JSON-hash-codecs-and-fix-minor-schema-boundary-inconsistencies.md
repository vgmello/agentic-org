---
id: TASK-18
title: >-
  Reduce SQLite-specific trigger overhead (ADR-0018), unify pre-v1 JSON hash
  codecs, and fix minor schema boundary inconsistencies
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T01:04:00Z'
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
- [x] #1 Redundant BEFORE INSERT duplicate-key triggers are removed and pure admission/immutability checks are evaluated for migration into the single Rust writer/projector while keeping declarative PK/FK/UNIQUE/CHECK constraints in SQL.
- [x] #2 Pre-v1 canonical JSON hashing is consolidated onto a single codec in Doc 08 and corresponding Rust codecs/test vectors.
- [x] #3 budgets.time_zone_subject_ref references an org-scoped person employee_id (or org timezone setting) rather than a host account ref, and org_epoch > 0 is consistent across tables.
- [x] #4 test_schema.py and workspace Rust tests pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. M-06: Remove redundant BEFORE INSERT duplicate-key triggers (receipt_no_replace, origin_stream_no_replace, checkpoint_no_replace) from Doc 16 and schema.sql (updating test_schema.py assertions to verify PK/DELETE guards still reject INSERT and INSERT OR REPLACE), and document declarative-vs-writer invariant ownership for ADR-0018 PostgreSQL portability in SCHEMA-COVERAGE.md.
2. L-01: Unify pre-v1 JSON hash codecs onto canonical-json/2 (+ raw-bytes/1 for raw blobs) across Doc 08, Doc 10, and Doc 15, retiring redundant sorted-compact-json/1 and jcs-rfc8785/1 pre-v1.
3. L-02: Replace host account ref in budgets/budget_policy_versions.time_zone_subject_ref with org-scoped owner_person employee_id FK (Doc 17 + schema.sql + test_schema.py), and align messages.org_epoch > 0 (Doc 15 + schema.sql).
4. Verify test_schema.py + cargo test -p org-office -- store::sqlite, finalize TASK-18, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Verified python3 -B docs/design/organization-data/test_schema.py (116/116 PASS) and cargo test -p org-office -- store::sqlite (26/26 PASS).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reduced redundant SQLite triggers, unified pre-v1 JSON hash codecs, and fixed schema boundary leaks (M-06, L-01, L-02): (1) removed redundant BEFORE INSERT duplicate-key triggers (receipt_no_replace, origin_stream_no_replace, checkpoint_no_replace) while retaining PK + BEFORE DELETE protection with recursive_triggers=ON, and documented declarative-vs-writer invariant ownership for ADR-0018 PostgreSQL portability in SCHEMA-COVERAGE.md; (2) unified pre-v1 JSON hash codecs onto canonical-json/2 (+ raw-bytes/1 for raw blobs) across Docs 08, 10, and 15, retiring redundant sorted-compact-json/1 and jcs-rfc8785/1; (3) replaced host account refs in budgets/budget_policy_versions.time_zone_subject_ref with org-scoped owner_person employee_id FKs (Docs 04, 17, schema.sql) and aligned messages/message_recipient_states.org_epoch > 0 (Doc 15, schema.sql) with regression tests in test_schema.py.
<!-- SECTION:FINAL_SUMMARY:END -->
