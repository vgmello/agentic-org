---
id: TASK-98.2
title: >-
  Attachments 2/6: Chat, Structured Asks, Ask Answers & Unified Inbox image
  attachments
status: To Do
assignee: []
created_date: '2026-11-24T01:57:00Z'
labels:
  - backend
  - rust
  - messaging
  - inbox
  - attachments
  - security
milestone: m-0
dependencies:
  - TASK-98.1
documentation:
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-45 - Spec-Hiring-Profiles-and-Team-Moves-UX.md
  - >-
    .backlog/docs/specs/doc-55 -
    REQ-04-Channel-Messaging-and-Question-Components.md
parent_task_id: TASK-98
priority: high
type: feature
ordinal: 108000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Wires first-class image attachments end-to-end across direct messages, channels, thread replies, structured Asks, Ask Answers, Handoffs, and the Unified Inbox, ensuring images promote atomically from staging to durable/blobs/ before journal commit and obey conversation/owner authorization.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Extends chat.send, msg.send, msg.reply, chat.ask, msg.ask, and chat.answer RPC params with optional attachment_ids (max 10 per message/ask/answer), permitting image-only messages and replies (empty body_markdown allowed when attachment_ids is non-empty), promoting staged uploads atomically to durable/blobs/ before journal commit, and preserving exact-retry idempotency checks including attachments without breaking MessageRecord::compute_content_sha256.
- [ ] #2 Updates chat.answer (crates/org-office/src/messaging/asks.rs:1199-1338) so answer attachments persist in BOTH ParticipantAnswerFile (AskAnswerRecord) AND the generated reply MessageRecord (replacing hardcoded Vec::new() at L1336), projecting into message_answer_attachments and message_attachments across Memory and SQLite.
- [ ] #3 Exposes AttachmentMeta lists on ChatMessageView, ChatInboxItem, AskAnswerRecord, and Unified Inbox items (org-proto/src/inbox.rs + org-office-server/src/mailbox_store.rs:309-389) and supports attachment_ids on Inbox inline Reply and Deny actions, with has:image filter support.
- [ ] #4 Enforces strict conversation membership / owner-read policy on every attachment read and attach (rejecting cross-user staged attachment hijacking and unauthorized conversation reads fail-closed) with dual-engine Memory/SQLite and Delete+Rebuild tests.
<!-- AC:END -->
