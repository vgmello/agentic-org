---
id: TASK-103
title: >-
  Live Chat Turn Controls: Stop/Interrupt Runaway Turn, Mid-Turn Steer &
  Ephemeral Activity/Tool Status Indicator
status: To Do
assignee: []
created_date: '2026-11-24T04:23:00Z'
updated_date: '2026-11-25T00:15:00Z'
labels:
  - feature
  - backend
  - rust
  - frontend
  - ui
  - agents
  - runtime
  - chat
  - ux
  - docs
milestone: m-0
dependencies:
  - TASK-42
  - TASK-45
  - TASK-83
documentation:
  - .backlog/docs/doc-7 - Architecture-Non-Negotiables-and-Who-Decides-What.md
  - .backlog/docs/design/doc-29 - Design-Agent-Sessions-and-Runtime-Isolation.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-53 - Spec-Employee-Supervision-and-Approvals-UX.md
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - >-
    .backlog/docs/design/doc-97 -
    Design-Embedded-Tess-—-Chief-of-Staff-and-Product-Assistant.md
priority: high
type: feature
ordinal: 118000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Gives people real-time visibility and control over running agent turns inside Chat without duplicating TASK-43's read-only session log viewer: leverages the already-implemented Control::Interrupt and Control::Send { urgent: true } (native turn/steer on Codex, NeedsInterrupt fallback on Claude/Agy) in org-agent-runtime, streams coarse ephemeral live activity ('Thinking…', 'Running Bash…') over coalesced Feed::Presence SSE, and exposes explicit authorized person 'Stop turn' and 'Steer' controls in Chat while keeping ordinary chat messages queued.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Records the owner-approved clarification in doc-7 (#11), doc-29 (§Control/Interrupt), doc-40 (Decision 8 / §4.2 / CHAT-7), and doc-53 (Decision 6): ordinary chat messages (chat.send/msg.send) ALWAYS queue behind a busy turn, while explicit authorized person Turn Controls (member.session_interrupt / member.session_steer) may interrupt or steer a running turn.
- [ ] #2 Extends PumpCommand with InterruptTurn and SteerTurn and tracks coarse ephemeral activity (activity_phase: Thinking | RunningTool | Responding | Interrupting, active_tool_name, active_tool_started_at, supports_steer) on SessionRuntimeState/SessionView (org-proto/src/sessions.rs); handles ControlError::NeedsInterrupt via interrupt-then-send on Claude/Agy and native turn/steer on Codex; ensures explicit human TurnOutcome::Interrupted keeps the warm process alive (distinct from member.session_stop / Control::Close), properly releases/re-acquires turn permits, and NEVER triggers provider-pool failover (pump.rs:869).
- [ ] #3 Adds member.session_interrupt and member.session_steer to org-proto::sessions, SessionManager (with authorize_owner_or_person / owner-or-ancestor-lead enforcement), crates/org-office-server/src/serve/host/operations.json (X-02 allowlist), and POST /api/v1/organizations/{org}/agents/{id}/{interrupt,steer} in apps/agentic-org-web/src/api/session_routes.rs.
- [ ] #4 Streams coalesced per-member live activity over LiveEvents (Feed::Presence) -> chat.events_poll -> /api/v1/events/stream without evicting message events from the 1024-entry ring and strictly emitting coarse labels only (zero raw tool inputs/outputs/diffs/thinking text, preserving doc-40 CHAT-4 and zero overlap with TASK-43).
- [ ] #5 Wires live Feed::Presence frames into apps/agentic-org-ui (messaging/http-api.ts, ChatWorkspace.svelte, Composer.svelte with aria-live='polite') to show ephemeral 'Thinking…' / 'Running <tool>…' status, an explicit 'Stop turn' button, an explicit 'Steer' submit action (distinct from queued Send), and a link to TASK-43's session log viewer; verified by fake-CLI adapter tests (Codex native steer + Claude/Agy NeedsInterrupt + Interrupt) and Playwright E2E; updates doc-71 and doc-3 (FC-081/FC-102).
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
2026-11-24 Shared Streaming Infrastructure with Embedded Tess (TASK-80.4): Design PumpCommand::{InterruptTurn,SteerTurn} and a shared post-redaction SessionStreamHub cleanly inside crates/org-office-sessions so both TASK-103 (Chat Turn Controls) and TASK-80.4 (Ask Tess SSE stream + Stop button) reuse the exact same interrupt/steer/stream primitives.
<!-- SECTION:NOTES:END -->
