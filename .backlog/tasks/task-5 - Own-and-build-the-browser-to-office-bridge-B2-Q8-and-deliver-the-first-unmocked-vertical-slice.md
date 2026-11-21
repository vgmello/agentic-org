---
id: TASK-5
title: >-
  Own and build the browser-to-office bridge (B2 Q8) and deliver the first
  unmocked vertical slice
status: In Progress
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T01:32:00Z'
labels:
  - architecture
  - bff
  - ui
  - rust
  - p0
milestone: m-2
dependencies:
  - TASK-1
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/implementation-plan.md
  - docs/design/web-api-v1.md
  - docs/workstreams/backend.md
  - docs/workstreams/ui.md
priority: high
type: feature
ordinal: 5000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit finding C-10: Zero browser requests currently reach an office because the browser-to-office bridge (B2 Q8) is unowned, leaving 93% of apps/agentic-org-ui backed by mocks and stalling the v1 dogfooding goal while UI contracts drift behind B2.1 backend models. Establishing the live web-to-office channel and driving one real vertical slice end-to-end validates the full stack before expanding scope.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Browser-to-office transport (B2 Q8) is implemented between agentic-org-web and agentic-org-office while preserving agentic-org-web's zero-DB and zero-office-key boundary.
- [ ] #2 At least one complete in-org vertical slice (e.g., organization/teams/employees read & mutation) runs end-to-end from agentic-org-ui through agentic-org-web to live org-office/org-store data without mock fallbacks.
- [ ] #3 End-to-end automated integration/browser verification proves live data round-trips and survives office restart/rebuild.
- [ ] #4 docs/FEATURES.md and docs/workstreams/{backend,ui}.md are updated with verified evidence.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add browser-authenticated office channel & grant host-control ops (browser.open_channel, browser.grant) in crates/org-host-control and crates/org-host (admitted web connection only, BrowserAuth validation, zero DB/office-key exposure to web).
2. Implement OfficeBridge in apps/agentic-org-web receiving office channel FDs over the existing transfer UnixDatagram socket, obtaining scoped HMAC Grants per browser session, and forwarding typed org.snapshot and team.create calls to live agentic-org-office.
3. Serve GET /api/v1/organizations/{org}/snapshot and POST /api/v1/organizations/{org}/teams in apps/agentic-org-web/src/api.rs with strict DTO validation and reply classification.
4. Wire apps/agentic-org-ui production orgApi (view + team.create) to live /api/v1/organizations/{org}/snapshot and /api/v1/organizations/{org}/teams with zero mock fallbacks.
5. Add real-process end-to-end integration test in crates/org-host/src/web_process_tests/ proving browser -> web -> host -> office live org snapshot + team.create mutation survives office restart and runtime/office.db deletion/rebuild, plus UI tests/build.
<!-- SECTION:PLAN:END -->
