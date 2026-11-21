---
id: TASK-23
title: Review and migrate ADRs 0013-0022 (10 files) to .backlog/decisions/
status: Done
assignee: []
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:46:00Z'
labels:
  - docs
  - adr
  - backlog-migration
milestone: m-5
dependencies:
  - TASK-19
documentation:
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 23000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Review ADRs 0013 through 0022 individually against the current codebase and audit findings (including ADR-0013 and ADR-0017 implemented status, ADR-0016 boundary risks, and ADR-0018 compliance gaps) and migrate each into .backlog/decisions/ via `backlog decision create`.

Files in scope (10):
- docs/adr/0013-stored-browser-sessions.md
- docs/adr/0014-acknowledgement-classes-and-agent-os-latency.md
- docs/adr/0015-agent-isolation-modes.md
- docs/adr/0016-no-application-level-data-encryption.md
- docs/adr/0017-linux-and-container-hosting.md
- docs/adr/0018-storage-engine-independence.md
- docs/adr/0019-office-coded-task-numbers.md
- docs/adr/0020-required-asks-suspend-and-resume.md
- docs/adr/0021-office-store-architecture.md
- docs/adr/0022-host-creates-its-registry-no-reverse-bootstrap.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Each of ADRs 0013-0022 is reviewed individually for accuracy, status, and alignment with current implementation.
- [x] #2 All 10 decisions are created in .backlog/decisions/ via `backlog decision create` with accurate statuses and complete decision rationale.
- [x] #3 A mapping table records ADR-0013..0022 -> Backlog decision IDs.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Read and review ADRs 0013-0022 individually against current codebase, TASK-19 updates, doc-1 (Vision), audit tasks (TASK-5..TASK-18), and implementation status.
2. Create 10 Backlog decisions (decision-13..decision-22) under .backlog/decisions/ via backlog decision create with accurate statuses, full technical substance, and Review note (2026-11-21) callouts.
3. Verify with backlog decision list and backlog doctor, record ADR-0013..0022 -> decision-13..22 mapping in TASK-23 notes, check ACs #1-#3, finalize Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed individual review and migration of ADRs 0013-0022 into .backlog/decisions/.

| Source Path | Decision ID | Status | Key Review Notes |
| --- | --- | --- | --- |
| docs/adr/0013-stored-browser-sessions.md | decision-13 | implemented | Implemented in org-host/hostdb + agentic-org-web; gateway rename in TASK-15, A6 account flows in TASK-38, signed tokens v2. |
| docs/adr/0014-acknowledgement-classes-and-agent-os-latency.md | decision-14 | accepted | Core batching in org-store; latency guards (TASK-33), session attach (TASK-15/42), domain adoption (TASK-45/49) pending. |
| docs/adr/0015-agent-isolation-modes.md | decision-15 | accepted | Pending across TASK-42 (private home), TASK-57 (L15 os-sandbox moved to v1), TASK-58 (L16 dedicated-user); OAuth scrubbing in TASK-11. |
| docs/adr/0016-no-application-level-data-encryption.md | decision-16 | implemented | Implemented (A1.1-A1.7); open boundary/security audit items in TASK-10, TASK-11, TASK-13; recovery flows v2 (TASK-72). |
| docs/adr/0017-linux-and-container-hosting.md | decision-17 | implemented | Implemented (X1a-X5); allowlist cleanup (TASK-63), container HTTPS (TASK-62), release image (TASK-64) pending. |
| docs/adr/0018-storage-engine-independence.md | decision-18 | accepted | org-office on org-store; open compliance gaps in org-host/re-exports (TASK-13), trigger overhead (TASK-18), B1c guards (TASK-33). |
| docs/adr/0019-office-coded-task-numbers.md | decision-19 | accepted | Applied to Doc 03/06 and schema.sql; code pending in TASK-49 (B4/B5). |
| docs/adr/0020-required-asks-suspend-and-resume.md | decision-20 | accepted | Aligned with doc-1; pending in TASK-46 (B7) and TASK-47 (C6). |
| docs/adr/0021-office-store-architecture.md | decision-21 | implemented | Implemented (B1a/B1b); flagged Decision 9 two-tables vs 4 store_* tables drift (TASK-19); follow-ups in TASK-10, 13, 14, 33, 36, 45, 49. |
| docs/adr/0022-host-creates-its-registry-no-reverse-bootstrap.md | decision-22 | implemented | Implemented; uninstall marker cleanup (FC-022) tracked in TASK-60. |
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed ADRs 0013-0022 individually against current implementation, TASK-19 updates, audit tasks (TASK-5..TASK-18), and doc-1 (Vision), and migrated all 10 into .backlog/decisions/ (decision-13 through decision-22) with accurate statuses, full technical rationale, and Review note (2026-11-21) callouts. Verified with backlog doctor.
<!-- SECTION:FINAL_SUMMARY:END -->
