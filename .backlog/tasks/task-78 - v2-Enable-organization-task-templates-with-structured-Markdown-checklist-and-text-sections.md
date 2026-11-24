---
id: TASK-78
title: >-
  v2: Enable organization task templates with structured Markdown checklist and
  text sections
status: To Do
assignee: []
created_date: '2026-11-21T22:21:00Z'
updated_date: '2026-11-23T03:22:00Z'
labels:
  - v2
  - work
  - templates
milestone: m-1
dependencies:
  - TASK-87
  - TASK-88
priority: low
type: feature
ordinal: 78000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Phase 2 (deferred to v2) — Organization-level task templates:

- Support organization-defined task templates selected by task type.
- Support two structured template section primitives embedded inside task Markdown files: `checklist` sections (e.g. acceptance criteria checklists governed by organization processes) and `text` / Markdown sections (e.g. description, implementation plan, notes).
- Delimit each template section in task Markdown using explicit begin/end comment markers/tags (for example, `<!-- section:description:begin --> ... <!-- section:description:end -->`, `<!-- section:implementation_plan:begin --> ... <!-- section:implementation_plan:end -->`, `<!-- section:notes:begin --> ... <!-- section:notes:end -->`, and `<!-- checklist:<name>:begin --> ... <!-- checklist:<name>:end -->`).
- Enable the UI to recognize template-defined sections when tasks are created and viewed so sections and checklists render natively in structured UI views.
- Instruct agents (via organization processes and task tooling) to populate and maintain template-defined sections cleanly.
- Note: Task labels, dependencies, reference_files, modified_files, and task ordering are NOT part of templates; they remain template-independent first-class primitives delivered in v1.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Organization task templates can be defined per task type with structured checklist and text/Markdown sections.
- [ ] #2 Task Markdown parser/writer preserves and validates structured begin/end comment markers for template sections and checklists.
- [ ] #3 UI identifies and renders template-defined sections and checklists on task creation and detail views, and agent instructions/CLI honor template sections.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Note (2026-11-23): v1 task storage reconciliation (TASK-87) and unified checklist_steps.section (TASK-88: acceptance/dod/work in sibling checklist.yaml) lay the foundation for TASK-78; v2 template checklist sections project onto checklist_steps.section = <name> rather than embedding checklists inside task.md.
<!-- SECTION:NOTES:END -->
