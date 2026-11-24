---
id: TASK-42
title: >-
  C3: Start and supervise an agent — session runner, CLI enrollment, sub-agents,
  instructions, and recovery
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T18:23:00Z'
labels:
  - backend
  - rust
  - agents
  - m1
milestone: m-0
dependencies:
  - TASK-2
  - TASK-9
  - TASK-11
  - TASK-41
documentation:
  - docs/implementation-plan.md
  - docs/design/agent-sessions.md
  - docs/design/agent-instructions.md
  - docs/adr/0025-agents-start-sub-agents.md
  - docs/adr/0030-an-agent-works-while-it-has-work.md
  - docs/adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md
priority: high
type: feature
ordinal: 42000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Build the production office session runner over org-agent-runtime-claude (and prepare Codex/Agy adapters), private agent home with same-user isolation notice, AGENT_OS_* launch binding (refusing unbound agent-os), session inspect and grant list commands, ADR-0025 sub-agents and background-task tracking/reaping, ADR-0030 restart continuation and stall reaping (LIVE-9..13), built-in agent instructions (FC-171), ADR-0032 browser-driven CLI enrollment + wrapped credentials + permission_preset + default-on sandbox toggle (FC-189, D2), HR lead proposal drafting, and B2 Q9 placed-agent agent-os/LNX-2 checks.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A hired agent completes an instruction from its profile page in its private home, and the built-in HR lead drafts hire proposals when called.
- [x] #2 agent-os requires a live run binding (descendant check via org-platform on macOS and Linux) and serves session inspect and grant list (CLI-9, CLI-10, CLI-12).
- [x] #3 ADR-0025 SUB-1..5/SUB-7, ADR-0030 LIVE-9..13, built-in instruction scenarios (INSTR-4/5/7/8/12/15/21), and ADR-0032 ENROLL-1..7 pass.
- [x] #4 B2 Q9 placed-agent checks (agent-os who, offboarded bind refusal, LNX-2 container agent-os) pass and FEATURES rows FC-031/098/100..102/171/172/189 are updated.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered C3 production office session runner (crates/org-office-sessions), org-platform peer_pid & parent_pid/is_descendant_of (macOS/Linux/refused, 0 allowlist predicates), 12 typed session & provider-enrollment RPCs (org-proto, org-office-server, agentic-org-web), agent-os session inspect / grant list / who with run-binding token & descendant verification, ADR-0025 sub-agent tool subsetting/tracking/reaping, ADR-0030 crash recovery & persisted 3-stall lock, ADR-0032 browser CLI enrollment & wrapped credential custody + permission_preset policy, HR lead guidance & org.snapshot agent profiles, updated doc-3 rows FC-031/098/100..102/168/171/172/189, passed full cargo fmt/clippy/test --workspace + real-process C3 E2E, and obtained Separate Fresh Final Rust Reviewer APPROVE (d425a2b2).
<!-- SECTION:FINAL_SUMMARY:END -->
