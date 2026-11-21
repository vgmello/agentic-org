---
id: TASK-67
title: >-
  UI Private Task Notes UX: Visualize per-agent private task notes in task
  detail view (ADR-0031)
status: In Progress
assignee:
  - '@ui-subagent'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T08:31:00Z'
labels:
  - ui
  - ux
  - work
  - agents
  - m1
milestone: m-0
dependencies:
  - TASK-66
documentation:
  - docs/adr/0031-any-employee-changes-tasks.md
  - .backlog/docs/doc-1 - Agentic-Org-—-Vision.md
  - docs/specs/work-items-ux.md
priority: high
type: feature
ordinal: 67000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Design and implement a calm, unmistakable per-agent private task notes pane on the /work/[ref] task detail view per ADR-0031 and Vision (doc-1): let humans inspect each agent's private working plan, reasoning, and test notes cleanly separated from public comments, grouped by author, read-only on agent notes, editable on the signed-in person's own private notes, with plain-language visibility explanation ('Visible to people and the author; hidden from other agents').
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Task detail view displays a dedicated Private notes section visually distinct from Public comments, grouped/attributed by author with clear visibility copy.
- [x] #2 Humans can read every agent's private notes strictly read-only (no edit/delete controls) and create/edit/delete only their own private notes.
- [x] #3 Screen works cleanly at desktop, narrow (320px), and 200% text scale with keyboard navigation and zero internal jargon.
- [ ] #4 Module API contract aligns with L8 (/api/v1 private-note endpoints), mock data lives under e2e/fakes/, and unit + Playwright specs pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Inspect ADR-0031, Vision doc-1, and current DiscussionSection.svelte/workitems module landed in TASK-66. 2. Implement dedicated per-agent Private notes pane on /work/[ref] visually distinct from Public comments, grouped/attributed by author with plain-language visibility explanation ('Visible to people and the author; hidden from other agents'), strictly read-only for agent notes and editable/deletable only for the signed-in person's own private notes. 3. Align WorkItemsApi contract with L8 (/api/v1 private-note endpoints), keep all mocks in e2e/fakes/, verify desktop + 320px + 200% zoom + keyboard nav + svelte-autofixer + vitest + Playwright, and pass fresh two-pass UI review.
<!-- SECTION:PLAN:END -->
