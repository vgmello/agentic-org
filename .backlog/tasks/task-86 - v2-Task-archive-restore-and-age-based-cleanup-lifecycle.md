---
id: TASK-86
title: 'v2: Task archive, restore, and age-based cleanup lifecycle'
status: To Do
assignee: []
created_date: '2026-11-23T02:53:00Z'
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
priority: low
type: feature
ordinal: 92000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Long-running multi-agent organizations accumulate hundreds or thousands of closed, cancelled, and duplicate tasks. Today closed tasks remain in active storage indefinitely (only folded in the UI after 7 days), while permanent deletion tombstones (doc-77) are too destructive for routine board hygiene. Introducing a non-destructive Archive and Restore lifecycle (plus optional age-based cleanup of old terminal tasks) keeps active boards, default CLI queries, and agent context windows focused while preserving stable IDs, T- keys, immutable history, and dependency satisfaction.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Tasks can be archived (moving off active boards and default task list queries while staying read-only and queryable via --include-archived / archive view) and restored back to active status with identical ID, T- key, status, comments, private notes, and immutable history.
- [ ] #2 Archived tasks preserve all dependency/dependent links and references; a dependency on an archived task in a terminal status (done) continues to count as satisfied, while archiving an open/in-progress task warns or requires explicit confirmation.
- [ ] #3 Supports age-based cleanup to archive terminal-status tasks older than a configured threshold (via UI and CLI with dry-run/preview and confirmation), and refuses permanent deletion while active subtasks or unresolved references depend on the task.
- [ ] #4 Updates Organization Data contracts (doc-77, doc-79), UX spec (doc-49), CLI design (doc-28), and schema.sql with automated backend, CLI, and UI tests covering archive, restore, dependency satisfaction, cleanup preview/apply, and projection rebuilds.
<!-- AC:END -->
