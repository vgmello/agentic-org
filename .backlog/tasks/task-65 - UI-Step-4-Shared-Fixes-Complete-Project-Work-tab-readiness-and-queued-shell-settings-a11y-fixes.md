---
id: TASK-65
title: >-
  UI Step 4 & Shared Fixes: Complete Project Work tab readiness and queued
  shell/settings/a11y fixes
status: Done
assignee:
  - '@ui-agent'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T05:32:00Z'
labels:
  - ui
  - a11y
  - m1
milestone: m-0
dependencies:
  - TASK-12
  - TASK-17
documentation:
  - docs/workstreams/ui.md
  - docs/plans/v1-ui-screens.md
  - docs/specs/app-shell-ux.md
priority: high
type: enhancement
ordinal: 65000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Apply pending UI decisions and queued shared fixes from docs/workstreams/ui.md: Members and access username visibility (owner/admin/self only), Updates attention-first ordering, Project Activity newest-first, Project Work tab aria-controls + arrow-key nav + 320px/200% tests, retire stale e2e/README-gate.md refs, fix settings/shell layout at 960-1279px with 200% text (re-enabling 9 Offices SET-200 fixme tests), close ConfirmDialog on browser Back, show unsupported as 'not available in this version' without Retry in Resource, integrate pointer focus return (origin/app-frontend-dev/pointer-focus), and live-refusal Delete dialog.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All three pending UI design decisions (username visibility, Updates ordering, Project Activity newest-first) are implemented and tested.
- [x] #2 Settings and shell layouts pass at 960-1279px with 200% text and the 9 Offices SET-200 tests pass without fixme.
- [x] #3 ConfirmDialog closes on Back, Resource renders unsupported without Retry, pointer focus return passes app-shell-ux 305-307, and Work tab keyboard/320px tests pass.
- [x] #4 e2e/README-gate.md is cleaned up and type check, unit tests, lint, build, and Playwright specs pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. AC #1: Enforce username visibility (owner/admin/self) in access module + mock; reorder UpdatesPanel (This instance -> Offices with hold-back note -> How to update); sort Project Activity newest-first in ProjectActivity & ProjectOverview; add unit and e2e tests. 2. AC #2: Scale shell desktop sidebar and settings side-column breakpoints with rem container queries so 960-1279px at 200%/250% text keeps adequate content room; re-enable all 9 Offices SET-200 fixme tests and clear BUG-119 expected failures. 3. AC #3: Close ConfirmDialog on browser Back via modal-back; support office 'unsupported' error state in Resource/Loaded as 'Not available in this version' without Retry; integrate pointer focus return (focus.ts, DropdownMenu, DialogContent, Offices Retry, ProjectTeams/RemoveProjectDialog) + live-refusal DeleteTeamDialog; add aria-controls, roving tabindex, arrow-key nav, and keyboard/320px@200% tests to ProjectWork. 4. AC #4: Clean stale non-chat-icons/project-profile refs in e2e/README-gate.md; run svelte-autofixer on all modified .svelte files; pass format, lint, check, test, build, and Playwright specs.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered all TASK-65 UI Step 4 & shared fixes on feat/task-65-ui-step4-and-shared-fixes: (1) Members and access shows usernames only to owner, administrator and self (access present/page/row + mock); Updates orders This instance -> Offices (with hold-back note at top) -> How to update; Project Activity and Overview sort newest-first. (2) Shell sidebar and Organization settings side column use rem container thresholds (@min-[40rem] and @min-[28rem]) with ResizeObserver-backed Sidebar rail state, giving 960-1279px at 200%/250% text adequate content width; all 9 Offices SET-200 fixmes and BUG-119 expected failures removed and passing. (3) ConfirmDialog closes on browser Back without navigating; Resource/Loaded render office unsupported answers as 'Not available in this version' without Retry; pointer focus return integrated across focus.ts, DropdownMenu, DialogContent, Offices Retry, and ProjectTeams/RemoveProjectDialog; TeamDialogs re-checks blockers quietly on live snapshot changes (re-enabling dialog-title-words live-refusal test); ProjectWork Tasks/Bugs tabs add aria-controls, tabpanel, roving tabindex, arrow/Home/End navigation, and 320px/200% checks. (4) Retired stale non-chat-icons/project-profile refs from e2e/README-gate.md; svelte-autofixer clean on all 20 modified .svelte files; format, lint, check, vitest (144/144, 2430 tests), build, test:e2e:verify, and Chromium/WebKit/production Playwright suites pass.
<!-- SECTION:FINAL_SUMMARY:END -->
