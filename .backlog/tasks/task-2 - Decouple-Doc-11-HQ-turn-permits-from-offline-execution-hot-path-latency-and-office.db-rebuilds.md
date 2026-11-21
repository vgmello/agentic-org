---
id: TASK-2
title: >-
  Decouple Doc 11 HQ turn permits from offline execution, hot-path latency, and
  office.db rebuilds
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
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
- [ ] #1 Satellites using office-local or unshared provider bindings execute and sustain agent turns autonomously while disconnected from HQ, reconciling Doc 11 with ADR-0032 and Non-Negotiable #8.
- [ ] #2 High-frequency turn permit heartbeats no longer issue durable F_FULLFSYNC writes through the shared committed-class acceptance journal/writer, preserving 1 ms / 2 ms hot-path latency (Non-Negotiable #18).
- [ ] #3 Deleting and rebuilding runtime/<office-id>/office.db on a satellite with PRAGMA foreign_keys=ON succeeds without requiring HQ-only connection_turn_permits rows (Non-Negotiable #1).
- [ ] #4 Doc 11 foreign keys support arbitrary-order projection rebuilds (DEFERRABLE INITIALLY DEFERRED where cross-record), and test_schema.py passes.
<!-- AC:END -->
