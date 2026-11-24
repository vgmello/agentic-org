---
id: TASK-102
title: >-
  Desktop/Browser Notifications & Unread Tab Badge for Blocking Asks, Approvals,
  Budget Holds & Reviews
status: To Do
assignee: []
created_date: '2026-11-24T04:23:00Z'
labels:
  - feature
  - frontend
  - ui
  - backend
  - rust
  - inbox
  - notifications
  - ux
  - docs
milestone: m-0
dependencies:
  - TASK-42
  - TASK-45
  - TASK-53
documentation:
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-45 - Spec-Hiring-Profiles-and-Team-Moves-UX.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
priority: high
type: feature
ordinal: 117000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Ensures owners never miss blocking agent events (--required Asks, tool WaitingApproval prompts, Budget holds, and Reviews once TASK-92.2 lands) when the browser tab is unfocused or in the background. Wires SessionManager pending tool approvals into inbox::Hq + inbox.changed SSE, adds a single shell-level SSE listener updating a live (N) unread tab-title badge across all routes, and adds privacy-safe Web Notifications with device-local /preferences controls and BroadcastChannel multi-tab deduplication. External Slack/email/mobile push escalation stays cleanly deferred to v2.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Registers SessionManager pending tool approvals (SessionState::WaitingApproval) as an inbox::Source in crates/org-office-server/src/inbox.rs and triggers LiveEvents::record_inbox_changed() on approval arrival/resolution so blocking Asks, Approvals, Budget holds, and (once TASK-92.2 lands) Reviews all drive inbox.changed SSE consistently.
- [ ] #2 Adds a single shell-level SSE listener in apps/agentic-org-ui/src/routes/(shell)/+layout.svelte (subscribing via appSession.controller().subscribeEvents to Feed::Organization + Feed::Messaging) that keeps inboxOpen live across every route (/home, /projects, /teams, etc.) and prefixes document.title with '(N) ' only when inboxOpen > 0 (keeping clean '<Area> · Agentic Org' titles when N === 0 per doc-39 §4).
- [ ] #3 Adds a Notifications section in /preferences (NotificationsSection.svelte, device-local localStorage per doc-39:58) with explicit user-gesture Notification.requestPermission() (never prompting on page load), a master Do-Not-Disturb toggle, and per-category toggles (approval, question/decision, budget/problem, review).
- [ ] #4 Fires privacy-safe Web Notification popups only when enabled, unmuted, tab hidden/unfocused, and deduplicated across open tabs via BroadcastChannel — showing category/actor/count metadata ONLY (zero DM bodies, private task notes, tool inputs, or secrets per ADR-0031), with click focusing the tab and navigating to /inbox (or item open.path); zero CSP regressions.
- [ ] #5 Adds unit + Playwright E2E tests (mocked Notification constructor + real SSE inbox.changed frame) covering permission gesture gating, (N) tab-badge lifecycle, DND/category mute, multi-tab dedupe, and zero body/secret leakage; updates doc-39 (§2.5 Notifications, §4 page title badge) and doc-3 (FC-035/FC-036).
<!-- AC:END -->
