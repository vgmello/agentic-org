---
id: TASK-29
title: >-
  Review and migrate Core App, Org, Work & Chat UX specs (docs/specs/, Batch A —
  11 files) to .backlog/docs/
status: Done
assignee:
  - '@backlog-worker'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:20:00Z'
labels:
  - docs
  - ui
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/design/documentation-authority.md
priority: medium
type: docs
ordinal: 29000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Individually review each core UI/UX specification in docs/specs/ against current apps/agentic-org-ui implementation, v1 scope, and data contracts, then migrate them into .backlog/docs/specs/ via `backlog doc create`.

Files in scope (11):
- docs/specs/app-shell-ux.md
- docs/specs/chat-ux.md
- docs/specs/dogfood-run.md
- docs/specs/feature-requests-ux.md
- docs/specs/features-ux.md
- docs/specs/offices-and-teams-ux.md
- docs/specs/our-org-in-the-app.md
- docs/specs/project-status-ux.md
- docs/specs/roles-ux.md
- docs/specs/server-installation-and-access-ux.md
- docs/specs/work-items-ux.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All 11 core UX spec files are individually reviewed for alignment with Organization Data contracts and current v1 UI scope.
- [x] #2 All 11 specifications are migrated into .backlog/docs/specs/ via `backlog doc create` with type specification.
- [x] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Review all 11 core UX specifications in docs/specs/ (Batch A) against apps/agentic-org-ui, Organization Data contracts, ADRs, and current v1/v2 scope.
2. Add concise Review note (2026-11-21) callouts flagging ADR-0028/ADR-0031/D3/D24 contradictions, nav-path drift, and deferred v2 items without silently deleting requirements.
3. Migrate all 11 specs into .backlog/docs/specs/ with type specification via backlog doc create/update.
4. Record mapping and findings in TASK-29 notes, check ACs #1-#3, set final summary, mark Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed review and migration of all 11 docs/specs/ (Batch A) core UX specifications into .backlog/docs/specs/.

Mapping (old path -> Backlog doc ID):
- docs/specs/app-shell-ux.md -> doc-39
- docs/specs/chat-ux.md -> doc-40
- docs/specs/dogfood-run.md -> doc-41
- docs/specs/feature-requests-ux.md -> doc-42
- docs/specs/features-ux.md -> doc-43
- docs/specs/offices-and-teams-ux.md -> doc-44
- docs/specs/our-org-in-the-app.md -> doc-45
- docs/specs/project-status-ux.md -> doc-46
- docs/specs/roles-ux.md -> doc-47
- docs/specs/server-installation-and-access-ux.md -> doc-48
- docs/specs/work-items-ux.md -> doc-49

Key review findings:
- feature-requests-ux.md (Decision 10, §4, §7) and work-items-ux.md (Decision 11, TSK-6) contradict ADR-0028 (always-available agents start on task assignment/DM/mention).
- feature-requests-ux.md RQS-21 contradicts ADR-0031 (still claims exclusive author access including against administrators, whereas §3 and ADR-0031 allow authorized people to read).
- dogfood-run.md (§1.7, §2.5), offices-and-teams-ux.md (§5a/§5b), and server-installation-and-access-ux.md (§4a.6, RCV-*, SRV-25) still list recovery code / CA escrow as v1 despite Owner D24 deferral to v2 (TASK-72).
- our-org-in-the-app.md (§3, §5, PROF-2) cites local CLI takeover paused by Owner D3 (FC-151).
- roles-ux.md §1 cites 'Agents > Roles' vs implemented '/settings/roles' (app-shell-ux.md §0.1), and leaves open B2 Q12 on role creation writes in v1.
- server-installation-and-access-ux.md §2 predates 2026-11-20 service-account/isolation decisions (L15/L16).
- project-status-ux.md preserved explicitly as not-in-v1 (v2 reference).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed all 11 core app, org, work, and chat UX specifications (docs/specs/, Batch A) against Organization Data contracts, ADRs, apps/agentic-org-ui, and current v1/v2 scope, and migrated them into .backlog/docs/specs/ as doc-39 through doc-49 (type specification) with Review note (2026-11-21) callouts documenting ADR-0028, ADR-0031, Owner D3/D24, and nav-path deltas.
<!-- SECTION:FINAL_SUMMARY:END -->
