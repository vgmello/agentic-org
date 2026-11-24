---
id: TASK-37
title: 'O1: Offices in the product — live office list, readiness, and host restart'
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T06:46:00Z'
labels:
  - backend
  - rust
  - bff
  - m1
milestone: m-0
dependencies:
  - TASK-5
documentation:
  - docs/implementation-plan.md
  - docs/design/offices-hq-and-satellites.md
  - docs/specs/offices-and-teams-ux.md
priority: high
type: feature
ordinal: 37000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Expose the live HQ office list, readiness/freshness state reported by host and office, and authorized office restart through /api/v1 and CLI for single-office v1 milestone 1 (FC-019, FC-021).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Office list and readiness endpoints return live HQ office state via /api/v1 and CLI.
- [x] #2 Administrator or organization owner can trigger an office restart through /api/v1 and verify it returns ready.
- [x] #3 Single-office OFC acceptance criteria pass against a real instance and FC-019/FC-021 are updated.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add BrowserOfficeRestart (browser.office_restart) to org-host-control + org-host Service (admitted web connection only): validate BrowserAuth, verify account is linked to org_id & office_id belongs to org_id (not_found otherwise), require admin or org owner (not_authorized -> 403), journal by operation_id (replay / operation_reused 409), call offices.restart_office, return HostOpResult<Empty>.
2. Wire GET /api/v1/organizations/{org}/offices (returning OfficeList { offices: Vec<OfficeView> }, stripping endpoint) and POST /api/v1/organizations/{org}/offices/{office}/restart (taking OfficeRestartRequest = CommandEnvelope<Empty>, returning MutationOutcome<OfficeRestartResult>) in apps/agentic-org-web/src/api/org_routes.rs & api.rs.
3. Add CLI `agentic-org office list [--name N]` and `agentic-org office restart --office <id> [--name N]` backed by host.sock authenticated office.list / office.restart (or native session) + real-process HTTP & host tests.
4. Update FC-019 and FC-021 in doc-3, run full checks + fresh two-pass review.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered O1 (FC-019, FC-021): added browser.office_restart (org-host-control + org-host serialized with office_restarting mutex, admin/org-owner authority, host op_journal idempotency), wired GET /api/v1/organizations/{org}/offices (OfficeView stripping endpoint) and POST /api/v1/organizations/{org}/offices/{office}/restart (with 401 vs 403 revalidation), added native host.sock office.list/office.restart with session-first auth, and added agentic-org office list/restart CLI. Verified with cargo fmt/clippy, unit/contract suites (org-host 804+, agentic-org-web 145, agentic-org 38, org-host-control 52), real-process crash-to-active E2E recovery test, and fresh two-pass read-only Rust review (Pass 2 APPROVE). Updated FC-019/FC-021 in doc-3.
<!-- SECTION:FINAL_SUMMARY:END -->
