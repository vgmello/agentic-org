---
id: TASK-84
title: >-
  Task blocking model (derived dependency blocks vs explicit blocked reasons),
  block history, unified Activity & discussion feed, and agent-os CLI parity
status: To Do
assignee: []
created_date: '2026-11-22T14:20:00Z'
updated_date: '2026-11-22T14:40:00Z'
labels:
  - backend
  - ui
  - work
  - tasks
  - cli
  - ux
  - dependencies
  - blog
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
Tasks can be blocked in two fundamentally different ways that must be modeled and visualized distinctly: (1) Derived / virtual dependency blocks — a task has unfinished upstream tasks in depends_on (TASK-77), so it is automatically blocked by those prerequisite tasks with NO manual status flip and NO comment/reason required, and automatically unblocks when those dependencies finish; and (2) Explicit external/manual blocks (status = blocked) — someone or an agent hits something outside the task dependency graph (waiting on another agent, an external vendor/API/credential, an outage, a budget hold, or an owner decision), which REQUIRES a non-empty blocked_reason and preserves a complete history of every block/unblock. Within explicit blocks, a task is NOT assumed to be blocked on the owner — only blocks explicitly marked waiting_on_owner (or linked to an owner question) route to Inbox and use amber Needs you styling, so the owner immediately distinguishes what they can unblock from what is blocked on another agent, external factor, or task dependency. Storing explicit block reasons as standalone comments causes two-step write races for agents and fragile active state; instead, active tasks.blocked_reason is backed by immutable status_changed events in task_events and rendered alongside shared comments in one chronological Activity & discussion feed, while fixing agent-os CLI read/history blindness.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Two-kind blocking model in backend & API: (a) Derived dependency blocks are computed automatically from unfinished depends_on tasks (exposing blocking dependency summaries/counts on task list/inspect views without requiring status = blocked or a comment, auto-clearing when prerequisites reach done, and flagging cancelled upstream dependencies clearly); (b) Explicit manual/external blocks (status = blocked) enforce a required, trimmed, non-empty plain-text blocked_reason (1..=1000 chars, line breaks allowed), support in-place blocked -> blocked reason updates recording previous_blocked_reason without resetting continuous blocked duration, atomically clear tasks.blocked_reason on unblock, accept optional transition notes/reasons on all other status changes, and support an opt-in waiting_on_owner flag (default false) so general/agent/external blocks remain distinct from owner-actionable blocks.
- [ ] #2 agent-os CLI fixes read/write parity: task list and task inspect expose blocked_reason, resolution, and derived dependency-block state (unmet blocking dependencies + --ready / dependency-blocked filtering); task history exposes change in EVENT_FIELDS and supports filtering (--event-type status_changed / --blocks-only); task transition adopts --reason as the primary flag (keeping --blocked-reason and --note as aliases, refusing --blocked-reason on non-blocked transitions with an actionable hint instead of silently dropping it, and supporting optional --waiting-on owner); and task list --query searches blocked_reason.
- [ ] #3 UI visualizes all three blocking states distinctly across wide and 320 px viewports without conflating them: (1) Derived dependency block — quiet neutral/dependency badge and callout/sub-line ("Waiting on T-A12, T-A15" with clickable task links, no manual comment required, auto-clears on completion); (2) Explicit general/agent/external block — top neutral Callout with danger red icon, full blocked_reason, Blocked by <Actor> · <time>, Unblock… (optional note + smart target status), and Edit reason; (3) Explicit owner block — merged amber Blocked · Needs you icon + Inbox link. Both explicit and dependency blocks can coexist cleanly on one task. Board cards and List rows display inline clamped block/dependency context with accessible screen-reader speech, and Shared discussion + Activity merge into one chronological Activity & discussion feed (deduplicating comment_added, All/Comments/Changes/Blocks(N) filter, Time-block jump link to past blocks after unblock, Private notes strictly isolated in sibling tab).
- [ ] #4 Documentation, contracts, UX specs, and ADRs are updated to match: doc-77 (two-kind blocking model + blocked_reason invariant), doc-79 (canonical status_changed payload and blocked -> blocked duration rule), doc-84 (preserved block/unblock history), doc-49 (TSK-3, derived dependency blocks vs explicit general/owner blocks, unified feed), doc-28 (agent-os --reason, --ready, and history filters), doc-37, doc-71 (task transition/comment/history routes), doc-3 (FC-043/FC-054 evidence), and schema.sql where applicable, preserving ADR-0031, ADR-0006, and ADR-0014 invariants.
- [ ] #5 Automated backend, agent-os CLI, and UI/Playwright tests (wide, 320 px, 200% text, keyboard, screen-reader speech) verify derived dependency-block calculation and auto-clear (including cancelled dependency surfacing), required explicit block reason (1..=1000 chars), in-place reason edit, general vs owner-block distinction, coexisting dependency + explicit block rendering, unblock clearing with retained history, unified feed deduplication, and CLI read/transition/history parity.
<!-- AC:END -->
