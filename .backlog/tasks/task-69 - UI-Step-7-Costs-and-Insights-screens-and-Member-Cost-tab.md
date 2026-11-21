---
id: TASK-69
title: 'UI Step 7: Costs and Insights screens and Member Cost tab'
status: Done
assignee:
  - '@ui-engineer'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T18:30:00Z'
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
- [x] #1 Project Insights, Organization Insights, and Member profile Cost tab match costs-and-insights-ux specs.
- [x] #2 Module mocks live strictly under e2e/fakes/ and production api.ts fails closed until L2/L3 routes land.
- [x] #3 Type check, unit tests, lint, build, and Playwright specs pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Extend costs module contracts, formatters (duration, safe RFC 4180 CSV with formula-injection escaping), and fail-closed production api.ts methods for Project Insights, Organization Insights, Member Cost, and Agent Budgets.
2. Extend e2e/fakes/modules/costs.ts (and e2e/fakes/modules/members.ts for manager/viewer cost scenarios) deriving all insights, trends, breakdowns, release caps, and member costs from the single effective ledger JSON source.
3. Build Project Insights (/projects/[id]/insights) and enable Insights tab in ProjectFrame/present.ts, Organization Insights (/insights), and replace the placeholder Member profile Cost tab with live-contract spend and owner-only budget controls.
4. Keep production capabilities.ts and production-unavailable.spec.ts honest and fail-closed until L2/L3 gateway routes land.
5. Verify with svelte-autofixer on all touched .svelte files, bun run check, lint, format:check, test, build, and Playwright chromium + production suites.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implemented Step 7 (Costs & Insights + Member Cost tab) on branch feat/task-69-ui-costs-insights:
- Contracts, pure helpers & tests: src/lib/modules/costs/{types.ts,api.ts,api.test.ts,money.ts,money.test.ts,duration.ts,duration.test.ts,csv.ts,csv.test.ts,insights-helpers.ts,insights-helpers.test.ts,index.ts}. Production costsApi fails closed (unavailableCall) across all new methods. Safe RFC 4180 CSV export neutralizes formula-injection prefixes (=, +, -, @, tab, CR).
- Project Insights (/projects/[id]/insights): ProjectInsightsView, TrendChart (SVG + Chart/Table toggle), InsightsList, ProjectBreakdowns (Team/Consumer/Model/Task type/Work type tabs, filter, aria-sort, UsageRecordsSheet drill-down), release caps, speed medians, CSV export; enabled Insights tab in projects/present.ts.
- Organization Insights (/insights): OrganizationInsightsView with KPI tiles, insights list, spend trend, sortable/filterable Projects table drilling down to /projects/[id]/insights and records sheet, owner-only Not on a task row, CSV export.
- Member Profile Cost tab: replaced NotBuilt placeholder in CostTab.svelte with MemberCostView + AgentBudgetDialog (period spend tiles, owner-only daily/monthly BudgetBars + Set/Change/Clear/Allow 20% once/Keep paused controls, PM 'in your projects' scope without budgets/org overhead, empty/error/unauthorized/person states).
- Fakes strictly under e2e/fakes/modules/{costs,members}.ts. Production capabilities.ts and production-unavailable.spec.ts keep /insights, /projects/[id]/insights, and /members/[id]/cost unavailable until L2/L3 gateway routes land.
- Verified: svelte-autofixer (0 issues on all 11 touched .svelte files), format:check, check (0/0), lint (--max-warnings=0), test (167 files / 2530 tests), build + check:build-assets, Playwright chromium (insights/costs/projects: 36/36), Playwright production (7/7).

Addressed all 10 Pass 1 review findings on feat/task-69-ui-costs-insights: aligned OrganizationInsightsView This month drill-downs, Not on a task filtering/mobile parity, and empty-period coverage/release-caps visibility; added unsupported state, OUTCOME_UNKNOWN Check status reconciliation, full period unknown-cost & coverage gaps to MemberCostView and AgentBudgetDialog; hardened CSV formula-injection prefix with newline; fixed TrendChart null gap breaking, single-point anchor, and bar width clamping; fixed singular item copy, archived badge, Investment h3 level, and budget over-max copy; expanded E2E coverage for PeriodMenu switching and budget save-unknown/save-fails/allow_once/keep_paused/clear flows.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Merged approved TASK-69 (UI Step 7: Project Insights, Organization Insights, Member Cost tab, AgentBudgetDialog, safe CSV export, e2e/fakes/modules/costs.ts, insights.spec.ts, fail-closed production api.ts) into v1. Separate Fresh Final UI Reviewer (24c926a1) returned VERDICT: APPROVE; post-merge svelte-check, vitest (167 files), and bun run build passed on v1.
<!-- SECTION:FINAL_SUMMARY:END -->
