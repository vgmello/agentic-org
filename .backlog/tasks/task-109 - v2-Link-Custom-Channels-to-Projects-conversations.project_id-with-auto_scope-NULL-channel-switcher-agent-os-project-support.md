---
id: TASK-109
title: >-
  v2: Link Custom Channels to Projects (conversations.project_id with auto_scope
  NULL, channel switcher & agent-os --project support)
status: To Do
assignee: []
created_date: '2026-11-24T15:40:00Z'
labels:
  - v2
  - backend
  - rust
  - frontend
  - ui
  - cli
  - chat
  - projects
  - messaging
  - docs
milestone: m-1
dependencies:
  - TASK-108
  - TASK-85.3
documentation:
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
  - .backlog/docs/specs/doc-40 - Spec-Chat-Channels-and-Structured-Asks-UX.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
priority: low
type: feature
ordinal: 124000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Extends Project Conversations in v2 so teams can create or link additional custom channels (e.g., #agency-v1-design, #agency-v1-releases) to a project using the existing conversations.project_id allowance (auto_scope IS NULL) in messaging-schema.sql:47-52, activating the multi-channel pill/dropdown switcher in ProjectConversations.svelte.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Defines and implements custom project-linked channel lifecycle rules in org-office (explicit manual vs synced membership semantics, project archival behavior, link/unlink authorization, and chat.channel_create / update project_id wiring in crates/org-office/src/messaging/features.rs) across Memory and SQLite engines.
- [ ] #2 Activates multi-channel switching in ProjectConversations.svelte (inline pill strip for 2–4 channels with #<project-slug> pinned first, unread counts & needs-you indicator; named modal radio selector for >=5 channels or <640px, URL-synced via ?c=<convId>) and adds optional Project linking in ChannelDialog.svelte.
- [ ] #3 Adds agent-os channel list --project <ref> and optional --project on channel creation/linking within CATALOGUE_LIMIT (TASK-85.3), updating doc-28, doc-40, doc-49, and doc-90 with automated backend, CLI, and UI tests.
<!-- AC:END -->
