---
id: TASK-80
title: >-
  Built-in Chief of Staff agent (Tess): Embedded Product Assistant, Safe System
  Operator, Ask Tess Side Panel (v1) & Proactive Brief/Queue (v2)
status: To Do
assignee: []
created_date: '2026-11-22T02:08:00Z'
updated_date: '2026-11-25T00:01:00Z'
labels:
  - backend
  - ui
  - agents
  - built-in
  - chief-of-staff
  - tess
  - home
  - inbox
  - blog
milestone: m-0
dependencies:
  - TASK-42
  - TASK-45
  - TASK-49
documentation:
  - >-
    .backlog/docs/organization-data/doc-78 -
    Organization-Data-04-—-Agent-Structure.md
  - >-
    .backlog/docs/organization-data/doc-75 -
    Organization-Data-01-—-Organization-Structure.md
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/design/doc-36 - Design-Smart-Features-Not-in-v1.md
priority: high
type: feature
ordinal: 80000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers Tess (employees.builtin_key='chief_of_staff'), Agentic Org's embedded Chief of Staff and product-aware assistant running on whichever local coding agent runtime the user has installed (Claude, Codex, or Agy) over agent-os CLI + session attach (zero MCP wrappers per CLI-12, 30x-50x lower token tax). Splits cleanly into v1 (m-0) Interactive Embedded Tess (80.1 Identity/Sandbox/Dual-Auth, 80.4 Operator CLI/Product-Guide Skills/MutationPolicy/SSE Streaming/Two-Phase Action Proposals, 80.5 Persistent Right-Hand Tess Side Panel UI replacing the cramped ⌘K AI tab) and v2 (m-1) Autonomous Proactive Tess (80.2 scheduled LLM Executive Brief, 80.3 proactive background Suggestions Queue reusing 80.4's exact proposal pipeline).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Creating a new organization provisions employees.builtin_key = "chief_of_staff" (Tess) in the top-level team reporting to the owner alongside the built-in HR lead, enforced unique per organization, with may_hire = true (authorized to hire into any team via the HR hire flow), no hire request, non-offboardable while org exists, and sole org-wide orchestrator enforcement.
- [ ] #2 Tess profile displays "Tactical Execution & Squad Supervisor"; inline banners, cards, and the Home Executive Brief signature show only "Tess" / "Chief of Staff" and link to her profile.
- [ ] #3 Home displays Tess Executive Brief (Today / 7-Day / Board lenses, with 1-click Copy on Board) incorporating live highlights and operational risks with zero action chips/buttons on the right.
- [ ] #4 Tess proposed solutions appear as actionable suggestion cards in the left-hand Decision & Approval Queue and Inbox with rationale and structured risk/impact/verification metadata; applying executes only after explicit owner confirmation and logs attributed audit history.
- [ ] #5 Global command bar (⌘K) shows placeholder "Search, run a command, or ask Tess…" with ✨ Ask opening Ask Tess, and clicking ✨ Ask about this on any active decision card opens Ask Tess pre-scoped to that item while enforcing caller read/private-note/secret boundaries.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (Tess Chief of Staff — parent of 80.1..80.4):
Execute subtasks in strict order: TASK-80.1 (identity/provisioning/sole orchestrator & org-wide may_hire) -> TASK-80.2 (Executive Brief API: Today/7-Day/Board read-only synthesis) -> TASK-80.3 (Suggestions Queue: new inbox kind 'suggestion' with explicit owner Apply/Dismiss & journal audit) -> TASK-80.4 (Ask Tess global & item-scoped Q&A enforcing caller read/private-note/secret boundaries). Coordinate UI surfaces with ui-dev once backend /api/v1 routes land.
<!-- SECTION:PLAN:END -->
