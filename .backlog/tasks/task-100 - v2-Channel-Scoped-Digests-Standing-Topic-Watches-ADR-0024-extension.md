---
id: TASK-100
title: 'v2: Channel Scoped Digests & Standing Topic Watches (ADR-0024 extension)'
status: To Do
assignee: []
created_date: '2026-11-24T02:28:00Z'
labels:
  - ax
  - backend
  - rust
  - messaging
  - agents
  - docs
milestone: m-1
dependencies:
  - TASK-99
documentation:
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - >-
    .backlog/decisions/decision-24 -
    ADR-0024-An-agents-inbox-holds-what-is-addressed-to-it-and-each-channel-has-a-watcher-that-feeds-responses-back-to-the-agents-who-asked.md
priority: medium
type: feature
ordinal: 114000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Extends ADR-0024 and TASK-99's v1 Channel Catch-Up Ladder with two opt-in v2 channel intelligence workflows for lead/coordinator agents: (1) channel-level scoped batched digests delivered cleanly at turn boundaries without waking idle agents for ordinary chatter; and (2) standing topic watches where the cheap tool-less watcher filters channel traffic off-context and drops only matching message_ids into the subscribing agent's inbox. Uses dedicated channel-level preference/watch storage (explicitly NOT overloading per-sender conversation_preferences.muted_employee_id in messaging-schema.sql:141-151) and amends ADR-0024/doc-90 cleanly.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Records an ADR-0024 amendment and updates doc-90 + messaging-schema.sql + test_schema.py with dedicated channel-level digest subscription and topic-watch tables (keeping per-sender conversation_preferences.muted_employee_id untouched) and explicit DeliveredBecause / inbox representation across Memory and SQLite (ADR-0018).
- [ ] #2 Implements turn-boundary channel digest injection for subscribed leads/coordinators (strictly never waking idle agents for routine chatter, bounded token cap, untrusted-data fenced, advancing digest watermark cleanly).
- [ ] #3 Implements standing topic watches evaluated off-context by the tool-less channel watcher (with strict no-wake invariant preventing cross-agent wake loops per ADR-0024 d12, org-level cost attribution, and multi-runtime compatibility beyond claude-code) plus agent-os CLI management verbs within CATALOGUE_LIMIT.
- [ ] #4 Adds WATCH-11-style precision/recall and AX-23 token-efficiency evaluation tests proving noise suppression without waking loops or context bloat.
<!-- AC:END -->
