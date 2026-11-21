---
id: TASK-26
title: >-
  Review and migrate runtime/ops Organization Data contracts (Docs 10, 11,
  14-17) and handle executable schema assets (9 files)
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
  - TASK-2
  - TASK-4
  - TASK-19
references:
  - crates/org-office/src/store/sqlite/schema.rs
  - crates/org-office/src/store/sqlite/spike.rs
documentation:
  - docs/design/organization-data/README.md
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 26000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Review the 6 runtime/ops Organization Data contracts against resolved permit, replication, recovery, and budget fixes, migrate them into .backlog/docs/organization-data/ via `backlog doc create`, and define/execute the safe handling of the 3 executable SQL/test assets (noting that crates/org-office/src/store/sqlite/schema.rs and spike.rs compile-time embed docs/design/organization-data/schema.sql via include_str!).

Files in scope (9):
- docs/design/organization-data/10-usage-and-cost-accounting.md
- docs/design/organization-data/11-model-profiles-and-agent-execution.md
- docs/design/organization-data/14-model-providers-and-connection-pools.md
- docs/design/organization-data/15-messaging-and-chat.md
- docs/design/organization-data/16-recovery-and-office-continuity.md
- docs/design/organization-data/17-budgets-and-controls.md
- docs/design/organization-data/schema.sql
- docs/design/organization-data/private-notes-schema.sql
- docs/design/organization-data/test_schema.py
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All 6 runtime/ops Markdown contracts (Docs 10, 11, 14, 15, 16, 17) are individually reviewed and migrated into .backlog/docs/organization-data/ via `backlog doc create`.
- [x] #2 Executable schema assets (schema.sql, private-notes-schema.sql, test_schema.py) are either moved with updated Rust include_str! / CI paths or explicitly retained at a documented code-owned path referenced from .backlog/docs/.
- [x] #3 cargo test -p org-office and test_schema.py pass after any path changes.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed and migrated all 6 runtime/ops organization-data contracts (Docs 10, 11, 14-17) into .backlog/docs/organization-data/ via backlog doc create, documented the 5 code-owned executable schema/test assets (schema.sql, private-notes-schema.sql, messaging-schema.sql, budgets-schema.sql, test_schema.py) in an index doc referencing crates/org-office include_str!, and verified 116/116 test_schema.py pass.
<!-- SECTION:FINAL_SUMMARY:END -->
