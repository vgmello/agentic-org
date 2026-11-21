---
id: TASK-48
title: 'C9: Channel question watcher and channel summarize'
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - agents
  - messaging
  - m1
milestone: m-0
dependencies:
  - TASK-46
  - TASK-42
  - TASK-47
documentation:
  - docs/implementation-plan.md
  - docs/adr/0024-a-channel-question-is-watched-for-its-answer.md
  - docs/design/smart-features.md
priority: medium
type: feature
ordinal: 48000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Measure watching-model accuracy on labelled channel excerpts (ADR-0024 decision 19), then build the per-channel tool-less watcher pass via org-copilot, organization watching model profile toggle, product-made usage record, answer_candidate inbox entries, and channel summarize CLI command (FC-170).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 ADR-0024 decision 19 accuracy measurement is recorded before enabling the watcher.
- [ ] #2 Channel questions open bounded watches that feed matched answers to the asker's inbox as answer_candidate without exposing unrelated channel traffic.
- [ ] #3 channel summarize CLI command works and WATCH-3..8, WATCH-10, and WATCH-11 pass.
<!-- AC:END -->
