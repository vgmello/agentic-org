---
id: TASK-59
title: >-
  L17: Extensions contract plus built-in Containers (Colima) and Xcode
  extensions
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - ui
  - extensions
  - late-v1
milestone: m-0
dependencies:
  - TASK-57
  - TASK-58
documentation:
  - docs/implementation-plan.md
  - docs/design/accounts-sandbox-and-extensions.md
  - docs/workstreams/ui.md
priority: low
type: feature
ordinal: 59000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Run Containers (CON-1..7) and Xcode (XC-1..4) spikes, finalize the ui-dev-led web app extension UI slot (D18, preserving no circular imports and Tailwind Plus licence boundary), and implement the consumer-owned extensions contract under extensions/ (with D17 LNX-5 per-extension platform module exception) plus the built-in Containers (Colima) and Xcode (macOS) extensions (FC-192..194, ASX-14..18).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 CON-1..7 and XC-1..4 spikes are recorded and the web app extension UI slot architecture is agreed between ui-dev and backend-dev.
- [ ] #2 Core never depends on extensions; per-extension platform modules fail closed on unsupported OSes.
- [ ] #3 Containers (Colima per agent account via DOCKER_HOST) and Xcode (checklist, sandbox allowances, signing setup) extensions pass ASX-14..18 and FC-192..194.
<!-- AC:END -->
