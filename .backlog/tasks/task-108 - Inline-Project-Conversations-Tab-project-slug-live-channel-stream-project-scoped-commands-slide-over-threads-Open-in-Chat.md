---
id: TASK-108
title: >-
  Inline Project Conversations Tab (#<project-slug> live channel stream,
  project-scoped commands, slide-over threads & Open in Chat)
status: To Do
assignee: []
created_date: '2026-11-24T15:40:00Z'
labels:
  - frontend
  - ui
  - chat
  - projects
  - messaging
  - ux
  - docs
milestone: m-0
dependencies:
  - TASK-45
  - TASK-49
documentation:
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
priority: medium
type: feature
ordinal: 123000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Replaces the unwired Project Conversations stub (projects/http-api.ts:447 returning [] and link-only ProjectConversations.svelte) with Principal Designer Option A: a focused inline live #<project-slug> channel stream right inside /projects/[id]/conversations. Keeps users in project context to read messages, inspect threads, and resolve Needs-you Ask/Approval/Handoff cards without pogo-sticking to /chat—while omitting Chat sidebar/search/admin chrome so it stays a calm single-conversation stream rather than a mini-Slack. Requires zero backend schema changes (uses existing auto_scope='project' conversation in doc-90) and accepts channels[] (hiding switcher chrome when length===1) so v2 custom project-linked channels (TASK-109) drop in with zero UI rework.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Extracts the reusable single-conversation controller/pane from ChatWorkspace.svelte (message feed, pending delivery queue, AskCard/QuestionForm/HandoffCard mutations, Composer, CommandDialog, ThreadPanel orchestration) into a shared /modules/messaging feature consumed by both /chat (full-height frame mode) and ProjectConversations.svelte (document-flow embedded mode), preserving shared shell-level provideDraftStore drafts across tab switches and Open in Chat.
- [ ] #2 Renders a compact channel bar below ProjectFrame TabsNav showing #<project-slug>, member count (and Archived badge when archived), an optional amber 'N needs you' jump button when an open Ask/Approval is scrolled off-screen, and one quiet 'Open in Chat' link (/chat?c=<convId>[&thread=<rootId>]) using Heroicons only (no Chat sidebar, no duplicate search bar, no Pins/Members admin panels); hides any channel switcher when only 1 channel is linked.
- [ ] #3 Implements responsive single-scroll layout (no nested fixed-height scroll trap): chronological latest-200 feed with Load older, day/New dividers, sticky bottom-0 Composer on desktop (>=640px & >36rem height) that unsticks to static on narrow/200% zoom viewports (SHELL-30), and ThreadPanel as a fixed right slide-over sheet (max-w-md desktop, full-width <640px, synced to ?thread=<rootId>, closed cleanly by Esc and browser Back per SHELL-28).
- [ ] #4 Passes defaultProjectId: frame.projectId into CommandEnv so /bug, /request, /task, and message 'Turn into…' pre-select the active project automatically; enforces full authorization, office-down read-only callout (CHAT-35), non-member owner read-only notice (CHAT-12), archived project/channel read-only state, and honest zero-leak unauthorized/empty states.
- [ ] #5 Updates doc-49 (§2 visual direction, §3 Flow 1, PRJ-2, §16 noting custom project-linked channels as v2) and doc-40 (Decision 3, §1.3, new CHAT-45 embedded project-channel criterion); passes type check, unit tests, build, and Playwright checks across wide, 320px narrow, and 200% zoom viewports.
<!-- AC:END -->
