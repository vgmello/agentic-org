---
id: TASK-60
title: >-
  L9 & Diagnostics: Signed release staging, updates, uninstall,
  logs/doctor/support, and copilot live wiring
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-23T02:53:00Z'
labels:
  - backend
  - rust
  - release
  - late-v1
milestone: m-0
dependencies:
  - TASK-38
  - TASK-52
documentation:
  - docs/implementation-plan.md
  - docs/design/standalone-server.md
  - docs/specs/server-installation-and-access-ux.md
priority: low
type: feature
ordinal: 60000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Complete signed release bundle staging/installation, update activation, uninstall (clearing host-registry.created), redacted logs/doctor/support diagnostics, protected restore/rebind utilities, and live wiring of search/preferences/copilot drafting entries (FC-006, FC-007, FC-022, FC-023, FC-123, FC-124, FC-147, FC-148, FC-154).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Signed offline release staging, verified install, update activation, and clean uninstall clearing host-registry.created pass (FC-022, FC-154).
- [ ] #2 agentic-org logs, doctor, and support commands emit redacted diagnostics (FC-023), and protected restore/rebind paths pass (FC-147, FC-148).
- [ ] #3 Live Search/New backend, device preferences, and copilot text.improve/bug.from_sentence runtime wiring pass verification.
- [ ] #4 agentic-org doctor also validates organization-state integrity (broken task/doc/decision cross-links, display-key clashes, invalid front matter from direct Git edits, and SQLite projection parity) alongside host/office/credential diagnostics.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (L9 & Diagnostics):
1. Release & Lifecycle (crates/org-release, crates/org-host, apps/agentic-org): wire signed offline release bundle staging, signature/hash verification, update activation, and clean uninstall clearing host-registry.created (FC-022, FC-154).
2. Redacted Diagnostics & Recovery: implement agentic-org logs, doctor, and support commands emitting strictly secret-redacted diagnostics (FC-023), plus protected restore/rebind utilities (FC-147, FC-148).
3. Live Copilot & Search/Prefs Wiring (crates/org-copilot, crates/org-office-server, apps/agentic-org-web): wire live org-copilot runtime for text.improve / bug.from_sentence, live Search/New backend endpoints, and device preferences (FC-006, FC-007, FC-123, FC-124).
4. Verification: cargo test -p org-release -p org-copilot -p org-host -p agentic-org -p agentic-org-web; update doc-3.
<!-- SECTION:PLAN:END -->
