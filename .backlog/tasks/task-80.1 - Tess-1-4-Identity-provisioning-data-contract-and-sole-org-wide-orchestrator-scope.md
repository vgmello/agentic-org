---
id: TASK-80.1
title: >-
  Tess 1/5 (v1): Identity, provisioning, OperatorSandboxed preset
  (Codex/Claude/Agy), per-caller session isolation & caller∩tess security
status: To Do
assignee: []
created_date: '2026-11-22T02:35:00Z'
updated_date: '2026-11-25T00:15:00Z'
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
  - TASK-83
  - TASK-85.2
  - TASK-85.3
documentation:
  - >-
    .backlog/docs/organization-data/doc-75 -
    Organization-Data-01-—-Organization-Structure.md
  - >-
    .backlog/docs/organization-data/doc-78 -
    Organization-Data-04-—-Agent-Structure.md
  - >-
    .backlog/docs/design/doc-97 -
    Design-Embedded-Tess-—-Chief-of-Staff-and-Product-Assistant.md
parent_task_id: TASK-80
priority: high
type: feature
ordinal: 81000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Provisions Tess as a first-class built-in agent (employees.builtin_key='chief_of_staff', unique per org alongside 'hr', non-offboardable while org exists, top-level team reporting to owner, may_hire=true) running on org-office-sessions + org-agent-runtime-{claude,codex,agy} with warm persistent sessions (Control::Send) — NOT org-copilot and NOT a separate server. Solves the Codex ReadOnly shell_tool=false lockout via an OperatorSandboxed preset (empty repo-less 0700 private home, zero repo mounts, office paths denied, allowed_unix_sockets=[AGENT_OS_SOCKET], narrow agent-os shell allowlist across Claude/Codex/Agy), implements server-side effective permission intersection (caller_permissions ∩ tess_capabilities via in-memory acting_for_caller_id on RunBindingRegistry), and partitions interactive Ask Tess sessions per caller (tess:ask:<caller_id>) so cross-caller LLM context can never leak.
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

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
2026-11-24 4-Way Council Alignment (PM + Systems Architect + Principal UX + Principal AX):
- Schema/Records: expand employees.builtin_key CHECK in docs/design/organization-data/schema.sql to allow 'chief_of_staff' + UNIQUE INDEX one_chief_of_staff_per_org; add BuiltinKey::ChiefOfStaff in crates/org-office/src/organization/records.rs; update doc-75, doc-78, ADR-0029, test_schema.py.
- Codex Lockout Fix (crates/org-agent-runtime-codex/src/plan.rs:57-75): add OperatorSandboxed / ChiefOfStaff preset with empty repo-less 0700 home, zero repo writable roots, denied_office_globs intact, allowed_unix_sockets=[AGENT_OS_SOCKET], and narrow agent-os shell execpolicy so Tess executes agent-os natively on Claude, Codex, and Agy with empty --mcp-config (CLI-12).
- Dual-Intersection Auth & Hard Invariants: bind acting_for_caller_id in-memory on RunBindingRegistry (never CLI flags); re-evaluate live caller permissions on every RPC (COST-7/COST-9 PM spend scoping, owner-only inbox); keep ADR-0031 private notes of other agents, non-member 1:1 DMs, CredentialVault/OAuth/PKI, and self-escalation unconditionally denied even when caller is Owner; partition warm sessions as tess:ask:<caller_id> + rotate at 60% context window or 20 turns.
- Standing Prompt (AX-11): add <=2 KiB CHIEF_OF_STAFF_GUIDANCE in Pos 5 of instructions.rs (parallel to HR_LEAD_GUIDANCE) with executive voice, safety rules, and ~350 B Product-Guide Topic Index while keeping Tiers 1-2 byte-identical across all agents for >=80% KV cache hit rate.
<!-- SECTION:NOTES:END -->
