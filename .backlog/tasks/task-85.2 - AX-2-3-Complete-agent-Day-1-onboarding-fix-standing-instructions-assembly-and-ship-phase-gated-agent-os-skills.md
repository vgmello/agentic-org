---
id: TASK-85.2
title: >-
  AX 2/4: Agent Day-1 onboarding, prefix-cached instructions, wake fencing,
  baseline skills & flag-level drift guard
status: To Do
assignee: []
created_date: '2026-11-22T14:37:00Z'
updated_date: '2026-11-24T03:17:00Z'
labels:
  - ax
  - backend
  - rust
  - agents
  - skills
  - docs
milestone: m-0
dependencies:
  - TASK-85.1
  - TASK-44
documentation:
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/design/doc-34 - Design-Organization-Processes-and-Standards.md
  - .backlog/docs/specs/doc-54 - Spec-Organization-Processes-and-Standards-UX.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - >-
    .backlog/decisions/decision-25 -
    ADR-0025-An-agent-can-start-sub-agents-inside-its-turn.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
parent_task_id: TASK-85
priority: high
type: feature
ordinal: 90000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Standing instructions in crates/org-office-sessions/src/instructions.rs currently contain only a ~1.8 KiB stub, invert doc-27 static-to-volatile prefix caching (putting volatile per-agent identity first and static command catalogue last), pass raw team_id instead of team name/ancestor leads, omit agent-os skill list|read, and use JSON field names instead of CLI flags; meanwhile manager.rs:2410-2414 + runtime adapters (claude/codex/agy plan.rs) prepend volatile per-agent spec.agent.prompt ahead of static instructions and pass per-wake task_ref into Part 2b, breaking cross-agent and cross-task KV prefix caching, and wake.rs:284-348 injects raw peer/sender content across all autonomous wakes with zero untrusted-content fencing. In crates/org-process/src/skills.rs, the 5 terse stubs omit budgets, task-note, task deps, msg unsnooze, checklists, and session attach, and contain 4 real broken flags/claims (--dry-run claim, msg ask --prompt, channel summarize --about, task transition --reason + missing --if-match) — NOTE: worktree prepare IS live end-to-end in git_table.rs/git_dispatch.rs/git_calls.rs and must be kept. This slice delivers Day-1 agent onboarding, true cross-agent prefix caching, autonomous wake fencing, and a complete Backlog.md-inspired phase-gated on-demand skill library inside the 10 KiB standing-prompt budget.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Fixes crates/org-office-sessions/src/instructions.rs, manager.rs, wake.rs:284-348, and claude/codex/agy plan.rs to achieve true doc-27 §1 static-to-volatile prefix caching (static/shared spec.agent.prompt at byte 0, per-agent identity/workspace and per-wake task_ref process context in the volatile tail), stay <= 10 KiB (BUILTIN_INSTRUCTIONS_MAX_BYTES), resolve actual team name and ancestor leads, fence untrusted external/peer content across BOTH instructions.rs TurnTriggers AND wake.rs autonomous wakes (with sender authority/relation and actionable reply/accept CLI hints), point agents explicitly to agent-os skill list|read, and extend verify_command_references() to validate both commands and CLI flags via org_command::parse.
- [ ] #2 Replaces the 5 terse stubs in crates/org-process/src/skills.rs with a comprehensive, AX-reviewed built-in skill library covering Day-1 orientation (agent-os-basics) and on-demand domain/phase guides: tasks-create, tasks-execute, tasks-finalize (phase-gated discipline: search first, no speculative plans at creation, evidence before checking checklists, closure summary), messaging-and-asks, channels, code-and-worktrees (keeping live worktree prepare), hiring-and-directory, processes-and-standards, budgets, errors-and-recovery, and batch-and-attach — fixing all 4 audited inaccuracies (--dry-run scope, msg ask flags, channel summarize --question, task transition --blocked-reason + --if-match).
- [ ] #3 Automated tests dry-parse every agent-os command and CLI flag cited in standing instructions and built-in skills against the live CommandTable via org_command::parse (catching non-existent flags and missing required options like --if-match), verify byte budgets, and exercise Day-1 orientation, wake fencing, and skill read flows.
- [ ] #4 Updates doc-27 (reconciling §4 sub-agent cross-turn lifetime with ADR-0025 decision 6, fixing stale channel summarize --about at L228, and aligning §6 skill keys with crates/org-process/src/skills.rs), doc-96, and doc-3 (FC-171 evidence).
<!-- AC:END -->
