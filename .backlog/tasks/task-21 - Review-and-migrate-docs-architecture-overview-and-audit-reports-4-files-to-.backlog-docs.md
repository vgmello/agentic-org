---
id: TASK-21
title: >-
  Review and migrate docs/architecture/ overview and audit reports (4 files) to
  .backlog/docs/
status: Done
assignee:
  - '@backlog-worker'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:15:00Z'
labels:
  - docs
  - architecture
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/architecture/README.md
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 21000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Review and migrate the architecture overview and audit documents under docs/architecture/ into .backlog/docs/ via `backlog doc create` so architectural baselines and audit reports live inside .backlog/.

Files in scope (4):
- docs/architecture/README.md
- docs/architecture/architecture-overview.html
- docs/architecture/system-architecture.html
- docs/architecture/architecture-and-data-audit.html
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All 4 files in docs/architecture/ are individually reviewed for current accuracy and overlap.
- [x] #2 Markdown summaries/canonical docs are created in .backlog/docs/architecture/ via `backlog doc create`, and an explicit disposition is recorded for the 3 interactive HTML reports (Markdown conversion vs static asset retention linked from Backlog docs).
- [x] #3 Cross-references from tasks and architecture docs resolve cleanly.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Review all 4 files under docs/architecture/ (README.md and 3 interactive HTML reports) for accuracy, overlap, and disposition. 2. Create 4 canonical Markdown guide/summary docs in .backlog/docs/architecture/ via backlog doc create/update, retaining the 3 self-contained HTML reports as static visual assets until TASK-32. 3. Record mapping, disposition, and review findings in notes, verify ACs, mark Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Migration mapping (old path -> Backlog doc ID):
- `docs/architecture/README.md` -> `doc-10` (Architecture Pages Index)
- `docs/architecture/architecture-overview.html` -> `doc-11` (Architecture Overview (HTML Guide); static HTML retained until TASK-32)
- `docs/architecture/system-architecture.html` -> `doc-12` (System Architecture (HTML Guide); static HTML retained until TASK-32)
- `docs/architecture/architecture-and-data-audit.html` -> `doc-13` (Architecture and Organization Data Structure Audit (2026-11-20); static HTML retained until TASK-32)

Disposition & Review Findings:
- All 3 interactive HTML reports (125–161 KB each) are retained as static visual assets at `docs/architecture/*.html` until `TASK-32` asset cutover, with canonical Markdown guides/summaries in `.backlog/docs/architecture/`.
- `docs/architecture/README.md` omitted `architecture-and-data-audit.html` and still referenced retired `v1-integration`.
- `architecture-overview.html` and `system-architecture.html` are 2026-11-18/19 snapshots (data rev 2.19, ADRs 0001–0031) predating ADR-0032, D1–D28 (2026-11-20), B1b, B2.1, C1 PR 17, X5, and X7 core TLS; staleness noted explicitly.
- `architecture-and-data-audit.html` (2026-11-20, data rev 2.23) is current and cross-referenced directly to `TASK-1`–`TASK-19`.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed all 4 files under docs/architecture/, created canonical Markdown guides in .backlog/docs/architecture/ (doc-10..doc-13), recorded explicit static-asset retention for the 3 interactive HTML reports until TASK-32, and mapped audit remediation to TASK-1..TASK-19.
<!-- SECTION:FINAL_SUMMARY:END -->
