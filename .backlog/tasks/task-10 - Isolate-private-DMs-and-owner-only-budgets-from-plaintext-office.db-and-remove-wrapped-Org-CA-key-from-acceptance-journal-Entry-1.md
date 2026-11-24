---
id: TASK-10
title: >-
  Isolate private DMs and owner-only budgets from plaintext office.db and remove
  wrapped Org CA key from acceptance journal Entry 1
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T01:26:00Z'
labels:
  - security
  - architecture
  - runtime
  - p1
milestone: m-3
dependencies: []
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/adr/0016-no-application-level-data-encryption.md
  - docs/design/organization-data/15-messaging-and-chat.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
  - docs/design/organization-data/17-budgets-and-controls.md
priority: high
type: enhancement
ordinal: 10000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings H-05, H-06: Under ADR-0016, runtime/<office-id>/office.db is unencrypted SQLite that mixes public org/task tables with private human DMs, owner-only agent budgets/spend, and inline supervisor_command_receipts.response_json—collapsing the isolation model established by task-notes.db. Separately, Doc 16:229-243 commits the wrapped Org CA private key (ca-key.v1.json) into Entry 1 of durable/recovery/ alongside public state/ files, forcing permanent special-case outbox exclusions and retaining key ciphertext in recovery segments indefinitely.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Protected messaging (private DMs) and owner-only budget projections are isolated from shared plaintext office.db behind explicit access-controlled boundaries matching private/task-notes.db.
- [x] #2 Inline supervisor_command_receipts.response_json is replaced with access-checked result_ref references.
- [x] #3 Wrapped Org CA private key material is written exclusively to local security storage outside durable/recovery/ acceptance journal segments.
- [x] #4 ADR-0016, Doc 15, Doc 16, Doc 17, and affected org-office/org-store tests reflect and enforce the updated boundaries.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Isolate Doc 15 messaging tables into dedicated messaging-schema.sql (runtime/<office-id>/protected/messaging.db) and Doc 17 budget tables into dedicated budgets-schema.sql (runtime/<office-id>/protected/budgets.db) matching private-notes-schema.sql, removing them from shared schema.sql and updating ADR-0016, Docs 15/16/17, SCHEMA-COVERAGE.md, and test_schema.py.
2. Note AC #2 (supervisor_command_receipts.response_json -> mutation_receipts with access-checked result_ref) was already completed in TASK-9 (52f023f3); verify and document in notes.
3. Once TASK-13 merges, remove wrapped Org CA key (ca-key.v1.json) from CreateOrganizationOp Entry 1 in crates/org-office so no CA key ciphertext ever enters durable/recovery/ acceptance journal segments; write it atomically to durable/local/offices/<office-id>/security/ca-key.v1.json with crash-safe retry recovery, update org-office tests and sqlite table-count test.
4. Run cargo fmt/clippy/test --workspace and test_schema.py, finalize TASK-10, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
AC #1: Isolated Doc 15 messaging tables into dedicated messaging-schema.sql (runtime/<office-id>/protected/messaging.db) and Doc 17 budget tables into dedicated budgets-schema.sql (runtime/<office-id>/protected/budgets.db) with read-only identity mirror tables matching private-notes-schema.sql, removing all 14 sensitive tables from shared plaintext schema.sql (now 76 shared tables + 12 views) and adding SQLite assertions in org-office that none exist in office.db. AC #2: supervisor_command_receipts.response_json was already eliminated in TASK-9 (52f023f3) by consolidating onto mutation_receipts with access-checked result_ref. AC #3: Removed ca-key.v1.json from CreateOrganizationOp Entry 1 in crates/org-office so wrapped Org CA private key ciphertext never enters durable/recovery/ acceptance journal segments; implemented atomic 2-phase local security write (stage_ca_key -> Entry 1 commit of public ca-escrow.v1.bin -> write_ca_key + remove_staged_ca_key) with full crash-point recovery and added recovery-directory scan assertions in office/tests.rs and store/open/tests/local.rs. AC #4: Updated ADR-0016, Docs 15/16/17, SCHEMA-COVERAGE.md, test_schema.py (116/116 PASS), org-office (272+6 PASS), and agentic-org-office hq_creation/launch (8/8 PASS).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Isolated private DMs (messaging-schema.sql -> protected/messaging.db) and owner-only budgets (budgets-schema.sql -> protected/budgets.db) out of shared plaintext office.db, confirmed inline supervisor_command_receipts.response_json elimination (TASK-9), and moved wrapped Org CA key storage exclusively outside durable/recovery/ acceptance journal segments with crash-safe local staging and full test verification.
<!-- SECTION:FINAL_SUMMARY:END -->
