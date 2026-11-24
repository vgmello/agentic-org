---
id: TASK-38
title: >-
  A6: Multi-person accounts — browser invites, password resets, access removal,
  and ownership handover
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T13:00:00Z'
labels:
  - backend
  - rust
  - auth
  - m1
milestone: m-0
dependencies:
  - TASK-34
documentation:
  - docs/implementation-plan.md
  - docs/specs/server-installation-and-access-ux.md
  - docs/design/organization-data/04-agent-structure.md
priority: high
type: feature
ordinal: 38000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Enable inviting a second person in the browser via one-time registration links, password reset links, self-service password change (ending other sessions), access removal, admin and organization ownership handover, UUIDv7 account IDs, session refresh/touch batching (A3 B7/B8 leftovers), and SIGN-34 SSH refusal plus admin reset dialog wording (FC-012, FC-014, FC-029, FC-153, FC-159).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Two people can sign in concurrently from separate browsers using browser-issued registration links and each action carries their tied person record.
- [x] #2 Set-password links, password change invalidating other sessions, access removal revoking sessions, and admin/owner handover work end-to-end.
- [x] #3 Dead CLI account.create ops are removed, account IDs are UUIDv7, session touch batching is implemented, and SIGN-34 SSH refusal/dialog copy pass.
- [x] #4 FEATURES rows FC-012, FC-014, FC-029, FC-153, and FC-159 are updated.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Slice 1: UUIDv7 AccountIds (backward-compatible CHECK IN (26,36)), LinkKind::SetPassword + subject-bearing redeem_in, HostDb multi_person module (invites, SIGN-35 attach, set-password, change-password, remove-access, org-owner handover, admin handover), TouchBatcher session touch batching, Admin::reset set-password link + SIGN-34 macOS SSH refusal & dialog copy, remove dead CLI account.create/set_password/reset_password ops from org-host-control. Slice 2: org-host-control browser_accounts ops, org-host service/multi_person handlers, agentic-org-web api/account_routes HTTP routes. Slice 3: Unit tests + real-process E2E proof with two concurrent signed-in browsers, password change/reset, handovers, and access removal; sync doc-3 FC-012/014/029/153/159.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Completed TASK-38 (A6: Multi-person accounts): UUIDv7 account IDs (legacy ULID compatible), removed dead account.create/set_password/reset_password ops, added LinkKind::SetPassword, TouchBatcher (B7), browser multi-person host/web ops & 10 /api/v1 routes (org invites, SIGN-35 existing-account attach, link inspection, set-password reset clearing lockout and ending all sessions, own password change ending other sessions, access removal ending sessions, and password-confirmed org-owner + instance-admin handover with DB-first multi-office custody rollback and graceful worker reload), host-channel multi-person authority in org-office-server, SIGN-34 macOS SSH refusal and per-action ConfirmReason presence prompts, doc-3 updates (FC-012, FC-014, FC-029, FC-153, FC-159), and real-process E2E verification. Double-reviewed (Pass 1 -> repairs -> Pass 2 -> repairs -> Fresh Final Review VERDICT: APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
