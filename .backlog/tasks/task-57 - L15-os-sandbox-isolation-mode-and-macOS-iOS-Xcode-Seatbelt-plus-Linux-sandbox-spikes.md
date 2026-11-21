---
id: TASK-57
title: >-
  L15: os-sandbox isolation mode and macOS iOS/Xcode Seatbelt plus Linux sandbox
  spikes
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - security
  - sandbox
  - late-v1
milestone: m-0
dependencies:
  - TASK-42
documentation:
  - docs/implementation-plan.md
  - docs/adr/0015-agent-isolation-modes.md
  - docs/design/accounts-sandbox-and-extensions.md
priority: medium
type: spike
ordinal: 57000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Run the macOS iOS-build spike (building an iOS app with xcodebuild and running simulator tests under Seatbelt) and the Linux sandbox spike (Landlock vs mount-namespace root vs bubblewrap), record results in docs/research/, and implement the os-sandbox isolation mode with standard and iOS profiles across agent runtimes (FC-190, ASX-11).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 macOS Seatbelt iOS/Xcode spike and Linux sandbox spike results are recorded and corresponding ADR updates are written.
- [ ] #2 Office confines agent processes under os-sandbox with standard and iOS profiles across supported runtimes without bypassing fail-closed platform adapters.
- [ ] #3 ASX-11 and FC-190 criteria pass (or documented fallback for iOS simulators is verified).
<!-- AC:END -->
