---
id: TASK-85.1
title: >-
  AX 1/3: Create Principal AX Designer persona/skill (Claude + Codex) and
  establish the Agent Experience (AX) Spec
status: To Do
assignee: []
created_date: '2026-11-22T14:37:00Z'
labels:
  - ax
  - agents
  - docs
  - skills
  - kit
milestone: m-0
dependencies: []
documentation:
  - .claude/agents/principal-designer.md
  - .codex/agents/principal-designer.toml
  - .agent-kit/project-skills/principal-designer/SKILL.md
  - .backlog/docs/design/doc-62 - Design-Documentation-Authority-Map.md
  - .backlog/docs/doc-2 - Start-Here.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - .backlog/docs/doc-4 - Feature-Areas.md
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
parent_task_id: TASK-85
priority: high
type: feature
ordinal: 89000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Establish Agent Experience (AX) as a first-class design discipline parallel to UX so every future agent-os command, onboarding flow, error message, and skill is designed and reviewed with the same rigor as browser screens. Mirror the proven principal-designer architecture across Claude Code, Codex, and .agent-kit/project-skills/, and codify the authoritative AX Spec so product contracts outrank persona preferences.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Creates .claude/agents/principal-ax-designer.md and .codex/agents/principal-ax-designer.toml mirroring principal-designer (Design, Implement, Review modes, read order, spec precedence, limits, handback contract) scoped specifically to agent-os CLI, instructions assembly, agent onboarding, skills, and AX reviews.
- [ ] #2 Creates .agent-kit/project-skills/principal-ax-designer/ with SKILL.md and focused reference docs covering AX laws & principles: context-window frugality / progressive disclosure, zero-guesswork CLI grammar & self-discovery, actionable self-healing errors & felt safety for agents, Day-1 agent onboarding, runtime portability (claude/codex/agy), and an AX review audit rubric.
- [ ] #3 Authors an authoritative Agent Experience (AX) Spec in .backlog/docs/specs/ (parallel to doc-39 App Shell UX) defining AX standards, token budgets, discovery tiers, onboarding flow, and measurable AX quality gates (CLI error/retry rate, cold-start orientation, token overhead).
- [ ] #4 Updates doc-62 (Documentation Authority Map), doc-2 (Start Here), doc-4 (Feature Areas), doc-3 (Feature Checklist), AGENTS.md, and .agent-kit/ metadata/evidence hashes so kit validation checks pass.
<!-- AC:END -->
