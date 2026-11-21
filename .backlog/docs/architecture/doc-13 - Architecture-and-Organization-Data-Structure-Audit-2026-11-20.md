---
id: doc-13
title: Architecture and Organization Data Structure Audit (2026-11-20)
type: guide
created_date: '2026-11-21T00:15:00Z'
updated_date: '2026-11-21T00:15:00Z'
tags:
  - architecture
  - audit
  - findings
---
# Architecture and Organization Data Structure Audit (2026-11-20)

> [!NOTE] Review note (2026-11-21)
> **Disposition:** `docs/architecture/architecture-and-data-audit.html` (125 KB, interactive audit findings explorer) is retained as a static visual asset linked here until `TASK-32`.
> **Current status:** This is the active 2026-11-20 architectural baseline for `v1` (data contract rev 2.23; `test_schema.py` 111/111 PASS). All remediation items from Section 9 are actively tracked in Backlog.md under `TASK-1`–`TASK-19` (milestones `m-2` P0, `m-3` P1, `m-4` P2).

## Visual Asset

- Open interactive explorer in browser: `docs/architecture/architecture-and-data-audit.html`

## Executive Summary

- **Scope audited:** 28 Rust workspace packages (~266k LOC), static SvelteKit UI (`apps/agentic-org-ui`, 741 files / ~79k LOC), Browser BFF (`apps/agentic-org-web`, 30 files / ~10.6k LOC), 32 ADRs, and `docs/design/organization-data/` (18 specs, `schema.sql`, `private-notes-schema.sql`: 91 shared + 4 private tables, 1,092 columns, 12 views, 78 triggers).
- **Verdict:** Strong foundations (0 crate cycles, 0 stability inversions, zero doc-to-SQL drift, hardened BFF, physical private-note isolation) paired with four structural fault lines and 42 tracked findings.

## Four Core Fault Lines

1. **Offline & Latency Contradictions** — Doc 11 HQ turn-permit admission conflicts with 'Works alone' & hot-path targets (`TASK-2`).
2. **Spec-to-Code Chasm & Integration Cliff** — Zero browser requests reach an office yet (unowned B2 Q8 bridge, 93% UI mocked, 6 unreached crates / 30.1k LOC; `TASK-5`, `TASK-12`, `TASK-14`, `TASK-15`).
3. **Data Model & Privacy Structural Defects** — 64 KiB YAML front-matter cap vs. high-churn comments/entries (`TASK-3`), 4 hard contract/SQL contradictions C-05..C-08 (`TASK-4`), missing SQL indexes & slow views (`TASK-6`), FK/immutability holes (`TASK-7`, `TASK-8`), warm process files & log compaction (`TASK-9`), plaintext `office.db` private DMs/budgets & CA key in Entry 1 (`TASK-10`), OAuth env scrub & private chat in `turn/input.json` (`TASK-11`).
4. **Boundary Leakage & Doc Drift** — `org-host` ADR-0018 gaps & fail-open `owner_only()` (`TASK-13`), BFF multiplexing (`TASK-16`), legacy UI folder layout & `proto.ts` drift (`TASK-17`), SQLite trigger overhead (`TASK-18`), and doc/coverage drift (`TASK-19`).

## Remediation Roadmap Mapping

- **P0 (`m-2`):** `TASK-1` (Green CI), `TASK-2` (Doc 11 permits), `TASK-3` (64 KiB task cap), `TASK-4` (C-05..C-08 contradictions), `TASK-5` (Browser-to-office bridge B2 Q8).
- **P1 (`m-3`):** `TASK-6`..`TASK-12` (indexes/views, SQL integrity, warm processes, privacy isolation, OAuth/input scrubbing, UI productionImportGuard).
- **P2 (`m-4`):** `TASK-13`..`TASK-19` (`org-host` ADR-0018, wire cutover, thin binaries/UUIDv7, BFF SSE/mux, UI modules, trigger reduction, doc sync).
