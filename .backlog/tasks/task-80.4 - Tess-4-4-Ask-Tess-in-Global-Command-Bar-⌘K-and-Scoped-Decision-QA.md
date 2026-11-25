---
id: TASK-80.4
title: >-
  Tess 2/5 (v1): Operator CLI delta, MutationPolicy + server dry-run, suggestion
  create & ui present tools, 7 product-guide skills & SSE streaming backend
status: To Do
assignee: []
created_date: '2026-11-22T02:35:00Z'
updated_date: '2026-11-25T00:15:00Z'
labels:
  - backend
  - ui
  - shell
  - chat
  - tess
milestone: m-0
dependencies:
  - TASK-80.1
  - TASK-103
documentation:
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/design/doc-36 - Design-Smart-Features-Not-in-v1.md
  - >-
    .backlog/docs/design/doc-97 -
    Design-Embedded-Tess-—-Chief-of-Staff-and-Product-Assistant.md
parent_task_id: TASK-80
priority: high
type: feature
ordinal: 84000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers the full token-efficient backend and AX engine for Embedded Tess exclusively over agent-os CLI + session attach (zero MCP per CLI-12): adds missing Chief-of-Staff operator read/config commands to agent-os, introduces declarative MutationPolicy (AutoSafe / ConfirmRequired / ForbiddenForAgent) in CommandTable with fail-closed policy_violation steering, wires server-validated tool-backed UI artifacts (agent-os suggestion create with real server --dry-run + pinned --if-match AND agent-os ui present for verified entity links/metrics/navigation/follow-ups), ships 7 version-locked product-guide/Tess skills via agent-os skill list --category | read, and streams live redacted tool.progress, text.delta, and ui.block frames over HTTP/1.1 SSE (/api/v1/organizations/{org}/tess/*) with Stop/Cancel (Control::Interrupt shared with TASK-103) and two-phase human-authenticated HostChannel Apply.
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

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
2026-11-24 4-Way Council Alignment (PM + Systems Architect + Principal UX + Principal AX):
- Operator CLI Delta (server-gated to chief_of_staff + live acting_for_caller_id): project add-team/remove-team, cost query/prices/subscription-usage, budget list/inspect, session list, agent execution-get/readiness, provider list/quota, model-profile list, office sync-status/list, inbox list (owner-only), channel create, suggestion create|list|inspect|withdraw, ui present. Host admin/TLS/PKI/OAuth/owner handover stay strictly person/host-only (never on agent-os).
- Zero-Memorization Safety (MutationPolicy): ALL durable org/work/config mutations default to ConfirmRequired in Chief-of-Staff / Ask Tess mode (AutoSafe = suggestion create|withdraw, ui present, status set|clear, Tess's own private note, caller reply). Attempting a direct ConfirmRequired write returns ErrorCode::PolicyViolation + exact error.hint steering to agent-os suggestion create. Server runs real rollback --dry-run on v1 proposal target commands (task assign|transition, agent pause|resume|move, hr hire, team/project/role/process/budget mutations) and pins expected_revision; clicking Apply executes under the human caller's HostChannel grant (via_grant: true) with dual attribution (actor=caller, proposed_by=tess).
- Product Knowledge (Tier 3): extend BuiltinSkill in crates/org-process/src/skills.rs with category (workflow|product_guide|chief_of_staff) and ship 7 concise version-locked guides checked by verify_command_references (AX-14): product-org-and-teams, product-projects-and-work (DSS vs Kanban), product-processes-and-standards, product-offices-and-runtimes, product-budgets-and-permissions, product-troubleshooting, tess-workflows.
- Streaming Bridge & Turn Envelope: add SessionStreamHub fed post-redaction from pump.rs (shared with TASK-103); endpoints POST /tess/ask, GET /tess/turns/{id}/stream (SSE with Last-Event-ID + 15s heartbeat), POST /tess/turns/{id}/cancel, POST /tess/proposals/{id}/{apply,dismiss}, POST /tess/session/reset; append <=500-800 B volatile UI viewport context envelope strictly last in turn input (never full org dumps); attribute usage under tess_employee_id with delegated_by_person_id; return honest status='fallback' + unavailable_reason when no runtime/provider is configured.
<!-- SECTION:NOTES:END -->
