---
id: TASK-4
title: 'Production Svelte UI: Port Modern 2D Executive Report theme'
status: To Do
assignee: []
created_date: '2026-11-22T01:26:00Z'
updated_date: '2026-11-22T01:38:00Z'
labels:
  - ui
  - themes
  - svelte
dependencies: []
priority: high
type: feature
ordinal: 4000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Port the Modern 2D Executive Report theme from poc/v5.html into apps/agentic-org-ui/src/lib/styles/themes/report.css and register it in themes/index.css and UI_THEMES in ui-preferences.svelte.ts, including Report-only executive section headings.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 New isolated apps/agentic-org-ui/src/lib/styles/themes/report.css scoped strictly to html[data-ui-theme='report']
- [ ] #2 Registered in themes/index.css and UI_THEMES with scheme: 'light' and settings swatch preview
- [ ] #3 Report-only section headings (Decision & Approval Queue, Operations & Activity Brief, Project Delivery Scorecard, Capacity & Bottleneck Analysis) appear only when Report theme is active
- [ ] #4 svelte-check, eslint, vitest, build, and browser visual verification pass
- [ ] #5 Report theme uses high-contrast callout/metric/up-next surfaces, crisp arrow buttons, primary navy '+ New task' CTA, and compact live-agents-first Activity rows
<!-- AC:END -->
