---
id: doc-11
title: Architecture Overview (HTML Guide)
type: guide
created_date: '2026-11-21T00:15:00Z'
updated_date: '2026-11-21T00:15:00Z'
tags:
  - architecture
  - overview
  - html-guide
---
# Architecture Overview (HTML Guide)

> [!NOTE] Review note (2026-11-21)
> **Disposition:** `docs/architecture/architecture-overview.html` (143 KB, self-contained interactive visual page) is retained as a static visual asset linked here until `TASK-32`.
> **Staleness / Drift:** Snapshot from 2026-11-18/19 (`v1-integration`, `organization-data` rev 2.19). Since that snapshot: `v1-integration` was retired in favor of `v1`; data contract reached rev 2.23; B1b, B2.1, C1 (PR 17), X5, and X7 core TLS merged on `v1`; and owner decisions D1–D28 (2026-11-20, `accounts-sandbox-and-extensions.md`) deferred recovery-code/CA escrow to v2 (D24), added `os-sandbox` (L15), dedicated accounts/setup (L16), and Containers/Xcode extensions (L17). This overview explains; authoritative contracts and `doc-1` (Vision) win where they differ.

## Visual Asset

- Open in browser: `docs/architecture/architecture-overview.html`

## Sections Covered

1. **01 · The product in one minute** — Self-hosted server running an organization of people and autonomous AI agents; two front doors (browser UI + `agent-os`); files first, SQLite as rebuildable projection; desktop shell removed (ADR-0005).
2. **02 · System at a glance** — Four executables (`agentic-org`, `agentic-org-office`, `agentic-org-web`, `agent-os`), static SvelteKit UI, and supervised vendor runtimes (`claude`, `codex`, `agy`).
3. **03 · The runtime parts** — Single-install host (`agentic-org serve`), accounts/people/agents separation, stored browser sessions (ADR-0013), `agent-os` 1–2 ms hot-path target (ADR-0014), provider pools.
4. **04 · Domain model** — Teams forest, typed tasks, no transition engine, organization processes & standards vs. composable development processes (ASDP), process-declared project tabs, project documentation tree.
5. **05 · Where truth lives** — `durable/` vs `runtime/<office-id>/`, YAML/Markdown/JSON file formats, immutable task history, no application-level DB encryption (ADR-0016), agent private homes & isolation (ADR-0015), macOS/Linux/container hosting (ADR-0017).
6. **06 · Rules everything aims at** — Single-writer office store, replaceable storage engine (ADR-0018), office-coded task numbers (ADR-0019).
7. **07 · Who decides what** — Autonomous engineering decisions inside the 18 non-negotiables.
8. **08–13 · Code map, Where we are, Decisions, Build order, Engineer onboarding, Reading map** — High-level orientation (superseded for live tracking by Backlog.md `TASK-1`–`TASK-75`).
