---
id: TASK-80.1
title: >-
  Tess 1/4: Identity, provisioning, data contract, and sole org-wide
  orchestrator scope
status: To Do
assignee: []
created_date: '2026-11-22T02:35:00Z'
labels:
  - backend
  - rust
  - agents
  - built-in
  - tess
milestone: m-0
dependencies:
  - TASK-42
  - TASK-45
  - TASK-49
documentation:
  - >-
    .backlog/docs/organization-data/doc-75 -
    Organization-Data-01-—-Organization-Structure.md
  - >-
    .backlog/docs/organization-data/doc-78 -
    Organization-Data-04-—-Agent-Structure.md
parent_task_id: TASK-80
priority: medium
type: feature
ordinal: 81000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Provision Tess as the second built-in organization agent (alongside HR) and establish her data contract, lifecycle invariants, and sole org-wide orchestrator authority boundary. Tess is the only agent allowed to orchestrate across all teams, projects, agents, and people; all other agents remain scoped to their normal team/reporting/project boundaries. Runs on demand (never spinning idle) with full office journal audit attribution and strict caller read boundaries (never exposing other agents private notes or host-only secrets).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Organization creation atomically provisions employees.builtin_key = "chief_of_staff" (kind: agent, display name: Tess, role: Chief of Staff) in the top-level team reporting directly to the organization owner, enforced unique per organization alongside hr; existing orgs without Tess can be bootstrapped/migrated safely.
- [ ] #2 Tess has no hire request (Hired by omitted), defaults to may_hire = false, cannot be offboarded or deleted while the organization exists, and stores the full expansion "Tactical Execution & Squad Supervisor" strictly in her profile bio/charter.
- [ ] #3 Organization Data contracts (doc-75, doc-78) and schema.sql are updated for builtin_key = "chief_of_staff", enforcing Tess as the sole org-wide orchestrator while preventing non-Chief-of-Staff agents from bypassing team/reporting/project scope.
- [ ] #4 Tess executes on demand with turns, reads, and actions recorded in the office journal under her employee ID, strictly respecting caller visibility (no private task notes of other agents, no host-only secrets), backed by automated tests.
<!-- AC:END -->
