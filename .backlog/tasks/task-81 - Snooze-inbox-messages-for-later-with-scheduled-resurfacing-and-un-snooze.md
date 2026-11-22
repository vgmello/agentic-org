---
id: TASK-81
title: Snooze inbox messages for later with scheduled resurfacing and un-snooze
status: To Do
assignee: []
created_date: '2026-11-22T03:11:00Z'
labels:
  - backend
  - ui
  - inbox
  - messaging
milestone: m-0
dependencies:
  - TASK-46
documentation:
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/decisions/decision-3 - ADR-0003-Agent-messaging-is-office-first.md
priority: medium
type: feature
ordinal: 85000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Users receive messages and pending items in their Inbox that they cannot act on immediately but do not want to lose or leave cluttering the active queue. Organization Data 15 (doc-90) and ADR-0003 already model per-recipient snooze state (message_recipient_states.state = snoozed, snoozed_until, snooze_note), but Inbox snoozing is not yet wired end-to-end in the API, resurfacing lifecycle, or browser UI. Users need a fast way to defer an inbox message until a chosen time so it leaves the active queue and reliably returns when due.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Backend API persists per-recipient snooze (state = "snoozed", snoozed_until, optional snooze_note) and early un-snooze in message_recipient_states through the recipient own office (ADR-0003 Decision 11), never altering other recipients state and surviving restarts and projection rebuilds.
- [ ] #2 Snoozed messages are hidden from the active Inbox queue and excluded from the active Inbox badge count until snoozed_until is reached (or un-snoozed early), at which point they automatically resurface in the active Inbox and update live SSE streams and badge counts.
- [ ] #3 Inbox UI provides a Snooze control on messages with quick presets (e.g., Later today, Tomorrow, Next week computed in the viewer saved account timezone per doc-39) and a custom date/time picker, plus a Snoozed filter/view showing each item return time with one-click Un-snooze and Re-snooze.
- [ ] #4 Edge cases and access boundaries are enforced: unauthorized callers cannot snooze another employee inbox state, past timestamps are rejected, asks with an expires_at deadline clearly handle snoozing near or past expiry, and automated backend and UI tests verify snooze, un-snooze, resurfacing, timezone presets, and isolation.
<!-- AC:END -->
