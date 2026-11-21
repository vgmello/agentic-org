---
id: TASK-76
title: >-
  Local HQ default: HTTP-only & authless direct entry into onboarding wizard
  with opt-in auth and HTTPS
status: To Do
assignee: []
created_date: '2026-11-21T05:16:00Z'
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
- [ ] #1 When HQ is local, the server serves the browser over plain HTTP by default with no HTTPS authorization prompt required.
- [ ] #2 By default on local HQ, authentication is disabled and opening the app lands directly in the organization / onboarding wizard without requiring sign-in or a registration link.
- [ ] #3 Users/teams can opt in to enable authentication (accounts/sign-in) and/or HTTPS from inside the app settings; once enabled, auth/HTTPS enforcement activates cleanly.
- [ ] #4 Onboarding wizard UI flow is coordinated with the UI stream (after TASK-65) and verified end-to-end against a real local HQ instance.
<!-- AC:END -->
