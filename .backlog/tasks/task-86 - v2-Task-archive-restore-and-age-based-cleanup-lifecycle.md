---
id: TASK-86
title: 'v2: Task archive, restore, and age-based cleanup lifecycle'
status: To Do
assignee: []
created_date: '2026-11-23T02:53:00Z'
updated_date: '2026-11-24T04:52:00Z'
labels:
  - v2
  - backend
  - ui
  - work
  - tasks
  - cli
milestone: m-1
dependencies:
  - TASK-49
  - TASK-77
documentation:
  - >-
    .backlog/docs/organization-data/doc-77 -
    Organization-Data-03-—-Features-and-Work.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - >-
    .backlog/docs/research/doc-95 -
    Research-Backlog.md-vs-Agentic-Org-Gap-Analysis.md
  - >-
    .backlog/docs/organization-data/doc-91 -
    Organization-Data-16-—-Recovery-and-Office-Continuity.md
priority: low
type: feature
ordinal: 92000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Long-running multi-agent organizations accumulate thousands of closed, cancelled, and duplicate tasks. Today closed tasks remain in active storage indefinitely (only folded client-side in the UI after 7 days), while permanent deletion tombstones (doc-77) are too destructive for routine hygiene. Introduces a Two-Tier non-destructive Archive and Restore lifecycle (keeping tasks inside office.db — never splitting into a separate tasks.db): (Tier 1 — Warm Archive) nullable archived_at + partial indexes WHERE archived_at IS NULL in office.db so archived tasks move off active boards/default queries while staying read-only, restorable, and preserving all FKs/views/dependency satisfaction; (Tier 2 — Cold Archive) age-based cold sealing of old warm-archived tasks (e.g. >180/365d) into immutable durable/archive/work/<YYYY-MM>/ bundles excluded from hot office.db Delete+Rebuild walks while retaining lightweight identity/dependency stubs so 50k–100k+ historical tasks never bloat office.db or rebuild times.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Tier 1 (Warm Archive): Adds nullable archived_at (and archived_by_kind/archived_by_id) on tasks plus partial indexes WHERE archived_at IS NULL in schema.sql (doc-77, doc-79), moving archived tasks off active boards and default queries while keeping them read-only, queryable via --include-archived / archive view / opt-in SearchRead, and restorable with identical ID, T- key, status, comments, private notes, and immutable history.
- [ ] #2 Archived tasks preserve all dependency/dependent links, FKs, SQL views, and references; a dependency on an archived task in a terminal status (done) continues to count as satisfied, while archiving an open/in-progress task warns or requires explicit confirmation.
- [ ] #3 Tier 2 (Cold Archive & Age-Based Cleanup): Supports age-based cleanup to warm-archive terminal tasks older than a configured threshold (UI and CLI with dry-run preview and confirmation) and a cold-seal maintenance command packing old warm-archived tasks into immutable durable/archive/work/<YYYY-MM>/ bundles (aligning with doc-91 §Retention and doc-79:291 history compaction) excluded from hot office.db Delete+Rebuild walks while retaining lightweight identity/dependency stubs (preserving T- key uniqueness, depends_on satisfaction, usage_records attribution, and private/task-notes.db mirrors); refuses permanent deletion while active subtasks or unresolved references depend on the task.
- [ ] #4 Updates Organization Data contracts (doc-77, doc-79, doc-91), UX spec (doc-49), CLI design (doc-28), and schema.sql with dual-engine (Memory + SQLite), test_schema.py, CLI, and UI tests covering warm archive, restore, dependency satisfaction, cold-seal bundle exclusion from hot rebuild, and cleanup preview/apply.
<!-- AC:END -->
