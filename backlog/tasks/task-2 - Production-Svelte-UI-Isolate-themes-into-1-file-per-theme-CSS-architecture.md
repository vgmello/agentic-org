---
id: TASK-2
title: 'Production Svelte UI: Isolate themes into 1-file-per-theme CSS architecture'
status: Done
assignee: []
created_date: '2026-11-22T01:26:00Z'
updated_date: '2026-11-22T01:27:00Z'
labels:
  - ui
  - themes
  - svelte
dependencies: []
priority: high
type: enhancement
ordinal: 2000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Refactored monolithic themes.css into isolated files under apps/agentic-org-ui/src/lib/styles/themes/ (base.css, ocean.css, light.css, classic.css, index.css) and derived theme lists/schemes from UI_THEMES in ui-preferences.svelte.ts.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Each theme lives in its own strictly scoped CSS file under src/lib/styles/themes/
- [x] #2 Adding a new theme requires only a CSS file, an import in themes/index.css, and one UI_THEMES entry
- [x] #3 svelte-check, eslint, vitest, vite build, and ui-gate verify pass
<!-- AC:END -->
