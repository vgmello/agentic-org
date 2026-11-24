---
id: TASK-85
title: >-
  Agent Experience (AX) discipline: Principal AX Designer persona/skill, AX
  standard, agent-os onboarding & skills, and CLI AX upgrades
status: To Do
assignee: []
created_date: '2026-11-22T14:37:00Z'
updated_date: '2026-11-23T03:36:00Z'
labels:
  - ax
  - agents
  - cli
  - skills
  - docs
  - dx
  - blog
milestone: m-0
dependencies:
  - TASK-44
  - TASK-83
documentation:
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/specs/doc-59 - REQ-08-agent-os-CLI-and-Kernel-Primitives.md
  - .backlog/docs/design/doc-62 - Design-Documentation-Authority-Map.md
  - .backlog/docs/doc-2 - Start-Here.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - .backlog/docs/doc-4 - Feature-Areas.md
  - .claude/agents/principal-designer.md
  - .agent-kit/project-skills/principal-designer/SKILL.md
priority: high
type: feature
ordinal: 88000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Agents are first-class citizens in Agentic Org: UX (owned by Principal Designer) governs the browser UI for people, while AX (Agent Experience) must become an equally rigorous, first-class vertical governing how agents onboard onto, discover, learn, and operate agent-os across the entire product (tasks, messaging, asks, channels, git worktrees, HR hiring, processes, budgets, and coordination). Today agent-os has strong foundations (org-command, --schema, session attach, skill list/read) but lacks a dedicated Principal AX Designer persona/skill to guide AX design decisions, has incomplete standing instructions (1.8 KiB stub with inverted prefix-caching order), ships only 5 tiny built-in skills with stale commands, and has concrete CLI AX friction (4 KiB catalogue scaling risk, missing --help examples, attach envelope mismatch, missing list count/filters). This initiative establishes AX as a first-class vertical, creates the Principal AX Designer persona & project skill (mirroring principal-designer across Claude and Codex), ships phase-gated on-demand agent-os skills and Day-1 agent onboarding, and hardens agent-os CLI ergonomics across claude, codex, and agy.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Principal AX Designer persona (.claude/agents/principal-ax-designer.md, .codex/agents/principal-ax-designer.toml) and project skill (.agent-kit/project-skills/principal-ax-designer/) exist alongside principal-designer, with an authoritative AX Spec in .backlog/docs/specs/ registered in doc-62, doc-2, doc-4, and doc-3.
- [ ] #2 Agent standing instructions assembly (crates/org-office-sessions/src/instructions.rs) follows doc-27 static-to-volatile prefix caching, stays <= 10 KiB, resolves real team/lead identity, covers all active agent-os capabilities, points agents to on-demand skills, and verifies both commands and flags against the live command table.
- [ ] #3 Product-side agent-os skill list|read (crates/org-process/src/skills.rs) ships a complete, verified, phase-gated skill library covering Day-1 agent onboarding and every agent-os domain (tasks create/execute/finalize, messaging/asks, channels, code/worktrees, hiring/directory, processes/standards, budgets, errors/recovery, batch/session-attach) with zero stale commands or flags.
- [ ] #4 agent-os CLI AX gaps are resolved (scalable launch catalogue within 4 KiB, --help examples rendered, attach.rs error envelope conforms to envelope_schema, actionable retry_with hints, task list total/--count/--exclude-status/--unassigned/--modified-file filters, ask status alignment) and verified by automated tests plus multi-runtime AX evals across claude, codex, and agy.
- [ ] #5 All affected docs, specs, and contracts (doc-27, doc-28, doc-59, doc-62, doc-2, doc-3 FC-168/FC-171, doc-4, AGENTS.md, and .agent-kit/ catalog/evidence) are updated and pass kit/validation checks.
- [ ] #6 Token Efficiency at High Quality is codified as a first-class AX and process primitive: static-to-volatile prompt-cache-friendly ordering, lazy-loaded skills, surgical CLI reads/patches, context-isolated resumable delegation (cheap/fast models for bounded scouting, flagship models for architecture/verification), and measurable token-efficiency metrics (tokens per completed task, cache-read ratio, CLI retry overhead) tracked alongside quality without weakening verification gates.
<!-- AC:END -->
