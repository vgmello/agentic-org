---
id: doc-93
title: Organization Data — Executable Schema Assets Index
type: guide
created_date: '2026-11-21T01:30:00Z'
updated_date: '2026-11-23T06:02:00Z'
tags:
  - organization-data
  - schema
  - code-assets
---
# Executable Organization Data Schema Assets

Status: **code-owned executable artifacts** (reviewed 2026-11-21, `TASK-26`).

These five files remain at `docs/design/organization-data/` (retained as code-owned executable assets because `crates/org-office/src/store/sqlite/schema.rs` and `crates/org-office/src/store/sqlite/spike.rs` compile-time embed `schema.sql` via `include_str!` and CI runs `test_schema.py` against them):

| Artifact | Purpose | Tables / Scope | Consumer |
| --- | --- | --- | --- |
| `docs/design/organization-data/schema.sql` | Shared office SQLite projection (`runtime/<office-id>/office.db`) | 76 shared tables + 12 views + indexes/triggers | `crates/org-office/src/store/sqlite/schema.rs` (`include_str!`), `test_schema.py` |
| `docs/design/organization-data/private-notes-schema.sql` | HQ-authoritative durable private task notes DB (`durable/private/task-notes.db`) | 4 tables (Doc 05, ADR-0031) | `test_schema.py`, planned L8 (`TASK-50`) |
| `docs/design/organization-data/messaging-schema.sql` | Isolated protected messaging DB (`runtime/<office-id>/protected/messaging.db`) | 8 messaging tables + 5 identity mirrors (Doc 15, ADR-0016, `TASK-10`) | `test_schema.py`, planned B6 (`TASK-45`) |
| `docs/design/organization-data/budgets-schema.sql` | Isolated protected budgets DB (`runtime/<office-id>/protected/budgets.db`) | 6 budget tables + 4 identity mirrors (Doc 17, ADR-0016, `TASK-10`) | `test_schema.py`, planned L3 (`TASK-53`) |
| `docs/design/organization-data/test_schema.py` | Executable Python stdlib schema verification suite (116 tests) | Verifies SQL/doc parity, Mermaid ER diagrams, `SCHEMA-COVERAGE.md`, `EXPLAIN QUERY PLAN` indexes/views, and all trigger/FK invariants | CI & local verification |

Run verification with:

```bash
python3 -B docs/design/organization-data/test_schema.py
cargo test -p org-office -- store::sqlite
```
