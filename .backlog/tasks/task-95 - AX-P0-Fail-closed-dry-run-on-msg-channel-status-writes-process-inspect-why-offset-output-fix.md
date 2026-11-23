---
id: TASK-95
title: >-
  AX P0: Fail-closed --dry-run on msg/channel/status writes & process inspect
  --why/--offset output fix
status: To Do
assignee: []
created_date: '2026-11-23T04:54:00Z'
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
- [ ] #1 Fails closed with ErrorCode::Unsupported whenever request.dry_run is true across all 12 write handlers in crates/org-agent-cli/src/msg_dispatch.rs before invoking any office RPC, and adds an org-command/CLI contract test proving every Kind::Write either implements true dry-run validation or refuses without side effects.
- [ ] #2 Adds declared optional output fields chain, winner, total_bytes, next_offset, and parent to PROCESS_INSPECT_OUTPUT and optional --offset integer input to PROCESS_INSPECT_INPUT (crates/org-agent-cli/src/process_table.rs:81-97), wiring offset in process_dispatch.rs:169.
- [ ] #3 Adds end-to-end tests through org_command::run / execute_with_office proving process inspect --why preserves chain and winner through shape::conform, process inspect --offset paginates chunked (>512 KiB) documents, and msg/channel/status --dry-run produces zero mutations; updates doc-28 and doc-96 as needed.
<!-- AC:END -->
