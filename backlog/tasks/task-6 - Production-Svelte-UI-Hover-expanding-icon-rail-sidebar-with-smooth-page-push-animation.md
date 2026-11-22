---
id: TASK-6
title: >-
  Production Svelte UI: Hover-expanding icon-rail sidebar with smooth page-push
  animation
status: To Do
assignee: []
created_date: '2026-11-22T01:26:00Z'
labels:
  - ui
  - shell
  - svelte
dependencies: []
priority: high
type: enhancement
ordinal: 6000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Port the hover-expanding desktop icon-rail sidebar (68px idle rail <-> 264px full sidebar, smoothly pushing page content with cubic-bezier animation and optional pin toggle) into the production Svelte shell across all themes.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 On desktop viewports, sidebar defaults to collapsed 68px icon rail and smoothly expands to 264px on hover while animating page padding in sync
- [ ] #2 Moving mouse away smoothly collapses back to 68px without vertical jump; respects open menus, keyboard focus, prefers-reduced-motion, and pin toggle
- [ ] #3 Works cleanly across Ocean, Light, Classic, and Report themes with passing svelte-check, eslint, and Playwright checks
<!-- AC:END -->
