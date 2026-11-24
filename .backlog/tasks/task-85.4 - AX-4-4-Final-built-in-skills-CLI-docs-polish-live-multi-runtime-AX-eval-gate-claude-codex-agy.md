---
id: TASK-85.4
title: >-
  AX 4/4: Final built-in skills & CLI docs polish + live multi-runtime AX eval
  gate (claude, codex, agy)
status: To Do
assignee: []
created_date: '2026-11-24T03:17:00Z'
labels:
  - ax
  - backend
  - rust
  - cli
  - skills
  - testing
  - docs
milestone: m-0
dependencies:
  - TASK-85.2
  - TASK-85.3
  - TASK-90
  - TASK-92.1
  - TASK-94
  - TASK-95
  - TASK-98.4
  - TASK-99
documentation:
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
parent_task_id: TASK-85
priority: medium
type: feature
ordinal: 115000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Postponed final v1 AX acceptance gate executed once the complete v1 agent-os CLI surface (TASK-85.3, TASK-90 surgical tasks, TASK-92.1 reviews, TASK-94 multi-runtime capabilities/Codex env, TASK-95 felt-safety fixes, TASK-98.4 attachments, and TASK-99 channel catch-up ladder) has landed. Avoids rewriting built-in skills repeatedly mid-flight and runs empirical live multi-runtime AX benchmarks against the real final v1 CLI.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Runs a holistic Principal AX Designer pass across crates/org-process/src/skills.rs and crates/org-office-sessions/src/instructions.rs covering the complete v1 CLI surface (tasks, checklists, conflicts, reviews, attachments, channel catch-up ladder, asks, batch/attach, error recovery), ensuring every skill is concise (~1 page), consistent, and 100% verified by org_command::parse flag-level drift checks within the 10 KiB prompt and 4 KiB catalogue budgets.
- [ ] #2 Ships an automated live multi-runtime AX evaluation suite across claude, codex, and agy adapters verifying doc-27 INSTR-1..23 and doc-96 AX-20..AX-24 quality gates: cold-start orientation (<= 2 orientation calls), CLI usage error rate (<= 5%), self-healing retry-to-recovery (>= 90%), and AX-23 token-overhead benchmarks (including --since-read channel catch-up and --fields/ConflictDetail savings).
- [ ] #3 Updates doc-96 §8 (calibrating quantitative AX-20..AX-22 thresholds from real claude/codex/agy receipts including cost-effective models per doc-27 §8), doc-28, and doc-3 (FC-168, FC-171 final evidence).
<!-- AC:END -->
