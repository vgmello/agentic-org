---
id: TASK-23
title: Review and migrate ADRs 0013-0022 (10 files) to .backlog/decisions/
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
- [ ] #1 Each of ADRs 0013-0022 is reviewed individually for accuracy, status, and alignment with current implementation.
- [ ] #2 All 10 decisions are created in .backlog/decisions/ via `backlog decision create` with accurate statuses and complete decision rationale.
- [ ] #3 A mapping table records ADR-0013..0022 -> Backlog decision IDs.
<!-- AC:END -->
