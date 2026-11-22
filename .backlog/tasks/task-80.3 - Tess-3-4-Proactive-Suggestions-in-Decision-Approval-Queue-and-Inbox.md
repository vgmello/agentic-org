---
id: TASK-80.3
title: 'Tess 3/4: Proactive Suggestions in Decision & Approval Queue and Inbox'
status: To Do
assignee: []
created_date: '2026-11-22T02:35:00Z'
labels:
  - backend
  - ui
  - inbox
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
ordinal: 83000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Enable Tess to proactively submit concrete, low-friction operational proposals into the owner Decision & Approval Queue (Home left column and Inbox) alongside human/agent approvals and decisions — for example, proposing to reroute queued tasks when an agent hits a budget cap. Tess never mutates assignments, budgets, or release gates silently; every proposal requires explicit owner confirmation and logs attributed audit history.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 New inbox/queue item kind "suggestion" is supported end-to-end (data model, office API, Home left-column Decision & Approval Queue, and Inbox) alongside human/agent approvals and decisions.
- [ ] #2 Each suggestion card includes Tess rationale, structured risk/impact/verification metadata (e.g., spend impact, recovered wait time, target capacity check), background context, and explicit owner actions (Apply suggestion / Dismiss), showing only "Tess" / "Chief of Staff" inline.
- [ ] #3 Tess never mutates assignments, budgets, or release gates silently; Apply suggestion executes the proposed action only after explicit owner confirmation and records full office journal audit attribution (both Tess proposal and owner approval/execution); Dismiss closes the suggestion without mutating target state.
- [ ] #4 Automated backend and UI tests cover suggestion creation, rendering in Home queue and Inbox, explicit Apply and Dismiss flows, authorization enforcement (owner-only where required), and failure/retry states.
<!-- AC:END -->
