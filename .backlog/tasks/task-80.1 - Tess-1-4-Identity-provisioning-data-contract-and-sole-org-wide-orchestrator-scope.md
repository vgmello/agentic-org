---
id: TASK-80.1
title: >-
  Tess 1/4: Identity, provisioning, data contract, and sole org-wide
  orchestrator scope
status: To Do
assignee: []
created_date: '2026-11-22T02:35:00Z'
updated_date: '2026-11-23T04:07:00Z'
labels:
  - backend
  - rust
  - agents
  - built-in
  - tess
milestone: m-1
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
Provision Tess as the second built-in organization agent (alongside HR) and establish her data contract, lifecycle invariants, org-wide hiring authority, and sole org-wide orchestrator authority boundary. Tess is the only agent allowed to orchestrate across all teams, projects, agents, and people; all other agents remain scoped to their normal team/reporting/project boundaries. Unlike ordinary agents (which default to may_hire = false and can only hire into subtrees they lead), Tess is provisioned with may_hire = true and can initiate hires into any team in the organization through the HR hire flow (still respecting HR proposal/provisioning and team max_members limits). Runs on demand (never spinning idle) with full office journal audit attribution and strict caller read boundaries (never exposing other agents private notes or host-only secrets).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Organization creation atomically provisions employees.builtin_key = "chief_of_staff" (kind: agent, display name: Tess, role: Chief of Staff) in the top-level team reporting directly to the organization owner, enforced unique per organization alongside hr; existing orgs without Tess can be bootstrapped/migrated safely.
- [ ] #2 Tess has no hire request (Hired by omitted), defaults to may_hire = true with org-wide hiring authority into any team through the HR hire flow (respecting ADR-0029 provisioning and team max_members; owner can toggle may_hire), cannot be offboarded or deleted while the organization exists, and stores the full expansion "Tactical Execution & Squad Supervisor" strictly in her profile bio/charter.
- [ ] #3 Organization Data contracts (doc-75, doc-78), ADR-0029 hiring rules, and schema.sql are updated for builtin_key = "chief_of_staff", granting org-wide hiring when may_hire = true and enforcing Tess as the sole org-wide orchestrator while preventing non-Chief-of-Staff agents from bypassing team/reporting/project scope.
- [ ] #4 Tess executes on demand with turns, reads, and actions recorded in the office journal under her employee ID, strictly respecting caller visibility (no private task notes of other agents, no host-only secrets), backed by automated tests including org-wide hiring via HR.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (Tess 1/4):
1. Data contract & schema: update doc-75, doc-78, ADR-0029, and docs/design/organization-data/schema.sql to add employees.builtin_key = 'chief_of_staff' (unique per org alongside 'hr'); verify python3 -B -m unittest docs/design/organization-data/test_schema.py.
2. Provisioning & lifecycle (crates/org-office/src/organization/records.rs, hq_creation.rs, organization/features/agent_lifecycle.rs, office/hiring.rs):
   - Add BuiltinKey::ChiefOfStaff alongside BuiltinKey::Hr.
   - Atomically provision Tess (name='Tess', role='Chief of Staff', may_hire=true, bio='Tactical Execution & Squad Supervisor', reports_to=owner, top-level team, no hire request) at org creation + safe idempotent bootstrap/migration for existing orgs.
   - Prevent offboarding/deletion while org exists; allow owner to toggle may_hire.
3. Authority boundaries: grant org-wide hiring into any team via HR hire flow when may_hire=true (still enforcing ADR-0029 proposal/provisioning & team max_members); enforce Tess as sole org-wide orchestrator while non-Chief-of-Staff agents stay bounded to team/reporting/project scope; enforce private task notes (ADR-0031) & host-secret isolation.
4. Verification: dual-engine tests in crates/org-office (org creation, migration, offboard protection, org-wide HR hiring, non-Tess scope denial) + test_schema.py.
<!-- SECTION:PLAN:END -->
