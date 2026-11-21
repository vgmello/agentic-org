---
id: TASK-41
title: >-
  C2: Hire an agent through HR — authorization, proposal confirmation,
  provisioning, and agent lifecycle
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - agents
  - m1
milestone: m-0
dependencies:
  - TASK-34
  - TASK-40
documentation:
  - docs/implementation-plan.md
  - docs/adr/0029-only-who-may-hire-hires.md
  - docs/specs/our-org-in-the-app.md
  - docs/specs/roles-ux.md
priority: high
type: feature
ordinal: 41000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement HR-only agent hiring with ADR-0029 authorization (owner or may_hire lead, no approval workflow), proposal review and explicit confirmation before provisioning employee record, private home, and execution config, agent edit/move/pause/resume/offboard, role record writes (resolving B2 Q12), and role/hr CLI rows (FC-031, FC-033, FC-034, FC-096, FC-097, FC-108).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Only the owner or an authorized may_hire team lead can hire into their subtree; unauthorized callers receive not_authorized on CLI, API, and HR messages.
- [ ] #2 No employee record, private home, or execution binding is created before the hirer confirms the proposal.
- [ ] #3 Agent edit, move, pause, resume, offboard, and role record create/update/list/inspect work via /api/v1 and CLI (CLI-14).
- [ ] #4 STAFF/HRL form-based hire criteria and ADR-0029 HIRE-1 and HIRE-3..6, HIRE-8 pass.
<!-- AC:END -->
