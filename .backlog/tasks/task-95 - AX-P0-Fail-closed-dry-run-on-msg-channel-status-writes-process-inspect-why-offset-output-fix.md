---
id: TASK-95
title: >-
  AX P0: Fail-closed --dry-run on msg/channel/status writes & process inspect
  --why/--offset output fix
status: Done
assignee: []
created_date: '2026-11-23T04:54:00Z'
updated_date: '2026-11-24T11:17:00Z'
labels:
  - ax
  - backend
  - rust
  - cli
  - security
  - docs
milestone: m-0
dependencies:
  - TASK-85.1
documentation:
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
priority: high
type: bug
ordinal: 104000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Fixes two P0 CLI felt-safety and output-integrity bugs confirmed by the Principal AX Designer audit (AX-AUDIT-01 & AX-AUDIT-05): (1) all 12 write handlers in crates/org-agent-cli/src/msg_dispatch.rs ignore request.dry_run and execute live office mutations (sending messages/asks, waking recipients, changing channels/status) while org-command stamps meta.dry_run: true; (2) PROCESS_INSPECT_OUTPUT in process_table.rs:89-97 omits chain, winner, total_bytes, next_offset, and parent so shape::conform silently strips inheritance data on process inspect --why and chunking metadata on large docs, while process_dispatch.rs:169 hardcodes offset: None.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Fails closed with ErrorCode::Unsupported whenever request.dry_run is true across all 12 write handlers in crates/org-agent-cli/src/msg_dispatch.rs before invoking any office RPC, and adds an org-command/CLI contract test proving every Kind::Write either implements true dry-run validation or refuses without side effects.
- [x] #2 Adds declared optional output fields chain, winner, total_bytes, next_offset, and parent to PROCESS_INSPECT_OUTPUT and optional --offset integer input to PROCESS_INSPECT_INPUT (crates/org-agent-cli/src/process_table.rs:81-97), wiring offset in process_dispatch.rs:169.
- [x] #3 Adds end-to-end tests through org_command::run / execute_with_office proving process inspect --why preserves chain and winner through shape::conform, process inspect --offset paginates chunked (>512 KiB) documents, and msg/channel/status --dry-run produces zero mutations; updates doc-28 and doc-96 as needed.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed AX P0 fixes for dry-run safety and process inspect output/pagination: fail-closed ErrorCode::Unsupported on all 12 msg/channel/status write handlers before any RPC + exhaustive Kind::Write dry-run contract test (table.rs) + real-office forwarded/refused zero-mutation proof; declared chain/winner/total_bytes/next_offset/parent/team_id/project_id/scope_name and --offset/--revision on process inspect; stripped duplicate body_markdown from process.show/process.why responses, frame-bounded slice_utf8_chunk against MAX_FRAME_BYTES JSON-escape size, rejected non-UTF-8-boundary/out-of-range offsets, enforced revision requires document_id with Conflict + offset-0 recovery hint; updated doc-28, doc-34, doc-96, doc-3 (FC-168), and skill-catalog hash. Verified with cargo fmt/clippy, org-agent-cli 21/21, org-command 23/23, org-office processes 5/5, org-office-server task95_* real-socket 2/2, agentic-org-web process_routes 1/1, Repowise exit 0, Tier-1 Sol xhigh VERDICT: APPROVE, and separate fresh Tier-2 Sol max VERDICT: APPROVE.
<!-- SECTION:NOTES:END -->
