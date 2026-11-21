---
id: TASK-13
title: >-
  Bring org-host into ADR-0018 compliance, restrict org-office SQLite
  re-exports, and fix fail-open owner_only() and macOS -25308 leaks
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - rust
  - architecture
  - security
  - p2
milestone: m-4
dependencies:
  - TASK-1
references:
  - crates/org-host/src/hostdb.rs
  - crates/org-host/src/writer.rs
  - crates/org-office/src/lib.rs
  - crates/org-office/src/store/open.rs
  - crates/org-platform/tests/platform_conditionals.rs
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/adr/0017-linux-and-container-hosting.md
  - docs/adr/0018-storage-engine-independence.md
priority: medium
type: enhancement
ordinal: 13000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings H-01, M-01, M-02: org-host leaks rusqlite::ErrorCode in hostdb::Error::Sql and writer.rs with no engine-neutral trait or memory conformance testkit, violating ADR-0018 (Non-Negotiable #6). org-office unconditionally re-exports SqliteEngine/SqliteSnapshot/SqliteTx publicly despite zero external callers. Additionally, org-office/src/store/open.rs:196-205 returns Ok(()) under #[cfg(not(unix))] (failing open in violation of ADR-0017), and org-host/src/writer.rs:145 hardcodes macOS keychain status -25308.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 org-host hides SQLite behind an engine-neutral HostStore trait with a memory adapter/testkit, removes rusqlite::ErrorCode from public errors and writer.rs, and enforces the seam with a syn-based engine_boundary test matching org-office.
- [ ] #2 org-office restricts SqliteEngine, SqliteSnapshot, and SqliteTx visibility to pub(crate) or test/testkit gates.
- [ ] #3 Directory permission tightening in org-office routes through org-platform::fs (failing closed on unsupported OSes), platform_conditionals.rs flags bare cfg(unix)/cfg(windows) outside platform crates, and macOS status -25308 is translated inside org-platform-host.
- [ ] #4 cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, and cargo test --workspace pass.
<!-- AC:END -->
