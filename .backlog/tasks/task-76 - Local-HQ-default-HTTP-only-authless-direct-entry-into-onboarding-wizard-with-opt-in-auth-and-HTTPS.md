---
id: TASK-76
title: >-
  Local HQ default: HTTP-only & authless direct entry into onboarding wizard
  with opt-in auth and HTTPS
status: In Progress
assignee:
  - '@lead'
created_date: '2026-11-21T05:16:00Z'
updated_date: '2026-11-21T11:37:00Z'
labels:
  - backend
  - ui
  - ux
  - auth
  - security
  - m1
milestone: m-0
dependencies:
  - TASK-65
documentation:
  - >-
    .backlog/decisions/decision-33 -
    ADR-0033-Default-HTTP-server-hosting-between-browser-and-office-with-opt-in-HTTPS-enabled-in-app.md
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - >-
    .backlog/docs/specs/doc-48 -
    Spec-Server-Installation-Sign-In-and-Access-UX.md
priority: high
type: feature
ordinal: 76000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Owner requirement (2026-11-21): to eliminate first-run friction when HQ runs locally, the browser-to-HQ server should run over plain HTTP with no authentication wall by default — removing both the HTTPS certificate authorization step and mandatory initial sign-in/registration. Opening a fresh local HQ instance should land directly in the organization / onboarding wizard.

Authentication (accounts, passwords, registration links, sessions) and HTTPS remain fully available as opt-in settings that a user or team can enable inside the app when needed (coordinating with TASK-62 for opt-in HTTPS and TASK-38 for multi-person auth). Coordinate the onboarding wizard changes with the UI wizard agent once TASK-65 finishes.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 When HQ is local, the server serves the browser over plain HTTP by default with no HTTPS authorization prompt required.
- [x] #2 By default on local HQ, authentication is disabled and opening the app lands directly in the organization / onboarding wizard without requiring sign-in or a registration link.
- [x] #3 Users/teams can opt in to enable authentication (accounts/sign-in) and/or HTTPS from inside the app settings; once enabled, auth/HTTPS enforcement activates cleanly.
- [ ] #4 Onboarding wizard UI flow is coordinated with the UI stream (after TASK-65) and verified end-to-end against a real local HQ instance.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Persistent local authless mode in HostDb (meta local_authless_account + !local-authless placeholder verifier) activating automatically on fresh local HTTP HQ with zero accounts/links while preserving auth_enabled=true on all existing/seeded/HTTPS/link-initialized instances. 2. Add org-host-control ops (browser.local_session, browser.auth_enable, auth.mode_changed event) + BrowserAuth::local_authless, and org-host handlers preserving organization ownership on opt-in auth enablement. 3. Add public GET /api/v1/access, POST /api/v1/instance/auth/enable, and authless-local request routing in agentic-org-web (with strict CSRF guards and instant 401 cutover on auth enable). 4. Prove end-to-end with real-process E2E tests (authless org create -> enable auth -> 401 enforced -> restart persistence).
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Backend complete (Pass 1 + Pass 2 + Final Rust review VERDICT: APPROVE): fresh local HTTP HQ starts authless (meta local_authless_account + !local-authless filtered centrally); GET /api/v1/access, POST /api/v1/instance/auth/enable (with LOCAL_AUTHLESS_SESSION_ID revocation & AUTH_MODE_CHANGED event, atomic ownership preservation), and 409 guard preventing authless HTTP->HTTPS lockout. Verified by real-process E2E tests. Remaining: UI direct onboarding wizard entry & Settings opt-in Auth/HTTPS controls (ACs #2-#4).

UI authless boot via GET /api/v1/access + direct entry into /create (0 orgs) or /welcome (>0 orgs) with zero sign-in/HTTPS wall; opt-in POST /api/v1/instance/auth/enable & POST /api/v1/instance/tls in Security & Access (/security and /settings/security); /cli-approve (SIGN-40 / FC-173) fragment-stripped approval flow. Verified with Vitest (2486 passed), svelte-autofixer (0 issues), format/lint/check/build/e2e:verify, and Playwright (67 passed across chromium/production/webkit). Real-instance E2E (AC #4) remains for Lead merge verification.
<!-- SECTION:NOTES:END -->
