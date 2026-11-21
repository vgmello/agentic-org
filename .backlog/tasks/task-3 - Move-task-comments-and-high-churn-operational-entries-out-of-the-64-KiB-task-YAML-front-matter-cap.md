---
id: TASK-3
title: >-
  Move task comments and high-churn operational entries out of the 64 KiB task
  YAML front-matter cap
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:32:00Z'
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
- [x] #1 Task comments (and other unbounded high-churn operational entries) are specified as append-only sibling files under each task's directory rather than embedded in task.md YAML front matter.
- [x] #2 task.md remains compact and bounded well below the 64 KiB / 10,000-event cap regardless of comment volume.
- [x] #3 Adding a comment or ticking a checklist item no longer rewrites the entire task file or amplifies work-history revision snapshots.
- [x] #4 Docs 03 and 05, schema.sql (if affected), and test_schema.py reflect the updated canonical storage layout with zero doc-to-SQL drift.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Update Doc 03 (03-features-and-work.md) and Doc 05 (05-operational-data.md) so tasks use a task directory (`work/tasks/<task-id>/task.md`) with append-only sibling comment files (`comments/<comment-id>.md`) and dedicated operational sibling files (`checklist.yaml`, `operations.yaml`), keeping `task.md` compact and bounded well below the 64 KiB / 10,000-event YAML front-matter cap.
2. Update task-activity history wording in Doc 05 so comment appends and checklist/watcher updates do not rewrite `task.md` or duplicate full task snapshots.
3. Update directory-tree path examples in Docs 01/02/03/05 where `work/tasks/<task-id>.md` appears so all docs agree.
4. Verify `python3 -B docs/design/organization-data/test_schema.py` passes (including relative link & SQL/Mermaid checks), finalize TASK-3, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Updated Docs 01, 02, 03, and 05 so tasks use a task directory (work/tasks/<task-id>/task.md) with append-only sibling comment files (comments/<comment-id>.md) and dedicated checklist.yaml and operations.yaml sibling files. Kept task.md compact and bounded well below the 64 KiB / 10,000-event YAML front-matter cap, and updated Doc 05 task activity history so comment/checklist/watcher writes record compact delta events without rewriting task.md. Verified test_schema.py (112/112 PASS).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Moved unbounded task comments and high-churn operational entries (checklist, watchers, owner questions, waiting spans) out of the 64 KiB task.md YAML front-matter cap into sibling files under work/tasks/<task-id>/.
<!-- SECTION:FINAL_SUMMARY:END -->
