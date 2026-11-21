---
id: TASK-24
title: Review and migrate ADRs 0023-0032 (10 files) to .backlog/decisions/
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
ordinal: 24000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Review ADRs 0023 through 0032 individually against the current codebase and audit findings (including removing superseded approval prose from ADR-0027 vs ADR-0029 and reconciling ADR-0032 offline/token notes) and migrate each into .backlog/decisions/ via `backlog decision create`.

Files in scope (10):
- docs/adr/0023-an-agents-ask-starts-a-turn.md
- docs/adr/0024-a-channel-question-is-watched-for-its-answer.md
- docs/adr/0025-agents-start-sub-agents.md
- docs/adr/0026-development-processes-are-content.md
- docs/adr/0027-a-lead-approves-a-hire.md
- docs/adr/0028-an-agent-is-always-available.md
- docs/adr/0029-only-who-may-hire-hires.md
- docs/adr/0030-an-agent-works-while-it-has-work.md
- docs/adr/0031-any-employee-changes-tasks.md
- docs/adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Each of ADRs 0023-0032 is reviewed individually for accuracy, supersession status, and open contradictions.
- [ ] #2 All 10 decisions are created in .backlog/decisions/ via `backlog decision create` with clean non-contradictory text and accurate statuses.
- [ ] #3 A mapping table records ADR-0023..0032 -> Backlog decision IDs.
<!-- AC:END -->
