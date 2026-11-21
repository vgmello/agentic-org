---
id: TASK-13
title: >-
  Bring org-host into ADR-0018 compliance, restrict org-office SQLite
  re-exports, and fix fail-open owner_only() and macOS -25308 leaks
status: Done
assignee: []
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T01:30:00Z'
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
- [x] #1 org-host hides SQLite behind an engine-neutral HostStore trait with a memory adapter/testkit, removes rusqlite::ErrorCode from public errors and writer.rs, and enforces the seam with a syn-based engine_boundary test matching org-office.
- [x] #2 org-office restricts SqliteEngine, SqliteSnapshot, and SqliteTx visibility to pub(crate) or test/testkit gates.
- [x] #3 Directory permission tightening in org-office routes through org-platform::fs (failing closed on unsupported OSes), platform_conditionals.rs flags bare cfg(unix)/cfg(windows) outside platform crates, and macOS status -25308 is translated inside org-platform-host.
- [x] #4 cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, and cargo test --workspace pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. H-01 (org-host ADR-0018): Introduce engine-neutral StoreFault, Error::Store(StoreFault), and HostStore trait in crates/org-host/src/hostdb.rs; isolate all SQLite code and rusqlite imports under crates/org-host/src/hostdb/sqlite/; wire Writers to Arc<dyn HostStore>; add MemoryHostStore and dual-engine contract_tests; remove rusqlite from writer.rs and external tests; add syn-based crates/org-host/tests/engine_boundary.rs matching org-office.
2. M-01 (org-office visibility): Make OfficeStore an opaque struct wrapping a private enum; change SqliteEngine, SqliteSnapshot, and SqliteTx to pub(crate) and remove their public re-exports from crates/org-office/src/lib.rs.
3. M-02 (fail-open owner_only, platform_conditionals, macOS -25308): Add fail-closed org_platform::fs::set_owner_only_dir and route org-office/src/store/open.rs through it; eliminate remaining bare #[cfg(unix)] test sites and extend crates/org-platform/tests/platform_conditionals.rs to flag cfg(unix)/cfg(windows) outside platform crates; translate macOS errSecInteractionNotAllowed (-25308) to CredentialError::InteractionNotAllowed inside org-platform-host and map through host_secrets/keys so writer.rs matches the typed variant.
4. Verification: Run cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace, and platform_allowlist_shrinks.py; finalize TASK-13 and commit on fix/task-13-host-adr0018-and-platform-seams.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implemented H-01, M-01, and M-02 on branch fix/task-13-host-adr0018-and-platform-seams.
- H-01: Added engine-neutral StoreFault, Error::Store(StoreFault), and HostStore trait in crates/org-host/src/hostdb.rs; moved all SQLite code and rusqlite imports under crates/org-host/src/hostdb/sqlite/; wired Writers to Arc<dyn HostStore>; added MemoryHostStore and 6 dual-engine conformance tests in hostdb::contract_tests running against both SQLite and MemoryHostStore; removed rusqlite from writer.rs and external tests; added syn-based crates/org-host/tests/engine_boundary.rs enforcing ENGINE_ADAPTER=src/hostdb/sqlite with empty allowlist.
- M-01: Wrapped OfficeStore in an opaque struct with private inner enum in crates/org-office/src/store/open.rs, narrowed SqliteEngine/SqliteSnapshot/SqliteTx to pub(crate) in crates/org-office/src/store/sqlite/mod.rs, and removed their public re-exports from crates/org-office/src/lib.rs.
- M-02: Added fail-closed org_platform::fs::set_owner_only_dir (0o700 on macOS/Linux, Refused::unsupported on unsupported OSes) and routed org-office/src/store/open.rs through it; eliminated remaining bare #[cfg(unix)] test sites and extended crates/org-platform/tests/platform_conditionals.rs to flag bare unix/windows predicates in cfg/cfg_attr/cfg!/cfg_select!/cfg_match!; translated macOS errSecInteractionNotAllowed (-25308) to CredentialError::InteractionNotAllowed inside org-platform-host and mapped through host_secrets/keys so org-host/src/writer.rs never matches raw OSStatus codes.
- Verification: cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, python3 .github/scripts/platform_allowlist_shrinks.py fe9cdb6f, and cargo test --workspace all pass.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Resolved audit findings H-01, M-01, and M-02: isolated org-host SQLite behind HostStore + StoreFault + MemoryHostStore conformance suite + syn engine_boundary guard; restricted org-office SQLite types to pub(crate) behind opaque OfficeStore; replaced fail-open owner_only() with fail-closed org_platform::fs::set_owner_only_dir, extended platform_conditionals.rs to catch cfg(unix)/cfg(windows) without expanding the allowlist, and translated macOS -25308 inside org-platform-host to CredentialError::InteractionNotAllowed. Full workspace fmt, clippy, allowlist, and tests pass.
<!-- SECTION:FINAL_SUMMARY:END -->
