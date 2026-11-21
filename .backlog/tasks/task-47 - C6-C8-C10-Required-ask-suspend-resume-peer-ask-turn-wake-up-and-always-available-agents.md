---
id: TASK-47
title: >-
  C6, C8 & C10: Required-ask suspend/resume, peer-ask turn wake-up, and
  always-available agents
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T23:58:00Z'
labels:
  - backend
  - rust
  - agents
  - messaging
  - m1
milestone: m-0
dependencies:
  - TASK-46
  - TASK-42
documentation:
  - docs/implementation-plan.md
  - docs/adr/0020-required-asks-suspend-and-resume.md
  - docs/adr/0023-an-agents-ask-starts-a-turn.md
  - docs/adr/0028-an-agent-is-always-available.md
  - docs/adr/0030-an-agent-works-while-it-has-work.md
priority: high
type: feature
ordinal: 47000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Deliver autonomous agent wake-up lifecycle: suspend and resume an agent in the same session on unanswered required asks (C6, ASK-3..12), start turns for idle agents on peer asks/handoffs/mentions/answers with pending-start retry and hourly limits (C8, WAKE-1..7, LIVE-1..8), and auto-start on human DMs, mentions, and task assignments with Available presence reporting (C10, AVAIL-1..6, HIRE-2/7) (FC-166, FC-169, FC-186).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Unanswered required asks cleanly suspend the agent and resume the same session upon answer (ASK-3, ASK-5..7, ASK-9, ASK-11..12, presence part of ASK-4).
- [x] #2 Peer asks, handoffs, mentions, and answers start turns for idle agents under the hourly limit with pending-start retries and deadlock-free mutual wake (WAKE-1..7, LIVE-1, LIVE-4..8).
- [x] #3 Human DMs, mentions, and task assignments wake idle agents automatically while displaying Available presence (AVAIL-1..6, HIRE-2, HIRE-7).
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered C6 required-ask suspend/resume (ASK-3..9, ASK-11..12), C8 peer-ask/handoff/mention turn wake-ups with coalescing, hourly limit, person exemption, and LIVE-8 deadlock breaker (WAKE-1..7, LIVE-1, LIVE-3..8), and C10 always-available presence and recipient presence overlay (AVAIL-1..6) across org-proto, org-office, org-office-sessions, and org-office-server. Verified with 21/21 org-office-sessions, 272/272 org-office-server, 22/22 org-office directory, 6/6 org-office messaging, 221+12 org-proto tests, cargo clippy -D warnings, Repowise health exit 0, and 3-pass fresh Rust review (final VERDICT: APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
