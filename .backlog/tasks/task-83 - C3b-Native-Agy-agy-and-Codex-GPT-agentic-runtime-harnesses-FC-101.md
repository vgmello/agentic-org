---
id: TASK-83
title: 'C3b: Native Agy (agy) and Codex (GPT) agentic runtime harnesses (FC-101)'
status: Done
assignee:
  - '@lead'
created_date: '2026-11-22T04:40:00Z'
updated_date: '2026-11-22T06:49:00Z'
labels:
  - backend
  - agents
  - m1
milestone: m-0
dependencies: []
priority: high
type: feature
ordinal: 86000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement first-class native agentic harness adapters for Agy (crates/org-agent-runtime-agy) and Codex/GPT (crates/org-agent-runtime-codex) behind org-agent-runtime::AgentRuntime alongside Claude (crates/org-agent-runtime-claude), replacing the fail-closed stubs in org-office-sessions::RuntimeRegistry. Include sans-I/O protocol/event parsing, env scrubbing & credential/token redaction, permission presets & sandbox integration, session resume/continuation, sub-agent/conversation transcript compatibility (C4 / REQ-05), and live/fake-CLI verification.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 crates/org-agent-runtime-agy implements AgentRuntime for the agy CLI (launch, stream/JSON event parsing, resume, usage/rate-limit events, permission/sandbox flags, env scrubbing, and redaction) and is wired into org-office-sessions RuntimeRegistry.
- [x] #2 crates/org-agent-runtime-codex implements AgentRuntime for the Codex/GPT CLI (launch, JSONL event parsing, resume, usage/rate-limit events, permission/sandbox flags, env scrubbing, and redaction) and is wired into org-office-sessions RuntimeRegistry.
- [x] #3 Provider connection, model profile, and session inspection (C4) support Claude, Codex/GPT, and Agy consistently, with unit and real-process fake-CLI tests passing and doc-3 FC-101 updated.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Implemented native sans-I/O Agy (org-agent-runtime-agy) and Codex (org-agent-runtime-codex) AgentRuntime/Session adapters sharing the exact org-agent-runtime primitives as Claude, wired both into org-office-sessions RuntimeRegistry, CliEnrollmentDriver, auth_env_var_for, and org-supervise REVIEWED_ENV_SWITCHES, and verified with testkit conformance (check_plan + check_all), real-process fake-CLI E2E (credential enrollment/injection, reply & C4 transcript redaction), Repowise exit 0, and 3-pass fresh Neon review (Pass 3 VERDICT: APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
