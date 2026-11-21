---
id: TASK-66
title: >-
  UI Step 5: Work items screens — Task, Bug, Request pages, Work board/list,
  dialogs, and budgets
status: In Progress
assignee:
  - UI Engineer (TASK-66)
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T07:37:00Z'
labels:
  - ui
  - work
  - m1
milestone: m-0
dependencies:
  - TASK-65
documentation:
  - docs/plans/v1-ui-screens.md
  - docs/workstreams/ui.md
  - docs/specs/work-items-ux.md
  - docs/specs/feature-requests-ux.md
priority: high
type: feature
ordinal: 66000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Build Step 5 of v1-ui-screens.md (reusing or replacing WIP from branch v1-ui-work): full /work/[ref] pages for Task, Bug, and Request; /work board and list with saved filters, Show menu, and Needs me; updated New task, Report bug, and New request dialogs (also exposed in top-bar New menu); budget set/raise/reached dialogs and chips; and replace Step 4's legacy Tasks/Bugs Work tab on /projects/[id]/work.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Task, Bug, and Request full pages at /work/[ref] and /work board/list/filters/Needs me match work-items-ux and feature-requests-ux specs using the current data contract.
- [x] #2 Project /projects/[id]/work uses the new Work board/list, replacing legacy Tasks/Bugs tabs, and top-bar New menu includes Project and Work item.
- [x] #3 Budget set/raise/reached dialogs and chips work against module mocks under e2e/fakes/ with zero mocks in src/.
- [ ] #4 Type check, unit tests, lint, build, principal-designer walkthrough, and Playwright specs pass.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implemented UI Step 5 Work items in src/lib/modules/workitems/** with module fakes under e2e/fakes/{modules/workitems.ts,data/workitems/tasks.json} (zero static mocks in src/). Rebuilt /work, /work/[ref], /projects/[id]/work, and top-bar New menu (Project + Work item + Request). Verified: format:check (817 checked, 0 failures), lint (0 errors/warnings), check (0 errors/warnings), vitest (151 files, 2452 tests passed), build + productionImportGuard (0 leaks), test:e2e:verify, Playwright chromium/production E2E (work.spec.ts + projects.spec.ts: 25 passed; shell-topbar.spec.ts + production-unavailable.spec.ts: 26 passed). Ready for lead fresh read-only review and merge.
<!-- SECTION:NOTES:END -->
