---
id: TASK-46
title: >-
  B7: Structured asks, shadcn-svelte question components, answers, and timeouts
  (REQ-04)
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T22:56:00Z'
labels:
  - backend
  - ui
  - messaging
  - m1
milestone: m-0
dependencies:
  - TASK-45
documentation:
  - docs/implementation-plan.md
  - docs/specs/req-04-channel-messaging-and-question-components.md
  - docs/adr/0020-required-asks-suspend-and-resume.md
priority: high
type: feature
ordinal: 46000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement 1-to-8 question asks with defaults, timeouts, short-hold msg ask / msg wait --ask / msg reply --answer / msg withdraw CLI commands, shadcn-svelte/Bits UI question cards in chat (single choice, multi choice, yes/no, free text), and live Home/Inbox action surfacing (FC-035, FC-079, FC-166).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All four question component types can be asked via CLI, answered together in the browser chat UI, and returned to the asker.
- [x] #2 msg ask returns within the short hold with answer or waiting+question ID, and defaults never auto-answer required asks (ASK-1, ASK-2, ASK-10).
- [x] #3 Live Home and unified Inbox surface pending asks/approvals and FC-035/FC-079 are updated.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered B7 structured asks (choice/yes_no/free_text, 1..=8 questions), bounded wait/short-hold (ASK-1, ASK-10), required-ask invariants (ADR-0020 d7/13/14), answer & withdrawal persistence with SQLite/Memory rebuild parity, live owner Inbox surfacing, agent-os msg ask/reply --answer/withdraw/wait --ask CLI, and shadcn-svelte QuestionForm + live Chat/Home/Inbox SSE UI. Both Rust (Pass 3 APPROVE) and UI (Pass 2 APPROVE) slices landed on v1.
<!-- SECTION:FINAL_SUMMARY:END -->
