---
id: TASK-99
title: >-
  AX Channel Catch-Up Ladder (v1): channel watermarks, --since-read/--after-id
  delta reads, channel mark-read, thread-first headers, and channel summarize
  --since-read
status: To Do
assignee: []
created_date: '2026-11-24T02:28:00Z'
labels:
  - ax
  - backend
  - rust
  - cli
  - messaging
  - docs
milestone: m-0
dependencies:
  - TASK-85.3
documentation:
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - >-
    .backlog/decisions/decision-24 -
    ADR-0024-An-agents-inbox-holds-what-is-addressed-to-it-and-each-channel-has-a-watcher-that-feeds-responses-back-to-the-agents-who-asked.md
priority: high
type: feature
ordinal: 113000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implements the Principal AX Designer's 4-rung token-efficient channel catch-up ladder for lead/coordinator/specialist agents (all Stage-2 verified against messaging-schema.sql, chat.rs, and org-command): (Rung 1) channel list exposes real watermark-based new_message_count alongside inbox unread_count plus last_read_message_id/last_read_at/last_message_id/last_message_at and --unread-only; (Rung 2) msg list --channel supports --since-read and --after-id <msg-id> (oldest-first deterministic delta, never unstable --after-seq) paired with agent-os channel mark-read; (Rung 3) channel summarize supports --since-read with optional --question as a strictly pure Kind::Read (no --mark-read side effect per ADR-0014, returning through_message_id/new_message_count/remaining_unread with 0-message short-circuit before resolve_watching_runtime); (Rung 4) msg list supports --top-level-only and --headers-only (omitting body, returning char-safe <=160-char snippet, body_bytes, reply_count, latest_reply_at, attachment_count). Preserves untrusted-data fencing (<<<DATA>>> + DERIVED_SUMMARY_NOTICE) and zero-polling ADR-0024 discipline.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Rung 1 (channel list watermarks): Fixes ConversationSummary / chat.channels to compute watermark-based new_message_count (messages after last_read_message_id excluding self, working around ADR-0024 d6 where ordinary channel messages have no message_recipient_states rows) alongside existing inbox unread_count; exposes new_message_count, unread_count, last_read_message_id, last_read_at, last_message_id, and last_message_at on agent-os channel list and supports --unread-only (matching new_message_count > 0 || unread_count > 0) across Memory and SQLite.
- [ ] #2 Rung 2 (delta reads & channel mark-read): Adds --since-read (anchored on conversation_read_markers.last_read_message_id) and --after-id <message-id> (UUIDv7 anchor, oldest-first deterministic (created_at, local_seq, id) order — never unstable local_seq/--after-seq) to chat.messages and agent-os msg list --channel (and ensures --unread-only on msg list --channel returns an actionable hint pointing to --since-read); exposes agent-os channel mark-read --channel <ref> [--up-to <msg-id>] (Kind::Write, Ack::Committed, --dry-run safe, within TASK-85.3 4 KiB catalogue headroom) and fixes decide_mark_read (features.rs:2031-2097) to accept #channel/name refs, default omitted --up-to to the channel's latest message ID (never erasing the watermark), advance monotonically, and never mutate message_recipient_states.
- [ ] #3 Rung 3 (delta-digest channel summarize): Adds --since-read to ChatChannelSummarizeParams and agent-os channel summarize, making --question optional when --since-read is set (defaulting to a concise decisions/blockers/handoffs/open-questions catch-up prompt citing message IDs); keeps channel summarize strictly pure Kind::Read (NO --mark-read mutation per ADR-0014/org-command); short-circuits 0 new messages BEFORE resolve_watching_runtime (watcher.rs:1206-1216) for instant zero-LLM-call responses even without a watching model; slices oldest-first up to 200 messages returning through_message_id, new_message_count, and remaining_unread so >200 messages never silently skip; preserves <<<DATA>>> untrusted fencing and DERIVED_SUMMARY_NOTICE.
- [ ] #4 Rung 4 (thread-first headers): Adds reply_count, latest_reply_at (single O(N) pass over thread_root_id), and attachment_count (from MessageRecord.attachments, aligned with TASK-98.2) to ChatMessageView, plus --top-level-only and explicit --headers-only (omits body, populates char-safe <=160-char single-line snippet + body_bytes without duplicating snippet on full-body reads) on chat.messages and agent-os msg list; documents explicit --since-read --top-level-only semantics for replies in older threads.
- [ ] #5 Dual-engine (Memory + SQLite) tests, org_command::testkit contract checks (verifying CATALOGUE_LIMIT <= 4096), and AX-23 token-reduction eval hook pass; updates crates/org-process/src/skills.rs (channels skill, verified by verify_command_references), doc-28, and doc-96.
<!-- AC:END -->
