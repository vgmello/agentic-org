---
id: TASK-22
title: Review and migrate ADRs 0001-0012 (12 files) to .backlog/decisions/
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
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
- [ ] #1 Each of ADRs 0001-0012 is reviewed individually for accuracy, status (accepted, implemented, deferred, superseded), and contradictions with newer ADRs.
- [ ] #2 All 12 decisions are created in .backlog/decisions/ via `backlog decision create` with accurate statuses and preserved decision context/consequences.
- [ ] #3 A mapping table records ADR-0001..0012 -> Backlog decision IDs.
<!-- AC:END -->
