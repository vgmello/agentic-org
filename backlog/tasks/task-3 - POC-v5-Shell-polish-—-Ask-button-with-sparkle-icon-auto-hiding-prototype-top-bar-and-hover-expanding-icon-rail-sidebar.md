---
id: TASK-3
title: >-
  POC v5: Shell polish — Ask button with sparkle icon, auto-hiding prototype top
  bar, and hover-expanding icon-rail sidebar
status: Done
assignee: []
created_date: '2026-11-22T01:26:00Z'
updated_date: '2026-11-22T01:27:00Z'
labels:
  - ui
  - shell
  - poc
dependencies: []
priority: high
type: enhancement
ordinal: 3000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Updated poc/v5.html across all 4 themes: renamed command-bar 'Ask AI' button to 'Ask' with sparkle icon, made the prototype version/theme top bar auto-hide until hovering the top edge, and made the left sidebar default to a 68px icon rail that smoothly expands to 264px on hover while pushing the page content.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Command bar displays sparkle icon + 'Ask' across Ocean, Light, Classic, and Report
- [x] #2 Prototype top bar hides off-screen by default, slides down smoothly on top-edge hover, and re-hides on mouse leave
- [x] #3 Left sidebar idles as a 68px icon rail, expands smoothly to 264px on hover (pushing page and top bar in sync), collapses cleanly on mouse leave, and supports pinning open
<!-- AC:END -->
