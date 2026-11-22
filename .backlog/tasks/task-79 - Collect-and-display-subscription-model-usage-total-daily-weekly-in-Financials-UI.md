---
id: TASK-79
title: >-
  Collect and display subscription model usage (total, daily, weekly) in
  Financials UI
status: To Do
assignee: []
created_date: '2026-11-22T01:47:00Z'
labels:
  - backend
  - ui
  - costs
  - subscriptions
milestone: m-0
dependencies:
  - TASK-53
  - TASK-54
documentation:
  - >-
    .backlog/docs/organization-data/doc-87 -
    Organization-Data-10-—-Usage-and-Cost-Accounting.md
  - >-
    .backlog/docs/organization-data/doc-89 -
    Organization-Data-14-—-Model-Providers-and-Connection-Pools.md
  - .backlog/docs/specs/doc-51 - Spec-Costs-Budgets-and-Insights-UX.md
  - >-
    .backlog/decisions/decision-32 -
    ADR-0032-Browser-driven-CLI-sign-in-enrollment-credential-distribution-and-permission-presets.md
priority: medium
type: feature
ordinal: 79000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
The v1 usage ledger tracks internal turn-level tokens and API-equivalent USD estimates, which do not show actual provider-side subscription consumption or quota utilization. Owners need to see total, daily, and weekly model usage for active provider subscriptions directly inside the Financials/Costs UI alongside existing spend views. Collecting this data requires the backend to authenticate against provider/user usage APIs (using the appropriate user/subscription auth mechanism while keeping credentials private to office storage and out of organization Git) and handle providers whose usage endpoints are unavailable or require re-authentication.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Backend supports authenticated collection of subscription model usage (total, daily, and weekly) per provider subscription connection, storing any required user/API credentials securely outside organization Git and failing closed or reporting an explicit auth-needed/unavailable state when credentials are missing, expired, or unsupported by a provider.
- [ ] #2 Backend persists and serves subscription model usage snapshots (total, daily, and weekly per model/subscription, with collection freshness/status) via the Financials/Costs API under owner-scoped visibility rules.
- [ ] #3 Financials/Costs UI displays total, daily, and weekly usage by model for active subscriptions, clearly distinguished from API-equivalent cost estimates, with loading, empty, auth-required, stale/incomplete, and error states.
- [ ] #4 Automated backend and UI tests verify authenticated collection, missing/expired auth and provider-unavailable handling, authorization scoping, and daily/weekly/total UI rendering.
<!-- AC:END -->
