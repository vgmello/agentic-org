---
id: TASK-12
title: >-
  Flip UI productionImportGuard to 'error' and remove static mock imports from
  production routes
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - ui
  - architecture
  - p1
milestone: m-3
dependencies:
  - TASK-5
references:
  - apps/agentic-org-ui/vite.config.ts
  - apps/agentic-org-ui/src/routes/(shell)/chat/+page.svelte
  - apps/agentic-org-ui/src/routes/(shell)/+layout.svelte
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/project-structure.md
priority: high
type: bug
ordinal: 12000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit finding C-09: Production routes in apps/agentic-org-ui statically import mock engines and reducers (chat/+page.svelte imports createMockChat; (shell)/+layout.svelte imports org/api.ts pulling 39.6 KB reducer.ts; work/api.ts bundles engine.ts) while vite.config.ts leaves productionImportGuard in non-blocking 'report' mode. This violates project-structure.md ('no production mock fallback') and Non-Negotiable #14.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 No production route under apps/agentic-org-ui/src/routes/ statically imports mock factories, mock chat engines, or in-memory mock reducers.
- [ ] #2 Mock factories live strictly behind dev/test-only entry points excluded from production bundles.
- [ ] #3 apps/agentic-org-ui/vite.config.ts enforces productionImportGuard in mode: 'error' so CI fails on any reintroduced mock import.
- [ ] #4 UI type check, unit tests, production build, and relevant Playwright tests pass.
<!-- AC:END -->
