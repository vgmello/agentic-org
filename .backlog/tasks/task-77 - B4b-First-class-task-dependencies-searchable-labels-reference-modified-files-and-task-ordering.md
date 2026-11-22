---
id: TASK-77
title: >-
  B4b: First-class task dependencies, searchable labels, reference/modified
  files, and task ordering
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T22:21:00Z'
updated_date: '2026-11-22T02:56:00Z'
labels:
  - backend
  - rust
  - work
  - m1
milestone: m-0
dependencies:
  - TASK-49
priority: high
type: feature
ordinal: 77000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Add template-independent first-class task primitives to the v1 work data contract, org-store (Memory + SQLite), RPC/BFF API, and agent-os/agentic-org CLI (designed alongside TASK-49 B4/B5):

1. Task dependencies hierarchy (`depends_on` / `dependents`): first-class DAG dependency links between tasks with target validation, self-dependency and cycle rejection, and queries for both upstream dependencies and downstream dependents/hierarchy.
2. First-class `reference_files`: explicit list of reference file paths attached to a task, stored and validated independently from modified files.
3. First-class `modified_files`: explicit list of file paths modified/touched by a task, distinct from `reference_files`.
4. First-class searchable and filterable `labels`: multi-value normalized labels per task (e.g., `backend`, `security`), properly indexed in org-store so tasks can be efficiently searched and filtered by single or multiple labels (AND/OR combinations).
5. First-class task ordering (`order` / rank index): explicit numeric order/rank index on tasks (independent of templates) so people and agents can order tasks top-to-bottom for execution (lifting basic ordering out of v2 TASK-73 while keeping full planning lifecycle, Following/Requested-by, and subtask cascade execution in v2 TASK-73).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Task dependencies (depends_on / dependents) are first-class on org-store (Memory + SQLite), reject self-loops and cycles, and expose hierarchy/dependency queries via API and CLI.
- [x] #2 Tasks persist and expose distinct first-class reference_files and modified_files lists through org-store, API, and CLI.
- [x] #3 Tasks support multiple first-class labels with indexed search and multi-label filtering (e.g. backend + security) across Memory and SQLite, API, and CLI.
- [x] #4 Tasks support an explicit first-class order/rank index for prioritized execution ordering via org-store, API, and CLI.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Implemented all 5 template-independent first-class v1 task primitives in commit c8a11ca9: DAG dependencies (depends_on/dependents with self-loop & transitive cycle rejection and hierarchy queries), distinct reference_files and modified_files with traversal rejection, indexed multi-label AND/OR search and label catalog, and explicit rank_order sorting across schema.sql, Memory+SQLite org-store (with rebuild parity), 20 tasks.* RPCs, /api/v1 BFF routes, and agent-os/agentic-org CLIs. Approved by Pass 4 fresh Neon Rust final review (8d65b26b).
<!-- SECTION:FINAL_SUMMARY:END -->
