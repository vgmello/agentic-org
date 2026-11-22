---
id: TASK-80
title: >-
  Built-in Chief of Staff agent (Tess): org-wide orchestrator, Executive Brief,
  Suggestions Queue, and Ask Tess
status: To Do
assignee: []
created_date: '2026-11-22T02:08:00Z'
updated_date: '2026-11-22T14:40:00Z'
labels:
  - backend
  - ui
  - agents
  - built-in
  - chief-of-staff
  - tess
  - home
  - inbox
  - blog
milestone: m-0
dependencies:
  - TASK-42
  - TASK-45
  - TASK-49
documentation:
  - >-
    .backlog/docs/organization-data/doc-78 -
    Organization-Data-04-—-Agent-Structure.md
  - >-
    .backlog/docs/organization-data/doc-75 -
    Organization-Data-01-—-Organization-Structure.md
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/design/doc-36 - Design-Smart-Features-Not-in-v1.md
priority: medium
type: feature
ordinal: 80000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Introduce Tess, a built-in AI Chief of Staff agent provisioned automatically with every organization alongside the built-in HR lead. Tess turns Agentic Org into an AI-native executive workspace and is the sole agent in the organization permitted to orchestrate across everything and everyone (all teams, projects, agents, people, and system workflows), while non-Chief-of-Staff agents remain bounded to their team, reporting, and project scopes. Tess is provisioned with may_hire = true so she can hire agents into any team in the organization through the HR hire flow (respecting ADR-0029 provisioning and team max_members limits). Tess synthesizes live operations into a read-only Executive Brief on Home, answers org-wide and item-scoped questions via Ask Tess (⌘K and card-level Ask about this), and submits concrete proposals (suggestion items) into the owner Decision & Approval Queue without ever mutating state silently. Profile bio/charter carries the full expansion "Tactical Execution & Squad Supervisor" (profile only — never repeated inline in banners, cards, or signatures).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Creating a new organization provisions employees.builtin_key = "chief_of_staff" (Tess) in the top-level team reporting to the owner alongside the built-in HR lead, enforced unique per organization, with may_hire = true (authorized to hire into any team via the HR hire flow), no hire request, non-offboardable while org exists, and sole org-wide orchestrator enforcement.
- [ ] #2 Tess profile displays "Tactical Execution & Squad Supervisor"; inline banners, cards, and the Home Executive Brief signature show only "Tess" / "Chief of Staff" and link to her profile.
- [ ] #3 Home displays Tess Executive Brief (Today / 7-Day / Board lenses, with 1-click Copy on Board) incorporating live highlights and operational risks with zero action chips/buttons on the right.
- [ ] #4 Tess proposed solutions appear as actionable suggestion cards in the left-hand Decision & Approval Queue and Inbox with rationale and structured risk/impact/verification metadata; applying executes only after explicit owner confirmation and logs attributed audit history.
- [ ] #5 Global command bar (⌘K) shows placeholder "Search, run a command, or ask Tess…" with ✨ Ask opening Ask Tess, and clicking ✨ Ask about this on any active decision card opens Ask Tess pre-scoped to that item while enforcing caller read/private-note/secret boundaries.
<!-- AC:END -->
