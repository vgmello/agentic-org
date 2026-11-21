---
id: TASK-14
title: >-
  Complete org-office wire cutover from legacy OrgCommand/OrgSnapshot to
  organization::records
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
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
- [ ] #1 All implemented organization operations in org-office are wired through organization::records without returning Unsupported stubs.
- [ ] #2 Legacy Member/Membership/areas shims and legacy OrgSnapshot shapes are deleted from org-office and org-proto.
- [ ] #3 Workspace tests and wire test vectors pass against the unified v1 organization contract.
<!-- AC:END -->
