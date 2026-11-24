---
id: TASK-54
title: >-
  L4: Provider subscription pools, per-connection model discovery, quota
  permits, cooldown, and failover
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T14:40:00Z'
labels:
  - backend
  - rust
  - agents
  - late-v1
  - blog
milestone: m-0
dependencies:
  - TASK-2
  - TASK-42
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/11-model-profiles-and-agent-execution.md
  - docs/design/organization-data/14-model-providers-and-connection-pools.md
priority: medium
type: feature
ordinal: 54000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement ordered provider connection pools, per-connection endpoint_url/env plumbing and model discovery, HQ quota permit/cooldown coordination, connection-switch stop/relaunch failover, and remaining ASK-4/LIVE-2 turn-permit rules (FC-098, FC-099).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Ordered provider connection pools, bindings, endpoint_url plumbing, and model discovery work per office (FC-098).
- [x] #2 Shared quota permits, rate-limit cooldowns, and connection failover operate cleanly while preserving offline execution per TASK-2 (FC-099).
- [x] #3 Remaining turn-permit clauses of ASK-4 and LIVE-2 pass.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered L4 provider subscription pools, per-connection model discovery, dual-authority quota permits & cooldowns, §7 stop/relaunch failover with side-effect guard, and ASK-4/LIVE-2 turn-permit handoff across org-agent-runtime*, org-proto, org-office (Memory + SQLite rebuildable protected/providers.db), org-office-sessions, org-office-server (11 RPCs + operations.json), and agentic-org-web (11 HTTP routes). Verified with fmt, clippy -D warnings, provider_pool_tests (3/3), engine_boundary (6/6), org-office-sessions (33/33), org-office-server host & watcher tests, agentic-org-web (175/175), test_schema.py (118/118), Repowise exit 0, and 2-pass fresh Neon review (Pass 2 VERDICT: APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
