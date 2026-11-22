---
id: TASK-84
title: >-
  Task blocked reason, block history, unified Activity & discussion feed, and
  agent-os CLI parity
status: To Do
assignee: []
created_date: '2026-11-22T14:20:00Z'
labels:
  - backend
  - ui
  - work
  - tasks
  - cli
  - ux
milestone: m-0
dependencies:
  - TASK-49
documentation:
  - >-
    .backlog/docs/organization-data/doc-77 -
    Organization-Data-03-—-Features-and-Work.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - >-
    .backlog/docs/organization-data/doc-84 -
    Organization-Data-12-—-Task-Assignment-and-History.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/design/doc-37 - Design-Tasks-Projects-and-Documents.md
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - >-
    .backlog/decisions/decision-31 -
    ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md
  - >-
    .backlog/decisions/decision-6 -
    ADR-0006-Task-changes-are-accepted-through-one-recoverable-office-journal.md
  - >-
    .backlog/decisions/decision-14 -
    ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md
priority: medium
type: feature
ordinal: 87000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
When a task is blocked, people and agents need to see immediately WHY it is blocked while active and retain a complete history of every past block/unblock once resolved. Storing block reasons as standalone comments causes two-step write races for agents and fragile active state, whereas the current UI splits Shared discussion and Activity into separate panels so transition reasons feel disconnected from discussion. This task completes the unified model: active tasks.blocked_reason (required on -> blocked, editable in place, cleared on unblock) backed by immutable status_changed events in task_events, rendered together with shared comments in one chronological Activity & discussion feed, while fixing agent-os CLI read/history blindness. Importantly, a blocked task is NOT assumed to be blocked on the organization owner — it may be waiting on another agent, another team, a dependency, or an external system; only blocks explicitly marked waiting on the owner route to Inbox and use amber Needs you styling.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Backend transition model enforces a required, trimmed, non-empty plain-text blocked_reason (1..=1000 chars, line breaks allowed) when transitioning to blocked, supports in-place blocked -> blocked reason updates recording previous_blocked_reason without resetting continuous blocked duration, atomically clears tasks.blocked_reason on unblock, accepts optional transition notes/reasons on all other status changes, and supports an optional opt-in waiting_on_owner flag (default false) so general blocks (e.g., waiting on another agent or external dependency) stay distinct from owner blocks that create owner waiting spans / Inbox items.
- [ ] #2 agent-os CLI fixes read/write parity: task list and task inspect expose blocked_reason and resolution in TASK_FIELDS/task_to_row(); task history exposes change in EVENT_FIELDS and supports filtering (--event-type status_changed / --blocks-only); task transition adopts --reason as the primary flag (keeping --blocked-reason and --note as aliases, refusing --blocked-reason on non-blocked transitions with an actionable hint instead of silently dropping it, and supporting optional --waiting-on owner); and task list --query searches blocked_reason.
- [ ] #3 UI surfaces active blocked_reason and unified history cleanly across wide and 320 px viewports: Task Detail renders one top neutral Callout with toned icon (danger red for general blocks such as waiting on another agent/dependency; merged amber Blocked · Needs you + Inbox link only for owner blocks), full reason, actor/timestamp, Unblock… (optional note + smart target status), and Edit reason; Board cards and List rows display inline clamped blocked_reason with accessible screen-reader speech; Shared discussion and Activity merge into one chronological Activity & discussion feed (deduplicating comment_added, All/Comments/Changes/Blocks(N) filter, Time-block jump link to past blocks after unblock, Private notes strictly isolated in sibling tab).
- [ ] #4 Documentation, contracts, UX specs, and ADRs are updated to match: doc-77 (blocked_reason invariant), doc-79 (canonical status_changed payload and blocked -> blocked duration rule), doc-84 (preserved block/unblock history), doc-49 (TSK-3, general vs owner block semantics, unified feed), doc-28 (agent-os --reason and history filters), doc-37, doc-71 (task transition/comment/history routes), doc-3 (FC-043/FC-054 evidence), and schema.sql where applicable, preserving ADR-0031, ADR-0006, and ADR-0014 invariants.
- [ ] #5 Automated backend, agent-os CLI, and UI/Playwright tests (wide, 320 px, 200% text, keyboard, screen-reader speech) verify required block reason, 1..=1000 char bounds, in-place reason edit, general vs owner-block distinction, unblock clearing with retained history, unified feed deduplication, and CLI read/transition/history parity.
<!-- AC:END -->
