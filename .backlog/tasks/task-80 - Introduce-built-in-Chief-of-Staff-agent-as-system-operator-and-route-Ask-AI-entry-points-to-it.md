---
id: TASK-80
title: >-
  Introduce built-in Chief of Staff agent as system operator and route Ask AI
  entry points to it
status: To Do
assignee: []
created_date: '2026-11-22T02:08:00Z'
updated_date: '2026-11-22T02:17:00Z'
labels:
  - backend
  - ui
  - agents
  - built-in
milestone: m-0
dependencies:
  - TASK-42
  - TASK-45
  - TASK-49
documentation:
  - >-
    .backlog/docs/organization-data/doc-78 -
    Organization-Data-04-—-Agent-Structure.md
  - >-
    .backlog/docs/organization-data/doc-75 -
    Organization-Data-01-—-Organization-Structure.md
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/design/doc-36 - Design-Smart-Features-Not-in-v1.md
priority: medium
type: feature
ordinal: 80000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Today the only built-in organization agent is HR (builtin_key: hr), while product-wide Ask AI / Ask interactions have no single persistent agent owner responsible for operating the system. Introducing a built-in Chief of Staff agent gives every organization a dedicated, always-addressable operator agent that backs every Ask AI / Ask interaction in the UI. Crucially, the Chief of Staff is the sole agent in the organization permitted to orchestrate across everything and everyone — coordinating across all teams, projects, agents, people, and system workflows (work items, delegation, status, HR handoffs) — while all other agents remain bounded to their team, reporting, and project scopes. Conversations and usage are recorded under its stable identity like any other agent, and office-enforced caller authorization and confirmation boundaries remain intact.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Organization creation provisions a built-in Chief of Staff agent (unique builtin_key per organization alongside hr) in the top-level team with product-shipped system-operator instructions, addressable handle, and on-demand agent turn execution; existing organizations without one can be migrated/bootstrapped safely.
- [ ] #2 Organization Data contracts and SQL schema (doc-78, doc-75, schema.sql) support the Chief of Staff builtin_key with uniqueness per organization and preserve office-enforced authorization rules so Chief of Staff actions never bypass the requesting person authority or skip required confirmations for structural/destructive changes.
- [ ] #3 Every Ask AI / Ask entry point in the UI routes the user conversation to the built-in Chief of Staff agent (attaching current view/entity context when triggered from a specific screen), persists messages in normal Chat/conversation history, and shows an explicit unavailable state when no runtime/provider is configured.
- [ ] #4 Chief of Staff usage and turns are recorded under its stable employee identity in the normal accounting and activity streams, and automated backend + UI tests cover provisioning, uniqueness, Ask AI routing with context, authorization enforcement, and unavailable-runtime handling.
- [ ] #5 Chief of Staff is enforced as the sole organization-wide orchestrator agent: it is the only agent authorized to orchestrate and coordinate across all teams, projects, agents, and people in the organization, while non-Chief-of-Staff agents remain restricted to their normal team/reporting/project boundaries.
<!-- AC:END -->
