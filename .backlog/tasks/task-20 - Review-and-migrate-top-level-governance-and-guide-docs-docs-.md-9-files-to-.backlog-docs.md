---
id: TASK-20
title: >-
  Review and migrate top-level governance and guide docs (docs/*.md, 9 files) to
  .backlog/docs/
status: Done
assignee:
  - '@backlog-worker'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:14:00Z'
labels:
  - docs
  - backlog-migration
milestone: m-5
dependencies: []
documentation:
  - docs/START-HERE.md
  - docs/FEATURES.md
  - docs/architecture-rules.md
  - docs/architecture-non-negotiables.md
priority: medium
type: docs
ordinal: 20000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Every existing documentation file in the repository needs an individual accuracy review against the current v1 codebase and migration into .backlog/docs/ via `backlog doc create` so Backlog.md becomes the single curated home for repository documentation.

Files in scope (9):
- docs/START-HERE.md
- docs/FEATURES.md
- docs/FEATURE-AREAS.md
- docs/implementation-plan.md
- docs/independent-delivery.md
- docs/architecture-non-negotiables.md
- docs/architecture-rules.md
- docs/rust-style-guide.md
- docs/rust-style-guide.LICENSE
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Each of the 9 top-level files in docs/ is individually reviewed against current v1 implementation reality, with stale claims or broken references flagged or corrected.
- [x] #2 Every Markdown doc in this batch is migrated into .backlog/docs/ using `backlog doc create` with appropriate document type and relative path (and license attribution preserved for rust-style-guide).
- [x] #3 A migration mapping records old path -> new .backlog doc ID/path and any review findings requiring follow-up.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Review all 9 top-level files in docs/ against current v1 reality and Vision (doc-1). 2. Migrate 8 Markdown docs into .backlog/docs/ via backlog doc create/update, embedding rust-style-guide.LICENSE inside the migrated Rust Style Guide doc and adding concise review notes where claims are stale. 3. Record old-path -> doc-ID mapping and findings in notes, verify ACs, finalize, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Migration mapping (old path -> Backlog doc ID):
- `docs/START-HERE.md` -> `doc-2` (Start Here)
- `docs/FEATURES.md` -> `doc-3` (Feature Checklist)
- `docs/FEATURE-AREAS.md` -> `doc-4` (Feature Areas)
- `docs/implementation-plan.md` -> `doc-5` (v1 Implementation Plan)
- `docs/independent-delivery.md` -> `doc-6` (Independent Delivery and Dependency Boundaries)
- `docs/architecture-non-negotiables.md` -> `doc-7` (Architecture Non-Negotiables and Who Decides What)
- `docs/architecture-rules.md` -> `doc-8` (Architecture Rules)
- `docs/rust-style-guide.md + docs/rust-style-guide.LICENSE` -> `doc-9` (Rust Style Guide)

Review findings:
- `docs/START-HERE.md`: table says 'seventeen fixed rules' but `architecture-non-negotiables.md` has 18 rules.
- `docs/FEATURES.md`: stale evidence in `FC-028` (deleted `memberships` table), `FC-162` (deleted `app_token`), `FC-017` (X5/X7 status), and `FC-164` (X2-X7 progress).
- `docs/FEATURE-AREAS.md`: AREA-01/AREA-02 verification focus lags mock Search/New (`FC-006`/`FC-007`) and A3 B6 registration (`FC-014`).
- `docs/implementation-plan.md`: several 'Code today' blocks predate A3 B4-B6, B2.1, C1 PR 17, X2, X5, and X7 merges.
- `docs/architecture-non-negotiables.md`: noted open audit tension on Doc 11 HQ permits (`TASK-2`) and D28 Xcode-extension exception (`FC-194`).
- `docs/rust-style-guide.LICENSE`: embedded verbatim in `Rust Style Guide` (`doc-9`) to preserve CC BY 3.0 attribution.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed all 9 top-level files in docs/ against v1 reality and Vision (doc-1), flagged stale status/evidence claims in concise review notes, and migrated them into .backlog/docs/ (doc-2..doc-9) with rust-style-guide.LICENSE preserved verbatim inside doc-9.
<!-- SECTION:FINAL_SUMMARY:END -->
