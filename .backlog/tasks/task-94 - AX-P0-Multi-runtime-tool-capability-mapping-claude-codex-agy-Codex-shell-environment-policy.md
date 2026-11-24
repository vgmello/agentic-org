---
id: TASK-94
title: >-
  AX P0: Multi-runtime tool capability mapping (claude/codex/agy) & Codex shell
  environment policy
status: To Do
assignee: []
created_date: '2026-11-23T04:54:00Z'
labels:
  - ax
  - backend
  - rust
  - agents
  - runtime
  - security
  - docs
milestone: m-0
dependencies:
  - TASK-85.1
documentation:
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - .backlog/docs/design/doc-29 - Design-Agent-Sessions-and-Runtime-Isolation.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
priority: high
type: bug
ordinal: 103000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Fixes two P0 multi-runtime blockers confirmed by the Principal AX Designer audit (AX-AUDIT-03 & AX-AUDIT-04) that break autonomous turn-1 execution on Codex and Agy and block TASK-85.3 live multi-runtime AX evals: (1) preset.rs and pump.rs:791 hardcode Claude-only tool names (Read, Glob, Grep, Edit, Write, Bash, Shell, Task, Agent), causing Codex (shell, apply_patch) to stall in WaitingApproval, Agy (run_command, read_file, edit_file) to fast-stop terminate on system/init in ReadOnly, and ReadOnly/WorkspaceEdits sessions to block on basic agent-os inbox calls; (2) Codex shell_environment_policy.include_only passes only the 6 AGENT_OS_* env names, stripping PATH, HOME, TMPDIR, LANG, and TERM from tool shells.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Introduces runtime-neutral tool capability classes (ToolCapability::{Read, Search, Edit, Shell, Subagent}) in org-agent-runtime and maps them inside each runtime adapter (claude, codex, agy) when translating PermissionPolicy and allowed_tools, eliminating hardcoded Claude tool strings from crates/org-office-sessions/src/preset.rs and normalizing sub-agent lifecycle events instead of matching Task/Agent in pump.rs:791.
- [ ] #2 Adds a safe CommandPrefix("agent-os") allow rule (strictly NO trailing whitespace so claude/src/plan.rs:600-610 does not drop it) for ReadOnly and WorkspaceEdits presets with strict word-boundary matching (cmd == prefix || cmd.starts_with("agent-os ")) and fail-closed rejection of shell chaining/redirection metacharacters (;, &&, ||, |, backticks, $(, >, <, newlines) in preset.rs:matches_any_allow.
- [ ] #3 Updates crates/org-agent-runtime-codex/src/plan.rs:251-268 so shell_environment_policy.include_only preserves office-curated baseline POSIX variables (PATH, HOME, TMPDIR, LANG, TERM) alongside ctx.office_env_names while continuing to exclude ambient secrets, and adds a real-shell conformance test verifying agent-os resolves on PATH with AGENT_OS_SESSION_TOKEN present.
- [ ] #4 Verifies ReadOnly, WorkspaceEdits, and AutonomousSandboxed policy translation and approval/init checks across claude, codex, and agy adapters with unit/conformance tests, and updates doc-29, doc-96, and doc-3 (FC-101 evidence).
<!-- AC:END -->
