---
id: TASK-45
title: >-
  B6: Conversations, channels, direct messages, presence, and live event streams
  on org-store
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T21:39:00Z'
labels:
  - backend
  - rust
  - messaging
  - m1
milestone: m-0
dependencies:
  - TASK-10
  - TASK-15
  - TASK-16
  - TASK-33
  - TASK-34
  - TASK-35
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/15-messaging-and-chat.md
  - docs/adr/0003-agent-messaging-office-first.md
  - docs/adr/0014-acknowledgement-classes-and-agent-os-latency.md
priority: high
type: feature
ordinal: 45000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Build the messaging family on org-store (DM, group DM, channel, per-employee read state/reactions/prefs) with committed local send and background relay (ACK-1), inbox reasons and @handle mentions (WATCH-1/2/9), msg/channel/status/session attach CLI rows, live subscribe and /events feeds (org.changed, message.received, member.presence_changed, inbox.changed), and restored LNX-1 agent-os messaging checks (FC-036, FC-074..078, FC-080..081, FC-160, FC-170).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Engine-independent messaging conformance suite passes on memory and SQLite, and ACK-1 latency is measured and recorded on macOS.
- [x] #2 msg send|reply|accept|inbox|read|list|done|defer|wait, channel list|join|leave, status set|clear, and session attach run over the office with no file-queue fallback (CLI-13, CLI-15, WATCH-1/2/9).
- [x] #3 Live subscribe and /events deliver org.changed, message.received (header only), member.presence_changed, and inbox.changed with restart/resume semantics.
- [x] #4 CHAT send/read/channel criteria and LNX-1 agent-os send/read pass and FEATURES rows are updated.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Prerequisite from TASK-15 (UUIDv7 wire cutover): org-proto::web::CommandEnvelope now emits canonical lowercase UUIDv7 for OperationKind::Msg (id), while legacy crates/org-mailbox/src/mailbox.rs still checks ulid_time_ms(&params.id). When wiring B6 messaging onto org-store, validate msg.send id via org_proto::uuid_v7_time_ms.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered B6 messaging on org-store (isolated protected/messaging.db on SQLite + Memory parity, DMs/group DMs/auto+manual channels, @handle mention extraction, inbox delivery reasons, reactions/read markers/mutes, handoff accept, presence/status lines), live subscribe + chat.events_poll + /api/v1/events/stream + /api/v1/organizations/{org}/chat/* BFF routes, agent-os msg/channel/status/session attach CLI commands, ACK-1 macOS measurement (2.48 ms/cmd avg across 30 commands in doc-66), and updated doc-3 feature checklist. Verified across full Rust workspace + 4 independent fresh review passes culminating in VERDICT: APPROVE (78c870ac) and Repowise pre-commit health exit 0.
<!-- SECTION:FINAL_SUMMARY:END -->
