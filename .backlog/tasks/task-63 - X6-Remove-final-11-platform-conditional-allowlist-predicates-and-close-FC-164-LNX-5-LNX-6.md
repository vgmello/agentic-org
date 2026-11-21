---
id: TASK-63
title: >-
  X6: Remove final 11 platform-conditional allowlist predicates and close FC-164
  (LNX-5, LNX-6)
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - linux
  - rust
  - ci
  - m2
milestone: m-0
dependencies:
  - TASK-62
documentation:
  - docs/workstreams/linux.md
  - docs/implementation-plan.md
  - docs/adr/0017-linux-and-container-hosting.md
priority: high
type: enhancement
ordinal: 63000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Eliminate the last 11 platform predicates in apps/agentic-org-office/src/startup/tests.rs (8) and apps/agentic-org-office/tests/launch/activation/transfer.rs (3) using org_platform::testing so .github/platform-conditionals.allow is empty, confirm macOS + Linux + container CI checks pass, and mark FC-164 Done (noting session attach timings follow B6).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 .github/platform-conditionals.allow has zero remaining entries and platform_conditionals test passes on macOS and Linux.
- [ ] #2 LNX-5 and LNX-6 pass in CI across macOS, Linux, and container jobs with zero excluded crates.
- [ ] #3 FEATURES row FC-164 is updated to Done with current CI evidence.
<!-- AC:END -->
