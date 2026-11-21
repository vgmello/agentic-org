---
id: TASK-30
title: >-
  Review and migrate Agent, Governance & REQ specifications (docs/specs/, Batch
  B — 11 files) to .backlog/docs/
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - docs
  - ui
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/design/documentation-authority.md
priority: medium
type: docs
ordinal: 30000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Individually review each agent, governance, and REQ-* specification in docs/specs/ against current v1 scope and ADRs, then migrate them into .backlog/docs/specs/ via `backlog doc create`.

Files in scope (11):
- docs/specs/agent-run-modes-ux.md
- docs/specs/costs-and-insights-ux.md
- docs/specs/decisions-and-tech-debt-ux.md
- docs/specs/employee-supervision-ux.md
- docs/specs/processes-ux.md
- docs/specs/req-04-channel-messaging-and-question-components.md
- docs/specs/req-05-virtual-console-conversation-log-viewer.md
- docs/specs/req-06-desired-state-spec-authoring-and-reconciliation.md
- docs/specs/req-07-composable-process-packages-and-team-blueprints.md
- docs/specs/req-08-agent-os-cli-and-kernel-primitives.md
- docs/specs/retrospectives-ux.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 All 11 agent, governance, and REQ-* specification files are individually reviewed against current ADRs and v1 vs v2 scope.
- [ ] #2 All 11 specifications are migrated into .backlog/docs/specs/ via `backlog doc create` with type specification.
- [ ] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->
