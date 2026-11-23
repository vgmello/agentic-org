---
id: TASK-89
title: >-
  Task Drafts (TaskStatus::Draft, promote/demote/batch, ADR-0028 wake
  suppression) and 1-level parent_task_id subtasks
status: To Do
assignee: []
created_date: '2026-11-23T03:22:00Z'
updated_date: '2026-11-23T04:07:00Z'
labels:
  - backend
  - ui
  - work
  - tasks
  - cli
  - drafts
  - subtasks
milestone: m-1
dependencies:
  - TASK-87
documentation:
  - >-
    .backlog/docs/organization-data/doc-77 -
    Organization-Data-03-—-Features-and-Work.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - >-
    .backlog/docs/organization-data/doc-74 -
    Organization-Data-—-Schema-Coverage-and-Integrity-Review.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - .backlog/docs/design/doc-37 - Design-Tasks-Projects-and-Documents.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - >-
    .backlog/decisions/decision-28 -
    ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md
  - >-
    .backlog/docs/research/doc-95 -
    Research-Backlog.md-vs-Agentic-Org-Gap-Analysis.md
priority: medium
type: feature
ordinal: 95000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Adds two orthogonal task-staging and hierarchy primitives cleanly before v1 ships: (1) TaskStatus::Draft (6th closed status) with explicit promote/demote/promote_batch operations so people and Tess (TASK-80) can stage, wire dependencies, and assign tasks without waking idle agents under ADR-0028 or cluttering active Kanban boards; and (2) 1-level parent_task_id epic/subtask containment strictly separated from M:N depends_on execution blocking, with ordinary T- keys (ADR-0019) and zero v1 status/assignment cascades (preserving the clean v2 cascade-engine boundary in TASK-73).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 TaskStatus adds draft as a 6th closed status: creating with draft=true (--draft) mints an ordinary T-<OFFICE><N> key immediately, allows assignees/dependencies/parent links, and suppresses ADR-0028 wakes on both CreateTask and AssignTask; TransitionTask refuses transitions from or to draft (directing callers to promote/demote, allowing draft -> cancelled to discard); tasks.promote (draft -> todo) and atomic single-journal tasks.promote_batch re-validate active assignees/scopes and wake all distinct newly-woken active agents; tasks.demote allows only todo -> draft without waking agents.
- [ ] #2 Default task list queries, Kanban board columns, UI isOpen(), and SQL progress views (milestone_progress, release_progress, document_version_progress) exclude draft tasks unless --include-drafts or status=draft is requested; active tasks depending on a draft prerequisite count as derived dependency-blocked (surfacing status: draft on TaskRefSummary for TASK-84).
- [ ] #3 tasks adds nullable parent_task_id with composite FK (organization_id, parent_task_id) and idx_tasks_parent: strictly 1-level hierarchy (enforced in domain, projector, and SQLite triggers rejecting self-parenting, attaching to a subtask or closed parent, or reparenting a task that already has children), matching org/project/accountable-team (while keeping assignee/assigned_team independent), ordinary T- keys that never renumber on reparent/detach, and parent_changed immutable events.
- [ ] #4 Cross-graph prohibition enforced in domain, projector, and SQLite triggers: depends_on links between a parent and its own subtask are forbidden in either direction (sibling and external depends_on work freely); zero v1 cascades (child changes never mutate parent, parent changes never mutate children, respecting TASK-73 boundary), and closing a parent to done/cancelled refuses with offending open/draft child keys until terminal or detached.
- [ ] #5 TaskView exposes parent: Option<TaskRefSummary> and subtask_progress {open, done, cancelled, draft} (excluding cancelled and draft from completion denominator), TaskInspectResult returns subtasks[], task list supports --parent and --top-level (root_only), UI adds Drafts staging view + parent Subtasks panel + child breadcrumb; updates doc-77, doc-79, doc-74, doc-49, doc-37, doc-28, doc-71, ADR-0028, and schema.sql with dual-engine, rebuild, and test_schema.py verification.
<!-- AC:END -->
