---
id: TASK-15
title: >-
  Thin out agentic-org-office and agent-os binaries, wire org-command, delete
  org-spool, cut wire IDs to UUIDv7, and rename gateway symbols
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - rust
  - architecture
  - p2
milestone: m-4
dependencies:
  - TASK-14
references:
  - apps/agentic-org-office/src/serve.rs
  - apps/agent-os/src/main.rs
  - crates/org-office/src/organization/operation_id.rs
  - crates/org-proto/src/gateway.rs
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/architecture-non-negotiables.md
  - docs/design/project-structure.md
priority: medium
type: enhancement
ordinal: 15000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings H-03, H-04, L-04: apps/agentic-org-office (19.1k LOC, Ce=9) and apps/agent-os (2,975 LOC in 2 files) violate Non-Negotiable #15 ('logic in libraries, thin binaries'). agent-os still depends on legacy org-spool (4,849 LOC) while ignoring built org-command (Ca=0), wire/host/office operation IDs remain ULIDs lossily mapped via from_wire_ulid (Non-Negotiables #4, #14), and residual gateway identifiers persist across 4 crates.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Office server/IPC/HQ-creation orchestration is extracted from apps/agentic-org-office into library crates so the binary acts as a thin composition root.
- [ ] #2 apps/agent-os is wired onto org-command and legacy org-spool is deleted (per B6 / Non-Negotiable #14).
- [ ] #3 Wire, host, and office operation IDs use native UUIDv7 end-to-end and the lossy from_wire_ulid shim is deleted.
- [ ] #4 Residual gateway module/symbol names are renamed to web and unreached crate wiring milestones + test platform-conditional allowlist entries are tracked cleanly.
- [ ] #5 cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, and cargo test --workspace pass.
<!-- AC:END -->
