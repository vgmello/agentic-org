---
id: TASK-92.3
title: >-
  Reviews 3/3: Issued-doc reviews, advisory process/skill context, cross-office
  bundles, Tess & Unified Search integration
status: To Do
assignee: []
created_date: '2026-11-23T03:40:00Z'
labels:
  - backend
  - reviews
  - processes
  - search
  - tess
milestone: m-0
dependencies:
  - TASK-92.1
  - TASK-56
  - TASK-52
  - TASK-80
  - TASK-91
documentation:
  - >-
    .backlog/docs/design/doc-32 -
    Design-Development-Processes-Process-Packages.md
  - >-
    .backlog/docs/organization-data/doc-86 -
    Organization-Data-18-—-Development-Processes-and-Project-Documentation.md
  - .backlog/docs/specs/doc-52 - Spec-Knowledge-Decisions-and-Tech-Debt-UX.md
parent_task_id: TASK-92
priority: medium
type: feature
ordinal: 101000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Connects Universal Reviews deeply to issued document versions (TASK-56), advisory development-process review skills/criteria (without violating ADR-0026), cross-office unpushed code review bundles (TASK-52), Unified Search FTS5 indexing (TASK-91), and Tess Executive Brief visibility (TASK-80.2).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 target_kind="document" pins immutable document_versions (doc@version + body_sha256), computes doc.diff across versions/rounds, and flags stale pins when newer versions are issued.
- [ ] #2 review.request attaches applicable effective org standards (TASK-44) and project development_processes skills/roles (TASK-56) as advisory guidance_refs and suggested criteria without adding a methodology workflow engine (ADR-0026), and ships an on-demand review-execution built-in skill via agent-os skill read.
- [ ] #3 Cross-office unpushed code reviews resolve commits via org-git checkpoint bundles over TASK-52 relay (respecting single-writer review ownership), TASK-91 SearchRead indexes reviews and findings in office.db FTS5, and Tess (TASK-80.2) surfaces pending human reviews, stale approvals, and >=5-round review loops in her Executive Brief; updates doc-52 and doc-3.
<!-- AC:END -->
