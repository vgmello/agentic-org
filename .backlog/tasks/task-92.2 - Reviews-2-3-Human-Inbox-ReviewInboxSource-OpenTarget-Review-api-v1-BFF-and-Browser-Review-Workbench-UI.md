---
id: TASK-92.2
title: >-
  Reviews 2/3: Human Inbox (ReviewInboxSource / OpenTarget::Review), /api/v1
  BFF, and Browser Review Workbench UI
status: To Do
assignee: []
created_date: '2026-11-23T03:40:00Z'
labels:
  - backend
  - ui
  - inbox
  - reviews
milestone: m-0
dependencies:
  - TASK-92.1
documentation:
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/specs/doc-52 - Spec-Knowledge-Decisions-and-Tech-Debt-UX.md
parent_task_id: TASK-92
priority: high
type: feature
ordinal: 100000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Surfaces first-class Universal Reviews to people via the live Inbox, authenticated /api/v1 BFF routes, and a dedicated Svelte 5 Browser Review Workbench (unified/split code diff viewer, Markdown/artifact viewer, inline anchored finding threads, round comparison, and atomic verdict submission).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Adds ReviewInboxSource to crates/org-office-server/src/inbox.rs with InboxKind::Review, OpenTarget::Review, and InboxClears::ReviewDecided in org-proto/src/inbox.rs, surfacing assigned open reviews, claimable team/role pool reviews, changes_requested feedback for human authors, and >=5-round escalations with live SSE inbox.changed/org.changed updates.
- [ ] #2 Exposes authenticated /api/v1/organizations/{org}/reviews* routes in apps/agentic-org-web through OfficeBridge with closed operations.json allowlisting and doc-71 updates.
- [ ] #3 Ships capability-enabled Svelte 5 / shadcn-svelte UI in apps/agentic-org-ui/src/lib/modules/reviews/ (/reviews queue, /reviews/[ref] workbench with file tree, unified/split code diff viewer, Markdown/artifact viewer, inline line/section-anchored finding threads, round selector + R1->R2 comparison, stale-pin warning banner, atomic Submit Verdict bar, Inbox review cards, and Task Detail Reviews widget) passing check, lint, test, build, and Playwright E2E across wide and 320 px viewports.
<!-- AC:END -->
