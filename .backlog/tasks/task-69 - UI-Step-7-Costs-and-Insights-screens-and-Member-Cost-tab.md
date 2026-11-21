---
id: TASK-69
title: 'UI Step 7: Costs and Insights screens and Member Cost tab'
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - ui
  - costs
  - late-v1
milestone: m-0
dependencies:
  - TASK-66
documentation:
  - docs/plans/v1-ui-screens.md
  - docs/specs/costs-and-insights-ux.md
priority: medium
type: feature
ordinal: 69000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement Step 7 of v1-ui-screens.md: Project Insights (/projects/[id]/insights with tiles, breakdowns, trend, table, and CSV export), Organization Insights (/insights projects table), and replace the placeholder Member profile Cost tab with live-contract spend and budget views.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Project Insights, Organization Insights, and Member profile Cost tab match costs-and-insights-ux specs.
- [ ] #2 Module mocks live strictly under e2e/fakes/ and production api.ts fails closed until L2/L3 routes land.
- [ ] #3 Type check, unit tests, lint, build, and Playwright specs pass.
<!-- AC:END -->
