---
id: TASK-60
title: >-
  L9 & Diagnostics: Signed release staging, updates, uninstall,
  logs/doctor/support, and copilot live wiring
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
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
<!-- AC:END -->
