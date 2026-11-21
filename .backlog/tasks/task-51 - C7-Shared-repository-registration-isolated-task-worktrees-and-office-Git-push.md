---
id: TASK-51
title: >-
  C7: Shared repository registration, isolated task worktrees, and office Git
  push
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - backend
  - rust
  - git
  - agents
  - m1
milestone: m-0
dependencies:
  - TASK-42
  - TASK-49
documentation:
  - docs/implementation-plan.md
  - docs/design/projects-git-github.md
  - docs/design/agent-os-command-line.md
priority: high
type: feature
ordinal: 51000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Resolve open design questions in projects-git-github.md, then build organization repository registration, project integration branches, office-managed bare Git caches, isolated per-task worktrees, office-credential push over plain remotes, and repo list|attach|push plus worktree list CLI commands (FC-103, FC-139, FC-140).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Open questions in projects-git-github.md are resolved and recorded before implementation.
- [ ] #2 Owners attach repositories and agents work in office-provisioned per-task worktrees with branches pushed by the office.
- [ ] #3 repo list, repo attach, worktree list, and repo push pass contract and real-office tests, and FC-103/139/140 are updated.
<!-- AC:END -->
