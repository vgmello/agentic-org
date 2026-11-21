---
id: doc-12
title: System Architecture (HTML Guide)
type: guide
created_date: '2026-11-21T00:15:00Z'
updated_date: '2026-11-21T00:15:00Z'
tags:
  - architecture
  - system-architecture
  - html-guide
---
# System Architecture (HTML Guide)

> [!NOTE] Review note (2026-11-21)
> **Disposition:** `docs/architecture/system-architecture.html` (161 KB, self-contained interactive reference page) is retained as a static visual asset linked here until `TASK-32`.
> **Staleness / Drift:** Snapshot from `v1-integration @ f7e60098` (2026-11-18/19, `organization-data` rev 2.19, ADRs 0001–0031). Known outdated items in the HTML snapshot: (1) omits ADR-0032 (browser-driven CLI enrollment & permission presets) and 2026-11-20 owner decisions D1–D28 (recovery deferred to v2, L15–L17); (2) marks organization creation as 'Built, not reachable' even though A3 B4–B6 & A4 browser registration/create landed on `v1` (while browser-to-office channel B2.3a remains open in `TASK-5`); (3) predates B1b store cutover, B2.1 single-home-team domain, C1 PR 17 (`org-agent-runtime-claude`), X5, and X7 core TLS. Authoritative contracts and ADRs always win over this visual reference.

## Visual Asset

- Open in browser: `docs/architecture/system-architecture.html`

## Sections Covered

- **01 · Process topology** — Single-instance process model (`agentic-org serve` host, sandboxed `agentic-org-web` with no DB/keys, `agentic-org-office` single-writer domain worker, `agent-os` agent CLI, supervised agent runtimes).
- **02 · Connections** — Local Unix sockets, verified launch rendezvous, descriptor passing, wire framing & versioning.
- **03 · Code structure** — Workspace crates, apps, and target domain-module vertical-slice layout.
- **04 · Storage** — Data classes (Organization Git, protected local durable files, runtime projections), file formats, engine-independent contracts (ADR-0018), journal/flusher/rebuild model (ADR-0021).
- **05 · Identity and numbering** — UUIDv7 IDs, HQ counters, offline office-coded task numbers (ADR-0019).
- **06–07 · Browser request path & Accepted writes** — Session cookie+proof, host grants, idempotent operation receipts, expected revisions.
- **08–09 · Agent execution, Providers, Usage & Budgets** — Headless sessions (ADR-0004), turns/attempts/permits, pools/cooldowns, usage ledger, independent budget holds.
- **10–12 · Messaging, Git & Repositories, HQ & Satellites** — Protected messaging, bare caches & per-task worktrees, satellite enrollment & sync.
- **13–14 · Secrets, Trust boundaries, Install, Release & Recovery** — Key custody (ADR-0016), isolation modes (ADR-0015), setup/registration/recovery flows, plus status matrix and review decisions.
