---
id: TASK-52
title: >-
  S1: Milestone 3 — Second office enrollment, replication, cross-office relay,
  and HQ re-enrollment
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-23T03:22:00Z'
labels:
  - backend
  - rust
  - sync
  - m3
milestone: m-0
dependencies:
  - TASK-39
  - TASK-42
  - TASK-45
  - TASK-49
  - TASK-87
documentation:
  - docs/implementation-plan.md
  - docs/design/offices-hq-and-satellites.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
priority: medium
type: feature
ordinal: 52000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Deliver multi-office v1 milestone 3: satellite office invitation and check-code approval, per-office storage roots, durable replication between HQ and satellites using org-sync (ENG-3 replication suite), cross-office message relay through HQ, one-command satellite re-enrollment after HQ replacement (owner D25), and full fresh-HQ + satellite release acceptance (FC-020, FC-024, FC-145).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A second office enrolls on another machine via invitation and check code and runs agents offline while HQ is unreachable.
- [ ] #2 org-sync durable replication passes the ADR-0018 ENG-3 replication suite and relays messages between offices through HQ.
- [ ] #3 After a lost HQ is imported via R1, satellite offices re-enroll with one command (D25), and FC-020/024/145 are updated.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (S1 / Milestone 3):
1. Enrollment & CA custody: extend crates/org-host and crates/org-office (hq_creation.rs, office_local.rs) + org-pki to issue satellite enrollment invitations, verify check-code approval, provision per-office storage roots, and support one-command satellite re-enrollment after R1 HQ import (D25).
2. Durable replication (ENG-3): wire crates/org-sync (organization.rs, work.rs, messages.rs, ordering.rs, versions.rs) and crates/org-sync-proto into org-office + org-office-server over Organization-CA mTLS, ensuring offline satellite execution continues without HQ (TASK-2 / L4 office_local permits) and converges cleanly on reconnect.
3. Cross-office relay: relay messages, structured asks, and wake triggers between offices via HQ in org-office-server (msg_calls.rs, chat_calls.rs, transfer/).
4. Multi-office provider permits: Office::is_multi_office() (already wired in crates/org-office/src/office/providers.rs) switches L4 PermitAuthority to HqPool automatically once offices_count > 1; verify HQ coordinator lease/cooldown sync across offices.
5. Verification: cargo test -p org-sync -p org-sync-proto -p org-office -p org-office-server -p org-host; ENG-3 replication conformance suite; fresh-HQ + satellite end-to-end test; update doc-3 (FC-020, FC-024, FC-145).
<!-- SECTION:PLAN:END -->
