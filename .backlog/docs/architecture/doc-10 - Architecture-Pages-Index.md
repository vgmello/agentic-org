---
id: doc-10
title: Architecture Pages Index
type: readme
created_date: '2026-11-21T00:15:00Z'
updated_date: '2026-11-21T00:15:00Z'
tags:
  - architecture
  - index
  - html-assets
---
# Architecture Pages

> [!NOTE] Review note (2026-11-21)
> Updated during Backlog migration (`TASK-21`). The original `docs/architecture/README.md` listed only two HTML pages and omitted the 2026-11-20 interactive audit report (`docs/architecture/architecture-and-data-audit.html`). All three HTML files are retained as static interactive visual assets linked from their canonical Markdown guides in `.backlog/docs/architecture/` until final asset cutover (`TASK-32`). Note also that `v1-integration` is retired; `v1` is the sole integration branch.

## Static Visual Assets & Canonical Guides

| Static HTML Asset | Snapshot Baseline | Canonical Backlog Guide |
| --- | --- | --- |
| `docs/architecture/architecture-overview.html` | 2026-11-18/19 (`v1-integration`, data rev 2.19) | Architecture Overview (HTML Guide) |
| `docs/architecture/system-architecture.html` | 2026-11-18/19 (`v1-integration @ f7e60098`, ADRs 0001–0031) | System Architecture (HTML Guide) |
| `docs/architecture/architecture-and-data-audit.html` | 2026-11-20 (`v1`, data rev 2.23, 42 findings) | Architecture & Organization Data Structure Audit (2026-11-20) |

---

## Original README Text

# Architecture pages

Two self-contained HTML pages for people. Open them in a browser.

| Page | What it is for |
| --- | --- |
| [architecture-overview.html](architecture-overview.html) | The product in one read: the parts, the main rules, what runs today, and what the owner decided in the 2026-11-17 architecture review. |
| [system-architecture.html](system-architecture.html) | The detailed reference: every process and connection, the libraries, the data, the main flows, and each decision with what the code does today. |

These are snapshots written on 2026-11-17 from `v1-integration`, updated on 2026-11-18 for the owner's REQ-06 and REQ-07 and the decisions that followed (development processes and their tabs, project documentation, desired state specifications, hiring power, and what starts an agent), and updated on 2026-11-19 for the `org-agent-runtime` crate rename, the C1 sign-in spike, amended ADR-0025, and ADR-0032. They explain; they do not decide. The contracts are the documents listed in the [documentation authority map](../design/documentation-authority.md), and where a page and a contract differ, the contract wins.

The pages load two fonts from Google Fonts and fall back to system fonts offline. They are not regenerated automatically; update them when the architecture changes.
