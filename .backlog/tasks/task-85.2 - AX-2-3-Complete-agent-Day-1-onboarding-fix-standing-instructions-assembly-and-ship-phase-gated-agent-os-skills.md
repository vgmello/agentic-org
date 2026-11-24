---
id: TASK-85.2
title: >-
  AX 2/3: Complete agent Day-1 onboarding, fix standing instructions assembly,
  and ship phase-gated agent-os skills
status: To Do
assignee: []
created_date: '2026-11-22T14:37:00Z'
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
parent_task_id: TASK-85
priority: high
type: feature
ordinal: 90000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Standing instructions in crates/org-office-sessions/src/instructions.rs currently contain only a ~1.8 KiB stub, invert doc-27 static-to-volatile prefix caching (putting volatile per-agent identity first and static command catalogue last), pass raw team_id instead of team name/ancestor leads, omit agent-os skill list|read, and use JSON field names instead of CLI flags. Meanwhile crates/org-process/src/skills.rs only has 5 terse stubs that omit budgets, task-note, task deps, msg unsnooze, checklists, and session attach, and cite removed command worktree prepare. This slice delivers a great Day-1 agent onboarding experience and a complete, Backlog.md-inspired phase-gated on-demand skill library inside the 10 KiB standing-prompt budget.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Fixes crates/org-office-sessions/src/instructions.rs and manager.rs to match doc-27 §1 static-to-volatile prefix caching order, stay <= 10 KiB (BUILTIN_INSTRUCTIONS_MAX_BYTES), resolve actual team name and ancestor leads, include all core operating sections and TurnTrigger shapes (including fenced untrusted-speaker blocks), point agents explicitly to agent-os skill list|read, and extend verify_command_references() to validate both commands and CLI flags against the live command table.
- [ ] #2 Replaces the 5 terse stubs in crates/org-process/src/skills.rs with a comprehensive, AX-reviewed built-in skill library covering Day-1 orientation (agent-os-basics) and on-demand domain/phase guides: tasks-create, tasks-execute, tasks-finalize (phase-gated discipline: search first, no speculative plans at creation, evidence before checking checklists, closure summary), messaging-and-asks, channels, code-and-worktrees, hiring-and-directory, processes-and-standards, budgets, errors-and-recovery, and batch-and-attach.
- [ ] #3 Automated tests verify every built-in skill and standing instruction references only real live commands and flags (removing stale worktree prepare and JSON-style flag wording), respects size budgets, and exercises Day-1 orientation and skill read flows.
- [ ] #4 Updates doc-27 (reconciling §4 sub-agent cross-turn lifetime with ADR-0025 decision 6 and aligning §6 skill keys with crates/org-process/src/skills.rs) and doc-3 (FC-171 evidence).
<!-- AC:END -->
