---
id: TASK-1
title: >-
  POC v5: Multi-theme system (Ocean, Frosted Light, Classic, Modern 2D Report) &
  historical snapshots
status: Done
assignee: []
created_date: '2026-11-22T01:26:00Z'
updated_date: '2026-11-22T01:27:00Z'
labels:
  - ui
  - themes
  - poc
dependencies: []
priority: high
type: feature
ordinal: 1000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Iterative HTML POC in poc/v5.html with 4 isolated themes (Ocean, Executive Frosted Light, Classic, and full-width Modern 2D Executive Report with Report-only section headings) plus archived v5-beta and v5-beta1 routes in poc/server.ts.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All 4 themes render cleanly in poc/v5.html with zero console errors
- [x] #2 Report theme displays full-width pure-white 2D executive layout with section titles (Decision & Approval Queue, Operations & Activity Brief, Project Delivery Scorecard, Capacity & Bottleneck Analysis)
- [x] #3 Historical snapshots /v5-beta and /v5-beta1 are hosted via poc/server.ts and selectable in the version switcher
<!-- AC:END -->
