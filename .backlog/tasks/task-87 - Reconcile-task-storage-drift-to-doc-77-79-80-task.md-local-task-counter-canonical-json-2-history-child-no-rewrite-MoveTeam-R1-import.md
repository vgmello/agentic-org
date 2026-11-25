---
id: TASK-87
title: >-
  Reconcile task storage drift to doc-77/79/80 (task.md, local task-counter,
  canonical-json/2 history, child no-rewrite, MoveTeam)
status: To Do
assignee: []
created_date: '2026-11-23T03:22:00Z'
updated_date: '2026-11-25T00:16:00Z'
labels:
  - backend
  - rust
  - storage
  - tasks
  - data-contract
milestone: m-0
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
  - >-
    .backlog/docs/organization-data/doc-80 -
    Organization-Data-06-—-Identity-and-Numbering.md
  - >-
    .backlog/docs/organization-data/doc-82 -
    Organization-Data-08-—-Record-Contracts-and-Hash-Codec-Registry.md
  - >-
    .backlog/docs/organization-data/doc-84 -
    Organization-Data-12-—-Task-Assignment-and-History.md
  - .backlog/docs/design/doc-37 - Design-Tasks-Projects-and-Documents.md
  - >-
    .backlog/docs/research/doc-95 -
    Research-Backlog.md-vs-Agentic-Org-Gap-Analysis.md
  - >-
    .backlog/decisions/decision-6 -
    ADR-0006-Task-changes-are-accepted-through-one-recoverable-office-journal.md
  - >-
    .backlog/decisions/decision-21 -
    ADR-0021-The-office-store-one-journal-of-file-changes-a-projection-rebuilt-from-files.md
  - >-
    .backlog/docs/design/doc-100 -
    Design-Storage-Scalability-DB-Split-Verdict-and-Two-Tier-Task-Archive.md
priority: high
type: enhancement
ordinal: 93000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Inside durable/state/ (the Git-replicated org tree), every other domain (organization, profiles, projects, processes, standards) uses YAML or Markdown+YAML via org_docs, while tasks is the sole outlier writing JSON under flat state/work/tasks/<id>/*.json. This drift also hides 5 critical defects that would break TASK-52 (multi-office replication) and TASK-78 (Markdown task templates): office-local task counters live inside Git-replicated state/work/, child ops (comments/watches/checklists) rewrite task.json with Ack::Durable instead of Ack::Committed, events use ad-hoc hashes instead of canonical-json/2 under work-history/, MoveTeam does not relocate projectless tasks, and R1 Git import omits TaskFiles/ProcessFiles. Replace cleanly pre-v1 with zero dual-format shims (Non-Negotiable #14, doc-72).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 All task mutations persist exclusively at state/projects/<project-id>/work/tasks/<task-id>/ (project-scoped) or state/teams/<team-chain>/work/tasks/<task-id>/ (projectless) with task.md (YAML front matter + Markdown body via org_docs/SourceDoc including links[]), comments/<comment-id>.md, checklist.yaml, and operations.yaml; no files are written under state/work/ and zero dual-format task.json shims remain.
- [ ] #2 Office-local task number counters persist exclusively at durable/local/offices/<office-id>/task-counter.yaml (never under state/), owned by TaskFiles::new(office_id) via the acceptance journal and rebuilt locally for the opening office only.
- [ ] #3 Comment, watch/unwatch, and checklist operations write ONLY their sibling file plus the next immutable history event without rewriting task.md; Ack classes match doc-79:289 across both office and CLI task_table (Ack::Committed for task create/comment/watch/checklist-check, Ack::Durable for core task mutations); effective tasks.revision = max(task.md.revision, max(task_events.revision)) in both SQLite and Memory so delete+rebuild preserves exact optimistic concurrency.
- [ ] #4 Task events append immutably (Change::create) to state/work-history/tasks/<task-id>/<revision>.json encoded and hash-verified via canonical-json/2 (schema_version 2, object change/source_metadata, full-object payload_sha256, creation public_snapshot), rejecting hash mismatches, gaps, or task.md.revision > max_event_rev on rebuild.
- [ ] #5 MoveTeam atomically relocates descendant teams/<chain>/work/tasks/<id>/ directories in the same journal entry, DurablePath::MAX_BYTES is raised (e.g. 1024) with headroom checks covering MAX_TEAM_DEPTH=16 task comment paths, and R1 Git import wires TaskFiles and ProcessFiles alongside OrganizationFiles and ProjectFiles.
- [ ] #6 Dual-engine (Memory + SQLite) tests, delete+rebuild parity after child-only ops (verifying revision preservation), test_schema.py, engine_boundary, cargo fmt/clippy/test pass, and doc-37, doc-77, doc-79 (L374 revision rule & checklist.yaml schema), doc-84, doc-15, doc-3 (FC-039..056, FC-142), and ADR-0006/ADR-0021 review notes are updated.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Update (2026-11-24): R1 Git import wiring (TaskFiles/ProcessFiles in apply_imported_to_engine, ARCH-06 Part B) was already shipped on v1 in TASK-97 (9ec28e8b) — do not duplicate it here. Preserve #[serde(default)] attachments[] front matter cleanly for TASK-98.4.

Architect Review Note (2026-11-24): When moving CreateTask from Ack::Durable to Ack::Committed (AC #3, ADR-0014 d3), implement ADR-0019 durable block reservation on office_task_counters.reserved_through (tasks/features/core.rs:93-100, 140) so office restarts never collide T- numbers while keeping routine task creation <1 ms. Keep tasks inside office.db (never split into a 3rd tasks.db).
<!-- SECTION:NOTES:END -->
