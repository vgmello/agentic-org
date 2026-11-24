---
id: TASK-2
title: >-
  Decouple Doc 11 HQ turn permits from offline execution, hot-path latency, and
  office.db rebuilds
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:30:00Z'
labels:
  - architecture
  - data-model
  - runtime
  - p0
milestone: m-2
dependencies: []
references:
  - docs/design/organization-data/schema.sql
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/11-model-profiles-and-agent-execution.md
  - docs/design/organization-data/14-model-providers-and-connection-pools.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
  - docs/adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md
  - docs/adr/0014-acknowledgement-classes-and-agent-os-latency.md
priority: high
type: bug
ordinal: 2000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings C-01, C-02, C-03: Doc 11 requires HQ turn-permit acquisition and 10-second durable renewals for every turn, contradicting ADR-0032 offline satellite execution (Non-Negotiable #8) and causing turns to abort within 40s on HQ disconnect. Durable 10s permit renewals share the single writer/journal with committed-class agent-os calls, violating 1 ms / 2 ms p95 latency under macOS F_FULLFSYNC (Non-Negotiable #18). Additionally, schema.sql:1739 puts a non-deferred immediate FK from satellite agent_execution_attempts to HQ-only connection_turn_permits, breaking satellite office.db rebuilds (Non-Negotiable #1).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Satellites using office-local or unshared provider bindings execute and sustain agent turns autonomously while disconnected from HQ, reconciling Doc 11 with ADR-0032 and Non-Negotiable #8.
- [x] #2 High-frequency turn permit heartbeats no longer issue durable F_FULLFSYNC writes through the shared committed-class acceptance journal/writer, preserving 1 ms / 2 ms hot-path latency (Non-Negotiable #18).
- [x] #3 Deleting and rebuilding runtime/<office-id>/office.db on a satellite with PRAGMA foreign_keys=ON succeeds without requiring HQ-only connection_turn_permits rows (Non-Negotiable #1).
- [x] #4 Doc 11 foreign keys support arbitrary-order projection rebuilds (DEFERRABLE INITIALLY DEFERRED where cross-record), and test_schema.py passes.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Update Doc 11 (11-model-profiles-and-agent-execution.md) and architecture-non-negotiables.md to introduce office_local permit authority for unshared/per-office bindings alongside hq_pool for shared pools, enabling offline satellite execution (C-01).
2. Keep 10s turn-permit lease heartbeats in-memory (journaling only acquisition, release_pending, and release transitions) so durable flushes never head-of-line block 1 ms / 2 ms committed hot-path commands (C-03).
3. In schema.sql and Doc 11 SQL: add permit_authority on agent_execution_attempts, remove the cross-authority FK to HQ-only connection_turn_permits, and mark Doc 11 FKs DEFERRABLE INITIALLY DEFERRED so satellite office.db rebuilds succeed in arbitrary order with foreign_keys=ON (C-02).
4. Add satellite-rebuild and deferred-insert tests to test_schema.py, verify test_schema.py and cargo test -p org-office, finalize TASK-2, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Updated Doc 11 and architecture-non-negotiables.md with hq_pool vs office_local permit authorities for offline satellite execution and in-memory 10s lease renewals (journaling only acquire/release_pending/release). Added permit_authority on agent_execution_attempts, removed the cross-authority FK to HQ-only connection_turn_permits, and marked Doc 11 FKs DEFERRABLE INITIALLY DEFERRED in schema.sql and Doc 11 (matching Mermaid ERD). Verified test_schema.py (112/112 PASS) and cargo test -p org-office -- store::sqlite (26/26 PASS).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Decoupled Doc 11 turn permits from offline satellite execution (office_local vs hq_pool), hot-path latency (in-memory 10s heartbeats), and satellite office.db rebuilds (removed cross-authority FK, deferrable FKs).
<!-- SECTION:FINAL_SUMMARY:END -->
