---
id: TASK-79
title: >-
  Collect and display subscription model usage (total, daily, weekly) in
  Financials UI
status: In Progress
assignee:
  - '@lead'
created_date: '2026-11-22T01:47:00Z'
updated_date: '2026-11-23T05:21:00Z'
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
- [x] #1 Backend supports authenticated collection of subscription model usage (total, daily, and weekly) per provider subscription connection, storing any required user/API credentials securely outside organization Git and failing closed or reporting an explicit auth-needed/unavailable state when credentials are missing, expired, or unsupported by a provider.
- [x] #2 Backend persists and serves subscription model usage snapshots (total, daily, and weekly per model/subscription, with collection freshness/status) via the Financials/Costs API under owner-scoped visibility rules.
- [ ] #3 Financials/Costs UI displays total, daily, and weekly usage by model for active subscriptions, clearly distinguished from API-equivalent cost estimates, with loading, empty, auth-required, stale/incomplete, and error states.
- [ ] #4 Automated backend and UI tests verify authenticated collection, missing/expired auth and provider-unavailable handling, authorization scoping, and daily/weekly/total UI rendering.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint & Handoff (TASK-79):
1. Wire contract (crates/org-proto/src/costs.rs): SubscriptionUsageStatus, SubscriptionUsageAuthSource, SubscriptionUsageWindowView, SubscriptionModelUsageView, SubscriptionUsageSnapshotView, SubscriptionUsageCredentialView, plus 4 owner-only Call RPCs (cost.subscription_usage.{list,collect,credential_set,credential_revoke}) with redacted Debug on secret params.
2. Storage & Office facade (crates/org-office/src/accounting/{subscription_usage.rs,mod.rs,memory.rs}, store/sqlite/accounting.rs, office/accounting.rs): durable-first 0600 JSON under <durable>/local/offices/<office_id>/subscription-usage/<conn>.json + isolated protected/accounting.db rebuild + full-table reconcile_durable sync; owner-only authorization; staleness check; cross-subscription totals_by_model aggregation (is_api_equivalent_estimate: false).
3. Auth & Collector (crates/org-office-sessions/src/{credentials.rs,subscription_usage.rs,manager.rs,lib.rs}): domain-separated usage-credential.v1.json (AAD {connection_id}:usage, KeyClass::ProviderCredential), strict SSRF/non-canonical-IP/DNS-pinned curl transport (2s/3s timeouts, bounded stdout), concurrent collection, parsers for Anthropic OAuth / OpenAI wham / structured models, explicit AuthRequired/Unavailable/Unsupported states.
4. Server & HTTP API (crates/org-office-server/src/{cost_calls.rs,serve/host/operations.json,serve/host/authority.rs}, apps/agentic-org-web/src/api/cost_routes.rs): GET /api/v1/organizations/{org}/costs/subscriptions/usage and POST .../collect, .../credentials, .../credentials/revoke.
5. Remaining UI work (AC #3 & #4 UI): Financials/Costs UI panel in apps/agentic-org-ui displaying total/daily/weekly usage per model/subscription distinct from API-equivalent estimates with loading/empty/auth-required/stale/error states and Playwright tests.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
2026-11-22 Handover checkpoint: Full backend slice implemented and all 10 Pass 1 fresh Neon review findings (4H/3M/3L) fixed in working tree. Resume step: confirm verification suite + Repowise exit 0, run Pass 2 separate fresh final review until VERDICT: APPROVE, check AC #1 & #2, and commit on v1. AC #3 & UI portion of AC #4 remain for ui-dev.

Backend vertical slice complete and approved (gpt-6.2-sol xhigh VERDICT: APPROVE + separate fresh gpt-6.2-sol max VERDICT: APPROVE; Repowise exit 0; cargo clippy --workspace --all-targets -- -D warnings; org-proto/org-office/org-office-sessions/org-office-server/agentic-org-web + test_schema.py 118/118 green). Implemented 4 owner-only RPCs (cost.subscription_usage.{list,collect,credential_set,credential_revoke}), dual-engine Memory+Sqlite AccountingStore with durable 0600 JSON & isolated protected/accounting.db rebuild, restart-safe attempt_seq fencing, office-local domain-separated private usage credential vault (AAD bound to destination origin, 5-level ancestor dir sync), isolated curl HTTPS transport with DNS permit cap + pinned --resolve + paired expiry/batch deadlines, strict duplicate-key-rejecting typed JSON decoders (Anthropic/OpenAI/structured), isolated bounded Web collect channels with cold-open coalescing, and independently pageable list/totals APIs. Remaining work: AC #3 (Financials UI screen) and UI portion of AC #4.
<!-- SECTION:NOTES:END -->
