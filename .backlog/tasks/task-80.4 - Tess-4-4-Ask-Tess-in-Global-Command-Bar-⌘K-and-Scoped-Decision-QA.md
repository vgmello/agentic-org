---
id: TASK-80.4
title: 'Tess 4/4: Ask Tess in Global Command Bar (⌘K) and Scoped Decision Q&A'
status: To Do
assignee: []
created_date: '2026-11-22T02:35:00Z'
updated_date: '2026-11-23T04:07:00Z'
labels:
  - backend
  - ui
  - shell
  - chat
  - tess
milestone: m-1
dependencies:
  - TASK-80.1
  - TASK-80.3
documentation:
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/design/doc-36 - Design-Smart-Features-Not-in-v1.md
parent_task_id: TASK-80
priority: medium
type: feature
ordinal: 84000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Wire every global and card-level Ask AI interaction in the UI to Tess so users can ask org-wide questions from the global command bar (⌘K) or ask item-scoped questions directly on any active Decision & Approval Queue card (risk, blast radius, checks, rollback path), with full authorization enforcement so Tess only reads data the signed-in caller is permitted to view.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Global command bar (⌘K) displays placeholder "Search, run a command, or ask Tess…" with a compact "✨ Ask" button (sparkle icon + Ask) that opens the command palette directly in the Ask Tess tab.
- [ ] #2 Every active card in the Decision & Approval Queue renders a quiet "✨ Ask about this" trigger that opens Ask Tess pre-scoped to that item context (risk, blast radius, checks, and rollback path).
- [ ] #3 Ask Tess conversations route to Tess (builtin_key = "chief_of_staff"), record turns and usage under Tess employee ID in normal chat/accounting streams, and show an explicit unavailable state when no runtime/provider is configured.
- [ ] #4 Access boundaries are enforced server-side on every Ask Tess query so Tess only reads data the signed-in caller is authorized to view (never exposing other agents private notes or host-only secrets), verified by automated backend and UI tests.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (Tess 4/4 — Ask Tess API):
1. Wire & RPC (org-proto, org-office-sessions, org-office-server, operations.json, agentic-org-web):
   - Add chief_of_staff.ask (global or item-scoped context: task/decision/inbox item id) routing to Tess (builtin_key='chief_of_staff') through normal chat/session/accounting streams so turns & usage record under Tess's employee ID.
   - Return explicit unavailable state when Tess has no configured/ready runtime or provider binding.
2. Server-side access enforcement: assemble item/org context strictly through caller-authorized read paths (never leaking other agents' private task notes from private/task-notes.db, private DMs, or host/office secrets).
3. Verification: backend authorization boundary tests (owner, PM, regular member), unavailable-runtime test, and accounting attribution test.
<!-- SECTION:PLAN:END -->
