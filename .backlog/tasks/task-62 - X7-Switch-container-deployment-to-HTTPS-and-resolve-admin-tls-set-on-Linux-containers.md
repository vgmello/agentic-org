---
id: TASK-62
title: >-
  X7 & ADR-0033: Default HTTP hosting between browser and office with opt-in
  in-app HTTPS enablement
status: In Progress
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T11:37:00Z'
labels:
  - linux
  - container
  - security
  - m2
milestone: m-0
dependencies:
  - TASK-1
documentation:
  - docs/workstreams/linux.md
  - docs/implementation-plan.md
  - docs/adr/0017-linux-and-container-hosting.md
priority: high
type: feature
ordinal: 62000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement owner requirement (ADR-0033, 2026-11-21) and finish Track X X7: allow the server (macOS, Linux, and container) to start hosted via HTTP between the browser and the office by default to reduce complexity, and let teams opt in to HTTPS inside the app (and via admin tls set). Keep X7 P3 fixes (re-announce tls.renew after failed web fetch and tighten gate cache doc wording) and update deploy/container/setup.json, README, and lnx2.sh accordingly (FC-017, FC-164).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 tls.renew is re-announced after a failed web fetch and gate cache wording is updated.
- [x] #2 Server and container deployments start cleanly over HTTP by default between browser and office without requiring TLS setup up front.
- [x] #3 Teams can enable HTTPS opt-in from the app (via /api/v1 instance TLS settings endpoint + UI settings control) as well as admin tls set on macOS/Linux/containers.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. X7 P3 fixes: wire bounded tls.renew re-announcement callback in org-host web stderr reader when web process reports transient fetch_failed/unavailable; tighten WebTls::verified gate cache doc comment. 2. Opt-in in-app HTTPS enablement: add Instance::upgrade_browser_to_https, org-host-control BrowserTlsEnable (self-signed or operator-supplied cert+key), org-host handler persisting key+pair+instance config (or live-rotating if already HTTPS), and POST /api/v1/instance/tls in agentic-org-web. 3. Document default HTTP + opt-in HTTPS in deploy/container/README.md and verify with real-process HTTP->HTTPS upgrade E2E test.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Backend complete (Pass 1 + Pass 2 + Final Rust review VERDICT: APPROVE): X7 P3 bounded tls.renew retry + gate cache docs; default plain HTTP + opt-in POST /api/v1/instance/tls (self-signed & zeroizing supplied PEM, live HTTPS rotation, HSTS updates, registered restart & CLI origin resolution via browser_matches_setup & registered_with_origin). Remaining: UI Settings HTTPS toggle/form (AC #3).

UI Settings -> Security & Access (/security and /settings/security) wired to GET /api/v1/access and POST /api/v1/instance/tls (self-signed & supplied PEM, HSTS off/on/clear, auth-first guard, origin/fingerprint/restart instructions, live HTTPS cert rotation). Verified with Vitest, svelte-autofixer (0 issues), build/production-import-guard, and Playwright (chromium/production/webkit).
<!-- SECTION:NOTES:END -->
