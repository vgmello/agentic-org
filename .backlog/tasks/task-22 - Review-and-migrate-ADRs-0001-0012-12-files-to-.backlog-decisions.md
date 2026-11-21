---
id: TASK-22
title: Review and migrate ADRs 0001-0012 (12 files) to .backlog/decisions/
status: Done
assignee: []
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:44:00Z'
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
ordinal: 22000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Review ADRs 0001 through 0012 individually against the current codebase and audit findings (especially ADR-0002/0006 vs ADR-0031, ADR-0007/0011 v2 deferrals, and ADR-0008/0009 implemented status) and migrate each into .backlog/decisions/ via `backlog decision create`.

Files in scope (12):
- docs/adr/0001-app-is-a-client-office-is-a-server.md
- docs/adr/0002-storage-sqlite-sqlcipher-one-writer.md
- docs/adr/0003-agent-messaging-office-first.md
- docs/adr/0004-headless-agent-sessions.md
- docs/adr/0005-standalone-server-v1.md
- docs/adr/0006-selected-task-acceptance.md
- docs/adr/0007-hq-creation-escrow.md
- docs/adr/0008-gateway-macos-sandbox.md
- docs/adr/0009-complete-release-verification.md
- docs/adr/0010-legacy-task-adoption.md
- docs/adr/0011-protected-recovery-delivery.md
- docs/adr/0012-office-channel-lifecycle.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Each of ADRs 0001-0012 is reviewed individually for accuracy, status (accepted, implemented, deferred, superseded), and contradictions with newer ADRs.
- [x] #2 All 12 decisions are created in .backlog/decisions/ via `backlog decision create` with accurate statuses and preserved decision context/consequences.
- [x] #3 A mapping table records ADR-0001..0012 -> Backlog decision IDs.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Read and review ADRs 0001-0012 individually against current codebase, TASK-19 updates, doc-1 (Vision), newer ADRs (including ADR-0016, ADR-0018, ADR-0021, ADR-0031), and v2 deferrals.
2. Create 12 Backlog decisions under .backlog/decisions/ via backlog decision create with accurate statuses, preserving full technical substance and adding concise Review note (2026-11-21) callouts for drift, pending implementation, or v2 deferrals.
3. Verify with backlog decision list and backlog doctor, record ADR-0001..0012 -> decision-XX mapping in TASK-22 notes, check ACs #1-#3, finalize Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed individual review and migration of ADRs 0001-0012 into .backlog/decisions/.

| Source Path | Decision ID | Status | Key Review Notes |
| --- | --- | --- | --- |
| docs/adr/0001-app-is-a-client-office-is-a-server.md | decision-1 | accepted | Active v1 architecture; bridge (TASK-5), org-spool removal (TASK-15), messaging (TASK-45), private notes (TASK-50), satellite relay (TASK-52) pending. |
| docs/adr/0002-storage-sqlite-sqlcipher-one-writer.md | decision-2 | accepted | SQLCipher superseded by ADR-0016; reconciled with ADR-0031 in TASK-19; open remediation in TASK-6..8, TASK-10, TASK-13, TASK-18, TASK-50. |
| docs/adr/0003-agent-messaging-office-first.md | decision-3 | accepted | org-mailbox implements directed rules; org-spool removal (TASK-15), org-store chat (TASK-45), DM isolation (TASK-10), HQ relay (TASK-52) pending. |
| docs/adr/0004-headless-agent-sessions.md | decision-4 | accepted | Aligned with doc-1; session runner/log viewer/asks pending (TASK-42, TASK-43, TASK-46); CLI takeover deferred to v2 (TASK-75); PTY exception in ADR-0032. |
| docs/adr/0005-standalone-server-v1.md | decision-5 | accepted | Flagged drift: CA escrow/recovery delivery bullet deferred to v2 (Owner D24, TASK-72); bridge/satellite/accounts/HTTPS tracked in TASK-5, 52, 58, 60, 62. |
| docs/adr/0006-selected-task-acceptance.md | decision-6 | accepted | Implementation pending (TASK-49, TASK-5, TASK-66); reconciled with ADR-0031 and crate names in TASK-19; TASK-3 moved comments off 64 KiB task.md cap. |
| docs/adr/0007-hq-creation-escrow.md | decision-7 | deferred | Deferred to v2 (Owner D24, TASK-72); org-pki foundation exists; TASK-10 removes wrapped Org CA key from journal Entry 1. |
| docs/adr/0008-gateway-macos-sandbox.md | decision-8 | implemented | Implemented in crates/org-platform-host/src/confinement/ + apps/agentic-org-web; flagged minor org-platform vs org-platform-host prose drift; gateway symbol cleanup in TASK-15. |
| docs/adr/0009-complete-release-verification.md | decision-9 | implemented | Implemented in crates/org-release + crates/org-host/src/release/; staging/updates/container release tracked in TASK-60, TASK-64. |
| docs/adr/0010-legacy-task-adoption.md | decision-10 | withdrawn | Withdrawn 2026-11-17 by owner; aligned with doc-1 no-legacy rule. |
| docs/adr/0011-protected-recovery-delivery.md | decision-11 | deferred | Deferred to v2 (Owner D24, TASK-72); preserved as v2 spec. |
| docs/adr/0012-office-channel-lifecycle.md | decision-12 | accepted | Pending/in-progress (TASK-5, TASK-16); RecoveryPublicRead use case deferred to v2 (TASK-72). |
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed ADRs 0001-0012 individually against current code, TASK-19 updates, newer ADRs, doc-1 (Vision), and v2 deferrals, and migrated all 12 into .backlog/decisions/ (decision-1 through decision-12) with accurate statuses, full technical substance preserved, and Review note (2026-11-21) callouts. Verified with backlog decision list and backlog doctor.
<!-- SECTION:FINAL_SUMMARY:END -->
