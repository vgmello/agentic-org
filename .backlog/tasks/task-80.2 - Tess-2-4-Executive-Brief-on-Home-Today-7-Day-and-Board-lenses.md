---
id: TASK-80.2
title: 'Tess 2/4: Executive Brief on Home (Today, 7-Day, and Board lenses)'
status: To Do
assignee: []
created_date: '2026-11-22T02:35:00Z'
updated_date: '2026-11-23T04:07:00Z'
labels:
  - backend
  - ui
  - home
  - tess
milestone: m-1
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

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (Tess 2/4 — Executive Brief API):
1. Wire & RPC (org-proto, org-office, org-office-server, operations.json, apps/agentic-org-web):
   - Add chief_of_staff.brief.get / chief_of_staff.brief.refresh RPCs and GET/POST /api/v1/organizations/{org}/chief-of-staff/brief routes supporting lens=today|seven_day|board.
2. Synthesis & caching: aggregate live agent session state, recent completed tasks/highlights, active risks (offline offices, budget-capped agents, stalled sessions, blocked tasks), and pointers to top decision queue items under caller's visibility scope; support on-demand generation via Tess runtime (failing cleanly to explicit unavailable-runtime state when unconfigured) and persisted brief freshness metadata.
3. Strictly read-only payload (no inline mutation actions); verify caller authorization scoping (owner vs PM vs member) and zero leakage of private notes/secrets.
4. Verification: cargo test -p org-office -p org-office-sessions -p org-office-server -p agentic-org-web.
<!-- SECTION:PLAN:END -->
