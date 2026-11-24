---
id: TASK-91
title: >-
  Unified Cross-Entity Search (SearchRead port, adapter-private SQLite FTS5 in
  office.db & protected/messaging.db, agent-os search, live Web FC-006)
status: To Do
assignee: []
created_date: '2026-11-23T03:22:00Z'
updated_date: '2026-11-24T17:07:00Z'
labels:
  - backend
  - ui
  - cli
  - search
  - ax
milestone: m-0
dependencies:
  - TASK-85.3
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
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - >-
    .backlog/docs/organization-data/doc-86 -
    Organization-Data-18-—-Development-Processes-and-Project-Documentation.md
priority: high
type: feature
ordinal: 97000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers deterministic, offline-first cross-entity full-text search (Tier A) in v1 across Tasks (key, title, body, labels, files, details, checklists, public comments), Org Processes/Standards, Projects, Members/Teams/Roles, Conversations/Messages, permitted Private Task Notes, and (as TASK-55 lands) Project Docs/Decisions/Features via a consumer-owned SearchRead port (ADR-0018). Uses bundled SQLite FTS5 (porter unicode61 + BM25 field weights + bounded snippets) strictly outside schema.sql with 3-way physical DB isolation (office.db, protected/messaging.db, and disposable runtime/<office-id>/private/task-notes-search.db — never altering durable private/task-notes.db and excluding accounting/budgets) plus pre-ranking authorization filters. Unblocks agent multi-query expansion via session attach (<5 ms total) and live browser search (FC-006/FC-077), while TASK-110 plugs opt-in v2 hybrid vector embeddings + RRF behind the exact same SearchRead contract.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Defines consumer-owned SearchRead port in org-office with adapter-private SQLite FTS5 (porter unicode61 with tokenchars '-_./@', safe Rust query sanitizer, field-weighted BM25 boost 10x key/ref > 6x title/path > 4x labels/headings > 2x body > 1x comments/checklists, bounded <=240-char excerpts, deterministic tie-breakers) strictly inside store/sqlite/ (zero VIRTUAL TABLE in schema.sql/messaging-schema.sql/private-notes-schema.sql; dump.rs skips shadow tables; engine_boundary.rs green) plus pure-Rust MemoryEngine parity adapter passing ENG-3 conformance and full runtime/ delete+rebuild offline.
- [ ] #2 Enforces strict 3-way physical DB isolation and pre-ranking security: (1) shared entities in office.db FTS5 (with project-scoped visibility filter for agents on documents once TASK-55 registers them per doc-86); (2) chat messages exclusively in protected/messaging.db FTS5 filtered to caller.visible_conversation_ids BEFORE scoring/COUNT/LIMIT; (3) private task notes exclusively in disposable 0700/0600 runtime/<office-id>/private/task-notes-search.db on HQ (Person sees all non-deleted notes; Agent sees author_id == caller ONLY; satellites omit HQ-only private notes gracefully); protected/{accounting,budgets}.db (COST-9) never enter search; inactive/unbound callers fail closed with not_authorized.
- [ ] #3 Exposes search.query on office RPC (operations.json policy: serve) and agent-os search query (--q, repeatable --types, --project, --channel, --status, --modified-file, --assignee, --label, --count, --limit, --cursor, --fields, --format) within TASK-85.3 4 KiB catalogue headroom returning compact ranked hits + truthful SearchCoverage metadata in <5 ms p95 over session attach; upgrades agent-os task list --query to use the FTS5 index (replacing the 4N+1 literal substring scan); updates tasks-create & batch-and-attach skills (doc-96) to teach agents multi-query expansion.
- [ ] #4 Wires GET /api/v1/organizations/{org}/search in agentic-org-web (strict query validation) and replaces work: [] / unavailableCall stubs in apps/agentic-org-ui (ShellTopbar/ShellSearch FC-006 and SearchPage.svelte WHY-4) with live search; updates doc-28, doc-71, doc-74, doc-96, doc-3 (FC-006/FC-062/FC-077), and records an ADR covering v1 FTS5 + v2 pluggable SearchRead hybrid path.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
2026-11-24 Agent Decision Capture alignment (TASK-111): Index kind, title, why, alternatives, and private_note_tags inside the disposable 0700/0600 HQ-only runtime/<office-id>/private/task-notes-search.db (weighting title 6x > tags 4x > why/alternatives/body 2x), expose --types private_decision alongside shared --types decision with strict pre-ranking author_id == caller filter for agents (and people-only --author filter), keeping WHY-4 shared knowledge search 100% free of private rows.
<!-- SECTION:NOTES:END -->
