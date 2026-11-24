---
id: TASK-34
title: >-
  B2.2 & B2.3b: Complete B2 dry-run, operation.get, /api/v1 organization routes,
  and UI wire mirrors
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T07:03:00Z'
labels:
  - backend
  - rust
  - bff
  - m1
milestone: m-0
dependencies:
  - TASK-5
  - TASK-14
documentation:
  - .agent-kit/slices/b2-organization-teams-people-plan.md
  - docs/design/web-api-v1.md
  - docs/workstreams/backend.md
priority: high
type: feature
ordinal: 34000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Complete the remaining B2.2 and B2.3b scope not covered by TASK-5 (B2.3a browser-to-office bridge) or TASK-14 (core OrgCommand/OrgSnapshot wire cutover): add org_store::Store::check dry-run, command_id expired/clock_skew validation, operation.get backed by store receipts, /api/v1 organization mutation and operation lookup routes, concurrency test for B2 risk C1 (team move racing descendant rename), and coordinated UI wire mirror updates.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 org_store::Store::check executes dry-run decide+apply+validate+undo with zero journal frames, files, or receipts, and command_id expired/clock_skew rules pass.
- [x] #2 operation.get returns committed and rejected organization receipts for the acting principal and refuses another actor's receipt as not_found.
- [x] #3 All /api/v1/organizations/{org}/* mutation routes and GET /api/v1/operations/org/{id} pass real-process HTTP tests including replay, conflict, and restart.
- [x] #4 Stale pre-B2.1 test vectors are replaced/removed, UI wire mirrors are updated in sync, team-move vs descendant-rename concurrency (risk C1) is tested, and FEATURES FC-025..029 are updated.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add org_store::Store::check dry-run (mark, decide, frame, apply_entry, tx.check, unconditional undo) with memory+SQLite tests.
2. Add dry_run support + command_id expired (>29d)/clock_skew (>5m) validation (after known-receipt replay check) in org-proto and org-office.
3. Implement Office::operation(member, command_id) backed by store receipts (enforcing actor match -> not_found on mismatch) and wire ports.operation_get in org-office-server.
4. Wire all /api/v1/organizations/{org}/* mutation routes and GET /api/v1/operations/org/{id} in agentic-org-web + real-process HTTP tests (replay, conflict, restart).
5. Add B2 risk C1 concurrency test (team move racing descendant rename), prune 25 retired multi-membership vectors in org-commands.json, sync UI wire mirrors after TASK-65 merges, update FC-025..029, and run fresh two-pass review.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Completed TASK-34 (B2.2 & B2.3b) across Rust and UI: store dry-run check, operation.get, command_id expiry/skew/reuse checks, all 11 /api/v1/organizations/{org}/{teams|members}/* routes with web-api-v1 §9 authorization recovery (abb27f6b), plus AC #4 UI wire mirrors (proto.ts 'checked' + 'operation_reused', calibrated immediate operationClock in app-session.svelte.ts, all 11 B2 routes wired in http-api.ts with ULID rejection and fail-closed unsupported checks, and 25 retired multi-membership vectors pruned 252->227 from org-commands.json). Verified with full Rust + UI suites (147/147 files, 2438/2438 tests, build, org-office 275/275) and fresh two-pass read-only reviews (APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
