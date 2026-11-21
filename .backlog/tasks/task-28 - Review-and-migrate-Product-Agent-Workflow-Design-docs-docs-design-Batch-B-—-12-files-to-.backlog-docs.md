---
id: TASK-28
title: >-
  Review and migrate Product, Agent & Workflow Design docs (docs/design/, Batch
  B — 12 files) to .backlog/docs/
status: Done
assignee:
  - '@backlog-worker'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:19:00Z'
labels:
  - docs
  - architecture
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/design/documentation-authority.md
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 28000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Individually review each product, agent, and workflow design document in docs/design/ against the authoritative Organization Data contracts and current v1 scope, then migrate them into .backlog/docs/design/ via `backlog doc create`.

Files in scope (12):
- docs/design/agent-instructions.md
- docs/design/agent-os-command-line.md
- docs/design/agent-sessions.md
- docs/design/create-organization.md
- docs/design/desired-state-specifications.md
- docs/design/development-processes.md
- docs/design/employee-supervision.md
- docs/design/processes.md
- docs/design/projects-git-github.md
- docs/design/smart-features.md
- docs/design/tasks-projects-docs.md
- docs/design/teams-and-org-chart.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All 12 product/agent/workflow design documents are individually reviewed for consistency with Organization Data contracts and current v1 scope (clearly marking deferred/v2 sections).
- [x] #2 All 12 documents are migrated into .backlog/docs/design/ via `backlog doc create`.
- [x] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Review all 12 product/agent/workflow design docs in docs/design/ (Batch B) against Organization Data contracts, doc-1 (Vision), ADRs, and current v1/v2 scope.
2. Add concise Review note (2026-11-21) banners flagging contradictions, stale code status, open questions, and deferred v2 sections.
3. Migrate all 12 docs into .backlog/docs/design/ via backlog doc create/update.
4. Record old path -> doc-XX mapping and findings in TASK-28 notes, check ACs #1-#3, set final summary, mark Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed review and migration of all 12 docs/design/ (Batch B) files into .backlog/docs/design/.

Mapping (old path -> Backlog doc ID):
- docs/design/agent-instructions.md -> doc-27
- docs/design/agent-os-command-line.md -> doc-28
- docs/design/agent-sessions.md -> doc-29
- docs/design/create-organization.md -> doc-30
- docs/design/desired-state-specifications.md -> doc-31
- docs/design/development-processes.md -> doc-32
- docs/design/employee-supervision.md -> doc-33
- docs/design/processes.md -> doc-34
- docs/design/projects-git-github.md -> doc-35
- docs/design/smart-features.md -> doc-36
- docs/design/tasks-projects-docs.md -> doc-37
- docs/design/teams-and-org-chart.md -> doc-38

Key review findings:
- agent-instructions.md §4 contradicts amended ADR-0025 decision 6 (background sub-agents persist across turns while session active).
- create-organization.md recovery code / CA escrow is deferred to v2 (Owner D24, 2026-11-20; TASK-72).
- agent-sessions.md & employee-supervision.md predate TASK-2 offline permit remediation, Owner D3 takeover pause, and L15/L16 isolation modes.
- tasks-projects-docs.md §3.4 cites deleted app_token; private notes covered by ADR-0031, TASK-8, TASK-10, TASK-67.
- teams-and-org-chart.md and agent-os-command-line.md 'What the code does today' sections predate K1, B1b, and B2.1.
- projects-git-github.md retains 9 open design questions (#1-#3, #5-#10) needed before C7 (TASK-50).
- smart-features.md explicitly preserved as not-in-v1 (v2 reference).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed all 12 product, agent, and workflow design documents (docs/design/, Batch B) against Organization Data contracts, ADRs, doc-1 (Vision), and current v1/v2 scope, and migrated them into .backlog/docs/design/ as doc-27 through doc-38 with Review note (2026-11-21) callouts preserving requirements while flagging stale status, ADR-0025/D3/D24 deltas, and v2 scope.
<!-- SECTION:FINAL_SUMMARY:END -->
