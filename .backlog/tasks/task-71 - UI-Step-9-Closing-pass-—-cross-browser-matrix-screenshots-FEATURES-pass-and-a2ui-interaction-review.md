---
id: TASK-71
title: >-
  UI Step 9: Closing pass — cross-browser matrix, screenshots, FEATURES pass,
  and a2ui interaction review
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - ui
  - ux
  - qa
  - late-v1
milestone: m-0
dependencies:
  - TASK-68
  - TASK-69
  - TASK-70
  - TASK-67
documentation:
  - docs/plans/v1-ui-screens.md
  - docs/workstreams/ui.md
  - docs/FEATURES.md
priority: medium
type: chore
ordinal: 71000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Complete Step 9 of v1-ui-screens.md: run full Chromium/WebKit/Firefox test suite, capture wide and narrow plus 200% text screenshots of every screen, restore the four allowedCoupling entries in e2e/gate-manifest.json, conduct the principal-designer walkthrough plus an interaction review using https://a2ui.org/ as reference, and reconcile all UI rows in FEATURES.md.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Full Playwright suite passes across Chromium, WebKit, and Firefox with zero unreviewed skips and gate-manifest.json restored.
- [ ] #2 Wide, narrow, and 200% text screenshots are captured and principal-designer + a2ui interaction review findings are resolved.
- [ ] #3 All UI rows in docs/FEATURES.md accurately reflect verified state.
<!-- AC:END -->
