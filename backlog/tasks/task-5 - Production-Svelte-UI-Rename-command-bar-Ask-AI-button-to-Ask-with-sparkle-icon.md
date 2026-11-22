---
id: TASK-5
title: >-
  Production Svelte UI: Rename command bar to 'Ask' + 'Ask Tess' (Chief of
  Staff)
status: To Do
assignee: []
created_date: '2026-11-22T01:26:00Z'
updated_date: '2026-11-22T02:25:00Z'
labels:
  - ui
  - shell
  - svelte
dependencies: []
priority: medium
type: enhancement
ordinal: 5000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Update ExecutiveCommandBar.svelte so the action button displays sparkle icon + 'Ask' and references the built-in AI Chief of Staff Tess (Tactical Execution & Squad Supervisor) as 'Ask Tess' across placeholder, modal tab, group headings, and aria labels.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 ExecutiveCommandBar.svelte renders sparkle icon + 'Ask' label across all themes
- [ ] #2 All Svelte unit tests, e2e specs, and ui-gate checks pass
- [ ] #3 Command bar placeholder, modal tab, and headings say 'Ask Org' (button says sparkle + 'Ask')
<!-- AC:END -->
