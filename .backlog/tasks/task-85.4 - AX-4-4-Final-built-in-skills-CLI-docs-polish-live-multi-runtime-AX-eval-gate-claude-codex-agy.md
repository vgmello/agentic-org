---
id: TASK-85.4
title: >-
  AX 4/4: Final built-in skills & CLI docs polish + live multi-runtime AX eval
  gate (claude, codex, agy)
status: To Do
assignee: []
created_date: '2026-11-24T03:17:00Z'
updated_date: '2026-11-25T00:15:00Z'
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
  - >-
    .backlog/docs/design/doc-97 -
    Design-Embedded-Tess-—-Chief-of-Staff-and-Product-Assistant.md
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

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
2026-11-24 Agent Decision Capture AX eval gate (TASK-111/TASK-112): Include agent-os note (--kind decision, promote) and docs-and-decisions skill in the final v1 AX polish pass and add 5 empirical multi-runtime record-judged scenarios across claude/codex/agy (including cost-effective models): DEC-AX-1 (Anti-Spam: 0 decisions on mechanical task), DEC-AX-2 (Meaningful Fork: 1-2 decisions with exit 0, non-empty why/alternatives, 0 spurious Git ADRs), DEC-AX-3 (Turn-40 Compact Recall: <=1 call, <1 KiB stdout across 15 verbose scratch notes), DEC-AX-4 (Promotion & ADR-0031 Firewall: 0 private_note_id in Git, Agent B sees ADR only), DEC-AX-5 (Non-Task Standalone Decision without creating an unsolicited task).

2026-11-24 Embedded Tess AX eval gate (TASK-80.1/80.4): Add TESS-AX-1..6 live multi-runtime record-judged scenarios across claude -p, codex, and agy (flagship + cost-effective models): TESS-AX-1 (Product Q&A via skill read <=2 hops, <=2,500 uncached tok), TESS-AX-2 (Batched session attach org synthesis <50ms IPC, Brief read-only), TESS-AX-3 (ConfirmRequired policy_violation >=90% 1-turn recovery to suggestion create, 0 silent mutations, 0 raw JSON fences), TESS-AX-4 (acting_for_caller_id dual-intersection, ADR-0031 private-note denial, prompt-injection immunity, per-caller session isolation), TESS-AX-5 (Stale revision ConflictDetail recovery on proposals), TESS-AX-6 (3/3 runtime parity including Codex sandboxed agent-os shell, >=80% turn-2+ KV cache hit, 0 MCP servers).
<!-- SECTION:NOTES:END -->
