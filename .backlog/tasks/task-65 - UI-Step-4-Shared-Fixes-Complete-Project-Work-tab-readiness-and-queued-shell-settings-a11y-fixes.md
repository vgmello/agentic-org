---
id: TASK-65
title: >-
  UI Step 4 & Shared Fixes: Complete Project Work tab readiness and queued
  shell/settings/a11y fixes
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
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
- [ ] #1 All three pending UI design decisions (username visibility, Updates ordering, Project Activity newest-first) are implemented and tested.
- [ ] #2 Settings and shell layouts pass at 960-1279px with 200% text and the 9 Offices SET-200 tests pass without fixme.
- [ ] #3 ConfirmDialog closes on Back, Resource renders unsupported without Retry, pointer focus return passes app-shell-ux 305-307, and Work tab keyboard/320px tests pass.
- [ ] #4 e2e/README-gate.md is cleaned up and type check, unit tests, lint, build, and Playwright specs pass.
<!-- AC:END -->
