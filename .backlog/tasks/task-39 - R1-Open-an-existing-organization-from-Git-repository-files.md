---
id: TASK-39
title: 'R1: Open an existing organization from Git repository files'
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T14:26:00Z'
labels:
  - backend
  - rust
  - recovery
  - m1
milestone: m-0
dependencies:
  - TASK-38
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
  - docs/design/accounts-sandbox-and-extensions.md
priority: high
type: feature
ordinal: 39000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement 'Open an existing organization' beside Create as v1's lost-HQ recovery path (owner D25): validate durable files against the data contract, register at the host under the existing UUIDv7 org ID with a new HQ CA, rebuild the office projection, bind the administrator to their person record as owner, and clearly state local-only loss boundaries (FC-167).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Deleting runtime/ and the host database and opening the organization from its Git repo restores all published teams, people, projects, and records.
- [x] #2 Invalid organization files refuse the open and name the offending file.
- [x] #3 Administrator selects their person record to become owner and sees an explicit summary of local-only data that did not come back.
- [x] #4 FEATURES row FC-167 is updated with real-process verification evidence.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Implemented R1 (Open an existing organization from Git repository files, FC-167): stateless read-only Git validation via agentic-org-office --inspect-import (bare clone + ls-tree + cat-file --batch rejecting symlinks/submodules/oversize/excess files without working-tree checkout), full projector validation naming offending files, host control (organization.open_preview, organization.open) and browser HTTP routes (POST /api/v1/organizations/open/preview, POST /api/v1/organizations/open), fresh HQ office ID/code/CA/custody under existing org UUIDv7, existing-person or new_owner_person owner binding, explicit local_only_not_restored loss summary, crash-safe post-Entry-1 resume, and two-pass fresh independent Rust review (Pass 2 VERDICT: APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
