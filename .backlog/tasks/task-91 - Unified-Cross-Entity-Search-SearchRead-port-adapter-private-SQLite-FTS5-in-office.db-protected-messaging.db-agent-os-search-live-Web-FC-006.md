---
id: TASK-91
title: >-
  Unified Cross-Entity Search (SearchRead port, adapter-private SQLite FTS5 in
  office.db & protected/messaging.db, agent-os search, live Web FC-006)
status: To Do
assignee: []
created_date: '2026-11-23T03:22:00Z'
updated_date: '2026-11-23T04:07:00Z'
labels:
  - backend
  - ui
  - cli
  - search
  - ax
milestone: m-1
dependencies:
  - TASK-88
  - TASK-89
  - TASK-90
documentation:
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-52 - Spec-Knowledge-Decisions-and-Tech-Debt-UX.md
  - .backlog/docs/specs/doc-51 - Spec-Costs-Budgets-and-Insights-UX.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - >-
    .backlog/docs/organization-data/doc-74 -
    Organization-Data-—-Schema-Coverage-and-Integrity-Review.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - >-
    .backlog/decisions/decision-18 -
    ADR-0018-The-storage-engine-is-replaceable-PostgreSQL-must-be-possible.md
  - >-
    .backlog/decisions/decision-31 -
    ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md
  - >-
    .backlog/docs/research/doc-95 -
    Research-Backlog.md-vs-Agentic-Org-Gap-Analysis.md
priority: medium
type: feature
ordinal: 97000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers deterministic, offline-first cross-entity full-text search across Tasks, Projects, Members/Teams, Conversations/Messages, and (as TASK-55 lands) Knowledge Docs/Decisions/Features via a consumer-owned SearchRead port (ADR-0018). Uses adapter-private SQLite FTS5 outside schema.sql (preserving PostgreSQL tsvector parity and automatic zero-migration projection rebuilds) with strict physical database isolation (office.db vs protected/messaging.db; private/task-notes.db and accounting/budgets permanently excluded) and pre-ranking authorization filters. Designed so optional v2 hybrid/semantic search can plug into SearchRead via org_store::ChangeStream with zero CLI, API, or UI contract changes.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Defines consumer-owned SearchRead port in org-office with adapter-private SQLite FTS5 (unicode61 + safe Rust query sanitizer, exact ref/key/slug/path boost, deterministic tie-breakers, bounded excerpts, verified by CI FTS5 unit tests) strictly inside store/sqlite/ (zero VIRTUAL TABLE in schema.sql or messaging-schema.sql; engine_boundary.rs green) plus a pure-Rust MemoryEngine parity adapter passing a shared ENG-3 conformance suite and surviving full runtime/ delete+rebuild offline.
- [ ] #2 Enforces physical DB isolation and pre-ranking security: shared entities live in office.db FTS5, messaging FTS5 lives exclusively inside protected/messaging.db with SQL-level conversation membership/history/owner filtering BEFORE BM25 scoring, COUNT, and LIMIT (negative tests prove non-members get zero hits and zero count leaks); private/task-notes.db (ADR-0031) and protected/{accounting,budgets}.db (COST-9) never enter shared search; paused/offboarded callers receive not_authorized.
- [ ] #3 Exposes search.query (and count mode / search count within the 4 KiB CLI catalogue cap) on office RPC (operations.json policy: serve) and agent-os search (--query, repeatable --type, --modified-file, --status, --project, --team, --assignee, --label, --include-archived, --count, --limit, --cursor, --fields, --format) with truthful SearchCoverage metadata (scope, indexed_entities, unavailable_entities, indexed_at, truncated_bodies) and single-digit ms read latency over session attach.
- [ ] #4 Wires GET /api/v1/organizations/{org}/search in agentic-org-web (strict query validation) and replaces work: [] / unavailableCall stubs in apps/agentic-org-ui (ShellTopbar/ShellSearch FC-006 and SearchPage.svelte WHY-4) with live search; updates doc-28, doc-71, doc-74, doc-3 (FC-006/FC-062/FC-077), doc-27, and adds an ADR documenting v1 FTS5 + v2 pluggable SearchRead hybrid path.
<!-- AC:END -->
