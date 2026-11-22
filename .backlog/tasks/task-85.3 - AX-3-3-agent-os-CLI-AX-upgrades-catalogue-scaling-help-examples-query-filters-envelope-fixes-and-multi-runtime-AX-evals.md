---
id: TASK-85.3
title: >-
  AX 3/3: agent-os CLI AX upgrades (catalogue scaling, help examples, query
  filters, envelope fixes) and multi-runtime AX evals
status: To Do
assignee: []
created_date: '2026-11-22T14:37:00Z'
labels:
  - ax
  - backend
  - rust
  - cli
  - testing
  - docs
milestone: m-0
dependencies:
  - TASK-85.1
  - TASK-85.2
  - TASK-83
documentation:
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/specs/doc-59 - REQ-08-agent-os-CLI-and-Kernel-Primitives.md
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - >-
    .backlog/decisions/decision-14 -
    ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md
parent_task_id: TASK-85
priority: medium
type: feature
ordinal: 91000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit of crates/org-command and crates/org-agent-cli revealed concrete AX friction points that slow agents down or cause avoidable retries: the 4 KiB launch catalogue is near saturation (~55+ commands before doc/feature/release/milestone land), --help omits Command.example usage lines, session attach error envelopes violate envelope_schema() by omitting data/meta, ask status naming drifts between meta.status and data.status, several handlers emit terse Retry hints instead of actionable retry_with guidance, and task list drops total and lacks --count/--exclude-status/--unassigned/--modified-file filters. We also lack empirical multi-runtime AX evals (INSTR-1..23) verifying cold-start agent effectiveness across claude, codex, and agy.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Solves launch-catalogue scaling within the 4 KiB CATALOGUE_LIMIT (progressive resource-level discovery so adding doc/feature/release/milestone commands never breaks the cap), renders Command.example in --help output, fixes crates/org-agent-cli/src/attach.rs error_envelope() to conform strictly to envelope_schema() ({ok, data, meta, error}), reconciles ask status placement (meta.status vs data.status), and replaces generic hints with actionable error.hint / retry_with guidance.
- [ ] #2 Adds token-saving query ergonomics to agent-os task list (exposing total and --count, plus --exclude-status, --unassigned, and --modified-file substring filtering) and ensures pre-normalized aliases (who, task note) behave consistently in --help, --schema, and session attach.
- [ ] #3 Adds an automated AX evaluation suite covering doc-27 INSTR scenarios and cold-start agent onboarding / CLI retry-rate / token-budget checks across claude, codex, and agy adapters, reviewed by Principal AX Designer.
- [ ] #4 Updates doc-28 (catalogue scaling, task list filters, ask status, budget/task-note/worktree alignment), doc-59 (REQ-08 if needed), and doc-3 (FC-168, FC-171 evidence).
<!-- AC:END -->
