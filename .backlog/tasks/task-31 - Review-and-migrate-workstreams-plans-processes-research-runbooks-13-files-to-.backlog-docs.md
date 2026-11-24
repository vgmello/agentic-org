---
id: TASK-31
title: >-
  Review and migrate workstreams, plans, processes, research & runbooks (13
  files) to .backlog/docs/
status: Done
assignee:
  - '@backlog-worker'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:17:00Z'
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
- [x] #1 All 13 files across workstreams, plans, processes, research, and runbooks are individually reviewed for current relevance vs historical status.
- [x] #2 Each active document is migrated into .backlog/docs/{workstreams,plans,processes,research,runbooks}/ via `backlog doc create`, and obsolete items are explicitly archived or noted.
- [x] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Review all 13 files across docs/{workstreams,plans,processes,research,runbooks}/ for current accuracy vs historical/obsolete status. 2. Migrate each file into .backlog/docs/{workstreams,plans,processes,research,runbooks}/ via backlog doc create/update with explicit review/historical notes. 3. Record old-path -> doc-ID mapping and findings in notes, verify ACs, mark Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Migration mapping (old path -> Backlog doc ID):
- `docs/workstreams/README.md` -> `doc-14` (Workstreams Overview)
- `docs/workstreams/backend.md` -> `doc-15` (Backend Stream Handover (Tracks A and B))
- `docs/workstreams/linux.md` -> `doc-16` (Linux Stream Handover (Track X))
- `docs/workstreams/ui.md` -> `doc-17` (Product UI Stream Handover (Track U))
- `docs/plans/v1-ui-builder-brief.md` -> `doc-18` (v1 UI Builder Brief (Historical))
- `docs/plans/v1-ui-screens.md` -> `doc-19` (v1 UI Screens Build Plan)
- `docs/processes/branch-and-merge.md` -> `doc-20` (Process: Branch, Review and Integration)
- `docs/processes/decide-and-escalate.md` -> `doc-21` (Process: Decide, Record, Escalate)
- `docs/processes/v1-kickoff-prompt.md` -> `doc-22` (v1 Implementation Kickoff Prompt (Historical))
- `docs/research/agent-runtimes-control.md` -> `doc-23` (Research: Controlling Agent Runtimes (Claude Code, Codex, Antigravity))
- `docs/research/assignment-branch-test-cases.md` -> `doc-24` (Research: Assignment Branch Test Cases)
- `docs/research/c1-sign-in-spike.md` -> `doc-25` (Research: C1 Sign-In, Private Home and Sub-Agent Isolation Spike)
- `docs/runbooks/local-hq.md` -> `doc-26` (Runbook: Development Host on Local Machine)

Review findings & historical flags:
- `docs/plans/v1-ui-builder-brief.md` and `docs/processes/v1-kickoff-prompt.md`: marked Historical / Superseded (hardcoded session trailers/paths and pre-Backlog kickoff state).
- `docs/plans/v1-ui-screens.md`: Step 3 Offices and Step 4 Projects table rows lag behind merged `v1` state; mentions superseded Codex review cycle.
- `docs/runbooks/local-hq.md`: 'What works' table predates A3 B4–B6 and A4 browser registration/sign-in/create-organization merges.
- `docs/workstreams/*`: active remaining work migrated to `TASK-33`–`TASK-75`; open owner decisions preserved explicitly.
- `docs/research/*`: dated vendor/test/spike references preserved with clear currency notes (C1 Linux + Codex live-turn gaps noted).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed all 13 operational, planning, process, research, and runbook docs and migrated them into .backlog/docs/{workstreams,plans,processes,research,runbooks}/ (doc-14..doc-26), explicitly marking superseded historical prompts/briefs and stale runbook/UI-plan status claims.
<!-- SECTION:FINAL_SUMMARY:END -->
