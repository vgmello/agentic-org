---
id: TASK-32
title: >-
  Cut over repository documentation links from docs/ to .backlog/, retire legacy
  docs/ markdown files, and verify checks
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - docs
  - backlog-migration
  - ci
milestone: m-5
dependencies:
  - TASK-20
  - TASK-21
  - TASK-22
  - TASK-23
  - TASK-24
  - TASK-25
  - TASK-26
  - TASK-27
  - TASK-28
  - TASK-29
  - TASK-30
  - TASK-31
documentation:
  - AGENTS.md
  - CLAUDE.md
priority: medium
type: chore
ordinal: 32000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Once all 127 documentation files under docs/ have been individually reviewed and migrated into .backlog/docs/ and .backlog/decisions/, repository-wide references (AGENTS.md, CLAUDE.md, crate READMEs/doc comments, CI scripts, and Backlog task links) must point to the new canonical .backlog/ locations and legacy markdown copies under docs/ must be retired cleanly without breaking Rust builds or kit hooks.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 AGENTS.md, CLAUDE.md, crate READMEs, and scripts/CI workflows reference the migrated .backlog/docs/ and .backlog/decisions/ locations (and any code-owned SQL assets still embedded via include_str!).
- [ ] #2 Legacy migrated Markdown files under docs/ are removed (leaving no duplicate competing sources of truth).
- [ ] #3 No broken internal links remain across .backlog/docs/, .backlog/decisions/, and AGENTS.md.
- [ ] #4 cargo test --workspace, test_schema.py, and python3 .agent-kit/runtime/kit_hooks.py check pass cleanly.
<!-- AC:END -->
