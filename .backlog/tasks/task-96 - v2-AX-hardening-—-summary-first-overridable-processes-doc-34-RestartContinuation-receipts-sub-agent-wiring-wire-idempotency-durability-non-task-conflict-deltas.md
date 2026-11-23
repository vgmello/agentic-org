---
id: TASK-96
title: >-
  v2: AX hardening — summary-first overridable processes (doc-34),
  RestartContinuation receipts, sub-agent wiring, wire idempotency/durability &
  non-task conflict deltas
status: To Do
assignee: []
created_date: '2026-11-23T04:54:00Z'
labels:
  - ax
  - backend
  - rust
  - cli
  - agents
  - docs
milestone: m-1
dependencies:
  - TASK-85.3
  - TASK-90
documentation:
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - .backlog/docs/design/doc-34 - Design-Organization-Processes-and-Standards.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - >-
    .backlog/decisions/decision-14 -
    ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md
  - >-
    .backlog/decisions/decision-25 -
    ADR-0025-An-agent-can-start-sub-agents-inside-its-turn.md
  - >-
    .backlog/decisions/decision-30 -
    ADR-0030-An-agent-works-while-it-has-work-continues-after-a-restart-and-a-stalled-turn-is-stopped.md
priority: medium
type: enhancement
ordinal: 105000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Consolidates Stage-2 verified v2 AX improvements from the Principal AX Designer audit (AX-AUDIT-07 Part B, 09 Part B, 11 Part B, 15, 18, 19 process update, 20, and optional 14 NotBound cleanup) that require wire/spec evolution beyond v1 P0/P1 scope.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Amends doc-34 §3.2 (PROC-7) and crates/org-office-sessions/src/manager.rs:1746-1769 + org-process/src/render.rs to default cold-start overridable process injection to a summary-first index with persistent read_blobs tracking and on-demand process inspect, reclaiming up to ~22 KiB per cold start.
- [ ] #2 Upgrades RestartContinuation tracking in crates/org-office-server/src/serve.rs:1162-1179 (ADR-0030 decision 8) to record bounded (<= 20) mutating-only receipts with CLI command name and returned entity IDs/keys instead of bare internal RPC method names including reads.
- [ ] #3 Wires office-owned sub-agent definitions (replacing dead runtime_dir/agents/general.json in manager.rs:1342 / subagents.rs:99-124 and injecting must-follow org standards per ADR-0025 decision 9) through SessionSpec across claude, codex, and agy adapters.
- [ ] #4 Adds wire command_id deduplication for the 8 secondary messaging/status RPCs, optional durable: bool upgrade for committed-class messaging RPCs (ADR-0014 d4), partial-field process update, msg reply --answer key=value sugar, ConflictDetail/retry_with.if-match across non-task revisioned domains (role, agent, member, team, process, task-note), consistent -h handling, and optional NotBound contract alignment (office.rs:142-160 + cli.rs + READ/WRITE_ERRORS).
<!-- AC:END -->
