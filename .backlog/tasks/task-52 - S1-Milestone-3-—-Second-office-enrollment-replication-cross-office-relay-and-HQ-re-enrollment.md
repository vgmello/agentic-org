---
id: TASK-52
title: >-
  S1: Milestone 3 — Second office enrollment, replication, cross-office relay,
  and HQ re-enrollment
status: Done
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-24T09:48:00Z'
labels:
  - backend
  - rust
  - sync
  - m3
milestone: m-0
dependencies:
  - TASK-45
  - TASK-49
  - TASK-42
  - TASK-39
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
- [x] #1 A second office enrolls on another machine via invitation and check code and runs agents offline while HQ is unreachable.
- [x] #2 org-sync durable replication passes the ADR-0018 ENG-3 replication suite and relays messages between offices through HQ.
- [x] #3 After a lost HQ is imported via R1, satellite offices re-enroll with one command (D25), and FC-020/024/145 are updated.
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

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed S1 / Milestone 3 backend, host, web API, CLI, and federation end-to-end: Org-CA mTLS satellite invitation/check-code enrollment (3-wrong-code cancel), per-office storage roots, native replication store (ENG-3 Memory+SQLite conformance + runtime delete rebuild), bounded streaming/paged snapshot export & disk-staged dependency-ordered import with heartbeats/cancellation/process-wide budget, HQ ingest & offline work convergence, durable cross-office message/ask/wake relay, per-peer revocation draining/fencing, and D25 one-command satellite re-enrollment. Verified with cargo fmt/clippy, Repowise exit 0, federation_e2e, satellite_tests, replication ENG-3, snapshot_transfer, host office_join, org-office-sessions, and read-only tiered Sol max review (VERDICT: APPROVE). Updated doc-3 (FC-019, FC-020, FC-024, FC-145, FC-165, FC-167).
<!-- SECTION:NOTES:END -->
