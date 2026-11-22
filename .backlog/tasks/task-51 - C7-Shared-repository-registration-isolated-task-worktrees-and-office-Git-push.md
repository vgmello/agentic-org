---
id: TASK-51
title: >-
  C7: Shared repository registration, isolated task worktrees, and office Git
  push
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T04:43:00Z'
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
- [x] #1 Open questions in projects-git-github.md are resolved and recorded before implementation.
- [x] #2 Owners attach repositories and agents work in office-provisioned per-task worktrees with branches pushed by the office.
- [x] #3 repo list, repo attach, worktree list, and repo push pass contract and real-office tests, and FC-103/139/140 are updated.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Implemented C7 plain-Git backend & CLI: resolved all 11 open questions in doc-35; built zero-dep crates/org-git (hardened GitRunner >= 2.35.2, fixed PATH, strict HTTPS/SSH URL validation, symlink-safe canonical bare-cache integrity verification + hostile config/hook/alternates/replace refusal, RepoLocks, isolated task/<key>-<slug> worktrees, non-force commit-OID push forbidding integration/main/master branches, cleanup tags/CAS delete, checkpoint bundles, and FC-103 sandbox rules); added project_repositories and task_repositories projections on Memory+SQLite in org-office; wired GitService in org-office-server, /api/v1 BFF routes in agentic-org-web, and repo/worktree commands in org-agent-cli/agentic-org; updated doc-3, doc-35, and doc-74; passed Pass 1 + Pass 2 fresh Neon reviews (VERDICT: APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
