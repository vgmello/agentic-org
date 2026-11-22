---
id: TASK-44
title: >-
  C5: Organization processes and standards single-chain resolution and agent
  injection
status: In Progress
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T08:42:00Z'
labels:
  - backend
  - rust
  - agents
  - m1
milestone: m-0
dependencies:
  - TASK-42
documentation:
  - docs/implementation-plan.md
  - docs/design/processes.md
  - docs/specs/processes-ux.md
priority: medium
type: feature
ordinal: 44000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Replace multi-chain org-process resolution with the single-chain rule (organization -> accountable team chain -> project), deliver resolved processes and non-overridable standards in agent launch instructions, add process list|inspect and skill list|read CLI rows, enforce already_exists on taken keys, and wire the browser process catalog/editor to live APIs (FC-110, FC-111, FC-112).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Single-chain resolution enforces non-overridable standards and injects them into agent launch instructions.
- [x] #2 process list|inspect and skill list|read CLI rows pass K1 contract tests, and writing a duplicate process key returns already_exists.
- [ ] #3 Organization settings > Processes reads and writes live office records and FC-110..112 are updated.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
C5 backend complete & Neon Pass 2 approved (0 High, 0 Medium). Implemented single-chain org-process resolver/validator/renderer, durable YAML+Markdown structure_documents storage & ProcessFiles projector (SQLite + Memory + MoveTeam relocation + rebuild parity), process.*/skill.* office RPCs, live session launch & WakeKind::TaskAssignment project-chain injection with mandatory budget-overflow refusal, agent-os process/skill CLI rows within K1 4096-byte budget, and /api/v1/organizations/{org}/{processes,skills} HTTP routes. AC #3 remains open for Svelte UI Settings > Processes adapter wiring.
<!-- SECTION:NOTES:END -->
