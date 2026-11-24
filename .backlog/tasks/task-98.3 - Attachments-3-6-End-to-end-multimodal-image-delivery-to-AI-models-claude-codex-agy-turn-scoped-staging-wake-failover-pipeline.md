---
id: TASK-98.3
title: >-
  Attachments 3/6: End-to-end multimodal image delivery to AI models (claude,
  codex, agy), turn-scoped staging & wake/failover pipeline
status: To Do
assignee: []
created_date: '2026-11-24T01:57:00Z'
labels:
  - backend
  - rust
  - agents
  - runtime
  - multimodal
  - attachments
  - security
  - ax
  - docs
milestone: m-0
dependencies:
  - TASK-98.2
  - TASK-94
documentation:
  - .backlog/docs/design/doc-29 - Design-Agent-Sessions-and-Runtime-Isolation.md
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - >-
    .backlog/docs/organization-data/doc-88 -
    Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
parent_task_id: TASK-98
priority: high
type: feature
ordinal: 109000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers pasted/uploaded image attachments all the way into AI model turns across claude, codex, and agy using Normalized Dual-Delivery while preserving Session sans-I/O purity, doc-88:242 ephemeral staging, prompt-injection fencing, and zero base64 bloat in durable session files.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Extends org-agent-runtime with exhaustive Capabilities::image_input (InlineBase64 | LocalPath | StagedFileOnly | None), TurnImage in Control::Send (keeping Session 100% sans-I/O), and MAX_OUTGOING_LINE_BYTES = 8 MiB in line.rs; runs 1-second live binary schema/protocol probes on claude, codex, and agy before wiring adapters.
- [ ] #2 Implements adapter multimodal delivery: Claude emits Anthropic base64 image content blocks (<= 4 inline × <= 1 MiB derivative); Codex emits localImage items on BOTH turn/start and turn/steer (zero base64 rollout bloat); Agy uses safe StagedFileOnly (never crashing with exit 1 on non-text stream-json blocks per doc-23:384; using InlineBase64 only if live probe proves support), verified by runtime testkit & opt-in live smoke tests.
- [ ] #3 Configures stable <runtime_dir>/attachments in SessionSpec.read_only_dirs (preserving spec_hash() stability across turns; adding read-only/write-deny support across claude, codex, and agy), stages ephemeral 0400 turn-scoped files in <runtime_dir>/attachments/<turn_id>/<att_id>.<ext>, and purges <turn_id>/ immediately in handle_turn_ended (pump.rs:823), finalize_child (pump.rs:1010), and startup recovery (doc-88:242).
- [ ] #4 Threads lightweight TurnAttachmentRef / attachment_ids ONLY (zero base64 on disk!) through wake_bridge.rs, PendingWakeItem, PendingAskAnswer, PumpCommand::SendTurn, ActiveTurnRecord, history.jsonl, and FailoverRequest (so §7 pool failover across heterogeneous runtimes re-resolves images for the target adapter); wraps image manifests with explicit untrusted-data & image prompt-injection framing (doc-27 §5) and degrades fail-soft (text + staged path + notice) on non-vision models, unsupported runtimes, or pre-side-effect provider image rejections.
- [ ] #5 Updates doc-29, doc-27, doc-88, doc-96, and doc-3.
<!-- AC:END -->
