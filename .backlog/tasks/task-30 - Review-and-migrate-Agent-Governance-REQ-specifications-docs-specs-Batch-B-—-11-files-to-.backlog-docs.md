---
id: TASK-30
title: >-
  Review and migrate Agent, Governance & REQ specifications (docs/specs/, Batch
  B — 11 files) to .backlog/docs/
status: Done
assignee:
  - '@backlog-worker'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:21:00Z'
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
- [x] #1 All 11 agent, governance, and REQ-* specification files are individually reviewed against current ADRs and v1 vs v2 scope.
- [x] #2 All 11 specifications are migrated into .backlog/docs/specs/ via `backlog doc create` with type specification.
- [x] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Review all 11 agent, governance, and REQ-* specifications in docs/specs/ (Batch B) against current ADRs, apps/agentic-org-ui, doc-1 (Vision), and v1 vs v2 scope.
2. Add concise Review note (2026-11-21) callouts preserving owner requirements verbatim while noting ADR-0028/D3/REQ-08 deltas, nav-path drift, and not-in-v1 scope.
3. Migrate all 11 specs into .backlog/docs/specs/ with type specification via backlog doc create/update.
4. Record mapping and findings in TASK-30 notes, check ACs #1-#3, set final summary, mark Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed review and migration of all 11 docs/specs/ (Batch B) agent, governance, and REQ-* specifications into .backlog/docs/specs/.

Mapping (old path -> Backlog doc ID):
- docs/specs/agent-run-modes-ux.md -> doc-50
- docs/specs/costs-and-insights-ux.md -> doc-51
- docs/specs/decisions-and-tech-debt-ux.md -> doc-52
- docs/specs/employee-supervision-ux.md -> doc-53
- docs/specs/processes-ux.md -> doc-54
- docs/specs/req-04-channel-messaging-and-question-components.md -> doc-55
- docs/specs/req-05-virtual-console-conversation-log-viewer.md -> doc-56
- docs/specs/req-06-desired-state-spec-authoring-and-reconciliation.md -> doc-57
- docs/specs/req-07-composable-process-packages-and-team-blueprints.md -> doc-58
- docs/specs/req-08-agent-os-cli-and-kernel-primitives.md -> doc-59
- docs/specs/retrospectives-ux.md -> doc-60

Key review findings:
- agent-run-modes-ux.md intro and Decision 2 omit ADR-0028 person DM/mention/task-assignment triggers despite RUN-3 including ADR-0028.
- employee-supervision-ux.md (§0, §2) references local CLI takeover paused by Owner D3 (FC-151).
- processes-ux.md §2 cites top-level main nav vs implemented Organization settings > Processes (/settings/processes) in app-shell-ux.md §0.1.
- req-04 alignment's ask_user MCP tool route is superseded by REQ-08 decision 8 (agent-os msg ask only).
- req-05 retains open owner call #4 (live transcript tailing vs finished/paused sessions in v1).
- req-06, req-07, and req-08 align with ADR-0026/0028/0029 and doc-1; K1 (crates/org-command) is already merged.
- retrospectives-ux.md preserved explicitly as not-in-v1 (v2 reference).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed all 11 agent, governance, and REQ-* specifications (docs/specs/, Batch B) against current ADRs, apps/agentic-org-ui, doc-1 (Vision), and v1 vs v2 scope, and migrated them into .backlog/docs/specs/ as doc-50 through doc-60 (type specification) with Review note (2026-11-21) callouts preserving owner requirements verbatim.
<!-- SECTION:FINAL_SUMMARY:END -->
