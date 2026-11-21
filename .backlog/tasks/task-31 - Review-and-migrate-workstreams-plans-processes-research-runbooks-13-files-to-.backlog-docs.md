---
id: TASK-31
title: >-
  Review and migrate workstreams, plans, processes, research & runbooks (13
  files) to .backlog/docs/
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - docs
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/workstreams/README.md
priority: medium
type: docs
ordinal: 31000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Individually review all operational, planning, process, research, and runbook documents under docs/{workstreams,plans,processes,research,runbooks}/ and migrate active ones into .backlog/docs/ via `backlog doc create` (archiving obsolete historical notes explicitly).

Files in scope (13):
- docs/workstreams/README.md
- docs/workstreams/backend.md
- docs/workstreams/linux.md
- docs/workstreams/ui.md
- docs/plans/v1-ui-builder-brief.md
- docs/plans/v1-ui-screens.md
- docs/processes/branch-and-merge.md
- docs/processes/decide-and-escalate.md
- docs/processes/v1-kickoff-prompt.md
- docs/research/agent-runtimes-control.md
- docs/research/assignment-branch-test-cases.md
- docs/research/c1-sign-in-spike.md
- docs/runbooks/local-hq.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 All 13 files across workstreams, plans, processes, research, and runbooks are individually reviewed for current relevance vs historical status.
- [ ] #2 Each active document is migrated into .backlog/docs/{workstreams,plans,processes,research,runbooks}/ via `backlog doc create`, and obsolete items are explicitly archived or noted.
- [ ] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->
