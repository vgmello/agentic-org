---
id: TASK-57
title: >-
  L15: os-sandbox isolation mode and macOS iOS/Xcode Seatbelt plus Linux sandbox
  spikes
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T14:30:00Z'
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

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (L15 — os-sandbox & Spikes):
1. Research Spikes: execute macOS Seatbelt iOS/xcodebuild+simulator spike and Linux sandbox spike (Landlock vs mount-namespace root vs bubblewrap); record findings in docs/research/ and update ADR-0015 (decision-15).
2. Fail-closed Platform Adapters (crates/org-platform/src/confinement/ & crates/org-supervise/):
   - Add os-sandbox isolation mode with Standard and iOS profiles behind consumer-owned platform traits; every unsupported OS fails closed (AGENTS.md Rule 1).
   - Integrate with org-office-sessions/src/preset.rs and manager.rs across Claude, Codex, and Agy runtimes without breaking private 0700 homes or workspace access.
3. Verification: cargo test -p org-platform -p org-supervise -p org-office-sessions; ASX-11 & FC-190 checks; update doc-3.
<!-- SECTION:PLAN:END -->
