---
id: TASK-3
title: >-
  Move task comments and high-churn operational entries out of the 64 KiB task
  YAML front-matter cap
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - architecture
  - data-model
  - p0
milestone: m-2
dependencies: []
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/03-features-and-work.md
  - docs/design/organization-data/05-operational-data.md
priority: high
type: enhancement
ordinal: 3000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit finding C-04: All task comments (full Markdown bodies), checklists, links, watchers, owner questions, and waiting spans are currently specified inside a single task file's YAML front matter capped at <=64 KiB and <=10,000 parser events. Active tasks with ~30-50 comments hit the hard cap and become unwritable, while every comment or checklist update rewrites the entire task file and creates a new work-history revision file, risking Git file-count limits and latency degradation before B4/B5 implementation.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Task comments (and other unbounded high-churn operational entries) are specified as append-only sibling files under each task's directory rather than embedded in task.md YAML front matter.
- [ ] #2 task.md remains compact and bounded well below the 64 KiB / 10,000-event cap regardless of comment volume.
- [ ] #3 Adding a comment or ticking a checklist item no longer rewrites the entire task file or amplifies work-history revision snapshots.
- [ ] #4 Docs 03 and 05, schema.sql (if affected), and test_schema.py reflect the updated canonical storage layout with zero doc-to-SQL drift.
<!-- AC:END -->
