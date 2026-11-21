---
id: TASK-14
title: >-
  Complete org-office wire cutover from legacy OrgCommand/OrgSnapshot to
  organization::records
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T02:47:00Z'
labels:
  - rust
  - architecture
  - p2
milestone: m-4
dependencies:
  - TASK-5
references:
  - crates/org-office/src/office/mod.rs
  - crates/org-office/src/organization/wire.rs
  - crates/org-office/src/organization/snapshot.rs
  - crates/org-office/src/organization/directory.rs
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/01-organization-structure.md
priority: medium
type: enhancement
ordinal: 14000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit finding M-03: While org-office/src/organization/features implements 9 operations, office/mod.rs and wire.rs only expose TeamCreate and return Unsupported for 19 OrgCommand variants, and organization/directory.rs still speaks legacy Member/Membership shapes. Completing the cutover eliminates dual internal models inside org-office.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All implemented organization operations in org-office are wired through organization::records without returning Unsupported stubs.
- [x] #2 Legacy Member/Membership/areas shims and legacy OrgSnapshot shapes are deleted from org-office and org-proto.
- [x] #3 Workspace tests and wire test vectors pass against the unified v1 organization contract.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed TASK-14 Part 2: deleted Membership, OrgSnapshot.memberships, OrgSnapshot.areas, TeamAddMember, TeamRemoveMember, and OrgChange::Membership* across org-proto, org-office, agentic-org-office, gateway vectors, and lockstep UI wire readers. Fresh read-only reviewer caught archived-team/inactive-person leak in snapshot::project, silent title_in_team drop in move_person_request, stale top-level team guard, and default_home_team read ordering; all fixed, re-tested, and approved by separate fresh final reviewer (VERDICT: APPROVE).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Completed org-office wire cutover onto organization::records for all 8 implemented operations (CreateTeam including top-level teams, UpdateTeam, MoveTeam, ArchiveTeam, AddPerson, UpdatePerson, MovePerson, DeactivatePerson), excluded archived teams and inactive employees from OrgSnapshot and directory::who, deleted legacy Membership/OrgSnapshot.{memberships,areas}/TeamAddMember/TeamRemoveMember shims across Rust crates and shared wire vectors, and synced lockstep UI readers/tests. Verified via Rust clippy/tests, real-worker launch tests, 2,438 UI Vitest tests, and separate fresh final read-only review (APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
