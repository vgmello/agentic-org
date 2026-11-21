---
id: TASK-74
title: >-
  v2: Hosting-provider Git integrations (GitHub/GitLab app, PRs) and
  release/failover automation
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
labels:
  - v2
  - git
  - automation
milestone: m-1
dependencies: []
documentation:
  - docs/design/projects-git-github.md
  - docs/specs/features-ux.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
priority: low
type: feature
ordinal: 74000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Deferred v2 automation and hosting integrations: GitHub/GitLab provider apps with short-lived tokens, repository creation, and pull requests (FC-163), automatic feature transitions and release automation (FC-071), and automatic HQ failover promotion and backup automation (FC-149).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Hosting-provider app authentication, short-lived tokens, repo creation, and PR workflows pass verification (FC-163).
- [ ] #2 Selected release/feature automation (FC-071) and automatic promotion/backup engines (FC-149) pass acceptance tests.
<!-- AC:END -->
