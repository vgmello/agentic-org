---
id: TASK-80.2
title: 'Tess 2/4: Executive Brief on Home (Today, 7-Day, and Board lenses)'
status: To Do
assignee: []
created_date: '2026-11-22T02:35:00Z'
labels:
  - backend
  - ui
  - home
  - tess
milestone: m-0
dependencies:
  - TASK-80.1
documentation:
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - >-
    .backlog/docs/organization-data/doc-78 -
    Organization-Data-04-—-Agent-Structure.md
parent_task_id: TASK-80
priority: medium
type: feature
ordinal: 82000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Replace the raw Catch-up / Live & Highlights activity feed on the Home right column with a cohesive, read-only Executive Brief authored and signed by Tess. The brief synthesizes live agent activity, recent delivery highlights, active operational problems/risks (offline offices, budget-capped agents, slipping milestones), and points to the highest-leverage items queued on the left — without rendering any execution buttons or action chips so the owner has one unified action surface in the Decision & Approval Queue.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Home right column replaces raw Catch-up / Live & Highlights with Tess Executive Brief authored and signed "Tess · Chief of Staff" (signature links to Tess profile; full bio expansion "Tactical Execution & Squad Supervisor" never appears inline).
- [ ] #2 Brief synthesizes live agent activity, recent delivery highlights, active operational risks/problems, and pointers to top-priority items in the left-hand queue, generated on demand / on brief-refresh triggers with explicit loading, empty, stale, and unavailable-runtime states.
- [ ] #3 Audience/timeframe lens switcher supports Today (tactical morning brief), 7-Day (velocity, autonomy, spend & quality trends), and Board (concise leadership summary with 1-click Copy).
- [ ] #4 Executive Brief is strictly read-only with zero inline execution buttons or action chips; automated backend and UI tests verify lens switching, Board copy, profile link, read-only separation, and caller authorization scoping.
<!-- AC:END -->
