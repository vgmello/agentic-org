---
id: TASK-88
title: >-
  Unified task checklist sections (acceptance, dod, work), copy-on-create DoD
  defaults with done_anyway gate, and attributed tasks.final_summary
status: To Do
assignee: []
created_date: '2026-11-23T03:22:00Z'
labels:
  - backend
  - ui
  - work
  - tasks
  - cli
  - dod
  - ac
milestone: m-0
dependencies:
  - TASK-87
documentation:
  - >-
    .backlog/docs/organization-data/doc-75 -
    Organization-Data-01-—-Organization-Structure.md
  - >-
    .backlog/docs/organization-data/doc-76 -
    Organization-Data-02-—-Project-Structure.md
  - >-
    .backlog/docs/organization-data/doc-77 -
    Organization-Data-03-—-Features-and-Work.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/design/doc-37 - Design-Tasks-Projects-and-Documents.md
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - >-
    .backlog/docs/research/doc-95 -
    Research-Backlog.md-vs-Agentic-Org-Gap-Analysis.md
priority: high
type: feature
ordinal: 94000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Unifies checkable Acceptance Criteria (ACs), per-task Definition of Done (DoD), and general work checklists onto a single checklist_steps.section model while promoting final_summary to an attributed first-class field on tasks. Retires the unused task_details.definition_of_done prose stub pre-v1, adds copy-on-create org/project default_dod inheritance (offline-safe and historically immutable), wires the existing task_details.done_anyway_json + ChecklistIncomplete gate for required DoD/AC overrides, gives Tess (TASK-80.2) zero-N+1 public outcome summaries without touching private notes, and prepares a clean storage foundation for TASK-78 v2 templates.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 checklist_steps adds section TEXT NOT NULL DEFAULT "work" CHECK (section IN ("acceptance", "dod", "work")) with independent contiguous 0-based position per (task_id, section) (UNIQUE(organization_id, target_kind, target_id, section, position)), retiring the unused task_details.definition_of_done prose column cleanly across schema.sql, Rust, and UI; TaskView exposes per-section counts (ac_total/ac_done, dod_total/dod_done, work_steps_total/work_steps_done) alongside aggregate checklist totals.
- [ ] #2 Organization (organization.yaml / organizations.default_dod_json, owner-only) and Project (project.yaml / projects.default_dod_json, owner or owning-team lead chain; NULL inherits org, explicit array overrides including [] opt-out) support default_dod lists; CreateTask copies resolved defaults (or explicit params.dod) into section="dod" steps with required=true using idempotent deterministic step IDs in one atomic local commit (working offline without HQ; later default edits never mutate existing tasks).
- [ ] #3 TransitionTask to done (resolution completed/fixed) refuses with ErrorCode::ChecklistIncomplete listing unchecked required steps (dod and acceptance default required=true, work defaults required=false) unless a non-empty done_anyway_reason (<= 1000 chars) is provided, which persists {reason, steps} into task_details.done_anyway_json and status_changed history and clears on reopen; cancelled/non-completed closures never gate.
- [ ] #4 tasks stores first-class attributed final_summary (1..=8000 chars), final_summary_at, final_summary_by_kind, and final_summary_by_id (matching updated_by_* checks/FK, never overwritten by comments/watches/ticks); tasks.transition accepts optional final_summary atomically on close (--final-summary + --input-file/--stdin via content_field), tasks.update supports PatchField set/clear, reopening clears live final_summary* while preserving prior summaries in task_events, and WorkDetailPage/WorkDetailDialogs + Tess (TASK-80.2) read final_summary directly from TaskView.
- [ ] #5 Updates schema.sql, test_schema.py, doc-75, doc-76, doc-77, doc-79, doc-49, doc-37, doc-28, doc-71, doc-27 (tasks skill), TASK-78 notes, and adds a concise ADR recording unified checklist sections + copy-on-create DoD + attributed final_summary; Memory/SQLite dual-engine, rebuild, CLI, HTTP, and UI tests pass.
<!-- AC:END -->
