---
id: TASK-85.1
title: >-
  AX 1/3: Create Principal AX Designer persona/skill (Claude + Codex) and
  establish the Agent Experience (AX) Spec
status: Done
assignee: []
created_date: '2026-11-22T14:37:00Z'
updated_date: '2026-11-23T03:57:00Z'
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
- [x] #1 Creates .claude/agents/principal-ax-designer.md and .codex/agents/principal-ax-designer.toml mirroring principal-designer (Design, Implement, Review modes, read order, spec precedence, limits, handback contract) scoped specifically to agent-os CLI, instructions assembly, agent onboarding, skills, and AX reviews.
- [x] #2 Creates .agent-kit/project-skills/principal-ax-designer/ with SKILL.md and focused reference docs covering AX laws & principles: context-window frugality / progressive disclosure, zero-guesswork CLI grammar & self-discovery, actionable self-healing errors & felt safety for agents, Day-1 agent onboarding, runtime portability (claude/codex/agy), and an AX review audit rubric.
- [x] #3 Authors an authoritative Agent Experience (AX) Spec in .backlog/docs/specs/ (parallel to doc-39 App Shell UX) defining AX standards, token budgets, discovery tiers, onboarding flow, and measurable AX quality gates (CLI error/retry rate, cold-start orientation, token overhead).
- [x] #4 Updates doc-62 (Documentation Authority Map), doc-2 (Start Here), doc-4 (Feature Areas), doc-3 (Feature Checklist), AGENTS.md, and .agent-kit/ metadata/evidence hashes so kit validation checks pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Create authoritative Agent Experience (AX) Spec in .backlog/docs/specs/ via backlog CLI defining AX discovery tiers, token budgets, CLI grammar/pruning/batching, Day-1 onboarding, phase-gated skills, errors/felt safety, runtime portability, and AX-1..AX-24 quality gates.
2. Author .agent-kit/project-skills/principal-ax-designer/SKILL.md and 4 deep reference files (cli-grammar-and-discovery.md, onboarding-and-skills.md, errors-and-felt-safety.md, evals-and-portability.md) grounded in org-command, org-agent-cli, org-office-sessions, org-process, and org-proto.
3. Create Claude Code (.claude/agents/principal-ax-designer.md) and Codex (.codex/agents/principal-ax-designer.toml) personas mirroring principal-designer.
4. Update doc-62, doc-2, doc-4, doc-3 (FC-168/FC-171), AGENTS.md, and .agent-kit/skill-catalog.json evidence/paths so kit_skills.py validate, kit_hooks.py check, and backlog doctor pass 100%.
5. Finalize TASK-85.1 (check ACs 1-4, final summary, Done) and commit cleanly on feat/task-85-1-principal-ax-designer with passing Repowise pre-commit hook.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Established Agent Experience (AX) as a first-class product vertical parallel to browser UX:
- Created Claude Code (.claude/agents/principal-ax-designer.md) and Codex (.codex/agents/principal-ax-designer.toml) personas mirroring principal-designer across Design, Implement, and Review modes.
- Authored .agent-kit/project-skills/principal-ax-designer/SKILL.md and 4 technical reference guides (cli-grammar-and-discovery.md, onboarding-and-skills.md, errors-and-felt-safety.md, evals-and-portability.md) grounded in org-command, org-agent-cli, org-office-sessions, org-process, and org-proto.
- Authored authoritative AX Spec doc-96 (.backlog/docs/specs/doc-96 - Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md) defining discovery tiers, token budgets, onboarding, phase-gated skills, felt safety/conflict deltas, portability, and AX-1..AX-24 quality gates.
- Registered AX across doc-62, doc-2, doc-4, doc-3 (FC-168/FC-171), AGENTS.md, and .agent-kit/skill-catalog.json (adding agent-experience skill, migrating stale docs/ evidence paths, and refreshing hashes).
- Verified green: kit_skills.py validate, kit_hooks.py check, backlog doctor, TOML syntax, markdown links, and .agents/check-tools.sh.
<!-- SECTION:FINAL_SUMMARY:END -->
