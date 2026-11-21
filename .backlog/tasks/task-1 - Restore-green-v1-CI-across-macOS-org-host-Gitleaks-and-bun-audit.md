---
id: TASK-1
title: Restore green v1 CI across macOS org-host, Gitleaks, and bun audit
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - ci
  - p0
  - rust
  - ui
milestone: m-2
dependencies: []
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/workstreams/backend.md
  - docs/workstreams/linux.md
priority: high
type: bug
ordinal: 1000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit finding C-10: The v1 integration branch is currently red across three CI jobs (7 macOS org-host test failures, 30 Gitleaks history hits, and bun audit failures). Shipping architecture fixes or vertical slices on a red integration branch masks regressions and blocks reliable verification.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 All 7 failing macOS org-host tests pass locally and in CI without weakening assertions.
- [ ] #2 Gitleaks scan passes cleanly on v1 without bypassing secret scanning rules.
- [ ] #3 UI bun audit check passes cleanly with pinned lockfile dependencies.
- [ ] #4 cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, and cargo test --workspace pass.
<!-- AC:END -->
