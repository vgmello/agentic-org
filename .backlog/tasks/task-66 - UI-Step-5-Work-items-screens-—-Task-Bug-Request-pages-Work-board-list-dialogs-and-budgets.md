---
id: TASK-66
title: >-
  UI Step 5: Work items screens — Task, Bug, Request pages, Work board/list,
  dialogs, and budgets
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
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
- [ ] #1 Task, Bug, and Request full pages at /work/[ref] and /work board/list/filters/Needs me match work-items-ux and feature-requests-ux specs using the current data contract.
- [ ] #2 Project /projects/[id]/work uses the new Work board/list, replacing legacy Tasks/Bugs tabs, and top-bar New menu includes Project and Work item.
- [ ] #3 Budget set/raise/reached dialogs and chips work against module mocks under e2e/fakes/ with zero mocks in src/.
- [ ] #4 Type check, unit tests, lint, build, principal-designer walkthrough, and Playwright specs pass.
<!-- AC:END -->
