---
id: TASK-12
title: >-
  Flip UI productionImportGuard to 'error' and remove static mock imports from
  production routes
status: Done
assignee: []
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T02:02:00Z'
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
- [x] #1 No production route under apps/agentic-org-ui/src/routes/ statically imports mock factories, mock chat engines, or in-memory mock reducers.
- [x] #2 Mock factories live strictly behind dev/test-only entry points excluded from production bundles.
- [x] #3 apps/agentic-org-ui/vite.config.ts enforces productionImportGuard in mode: 'error' so CI fails on any reintroduced mock import.
- [x] #4 UI type check, unit tests, production build, and relevant Playwright tests pass.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Separated mock factories from production API adapters: moved createMockOrg to src/lib/org/mock.ts (preserving http-api.ts gateway wiring in src/lib/org/api.ts), createMockWork to src/lib/work/mock.ts, createMockChat to src/lib/modules/messaging/mock.ts, removed legacy-api.ts, and wired production chatApi and copilotApi to fail-closed unavailable defaults. Added dev/test composition fakes in e2e/fakes/modules/{copilot,messaging}.ts, updated e2e chat dynamic imports to /e2e/fakes/modules/messaging.ts, updated e2e/memory/vite.memory.config.ts and e2e/gate-manifest.json. Retired mixedFactories in production-import-guard.ts while guarding in-memory org/reducer, work/engine, and processes/world, and flipped vite.config.ts productionImportGuard to mode: 'error' with explicit .ts extension.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Production routes and adapters in apps/agentic-org-ui no longer import mock factories, sample data, or in-memory reducers/engines. Mock composition lives exclusively behind dev/test fakes under e2e/fakes/ and */mock.ts. vite.config.ts enforces productionImportGuard({ mode: 'error' }) with zero build leaks. Verified with format:check (0 failures), lint (0 warnings), svelte-check (0 errors/warnings), vitest (142 files / 2438 tests), vite build + check-build-assets, and Playwright (production-unavailable: 6/6; chat + copilot + chat-bugs: 57/57).
<!-- SECTION:FINAL_SUMMARY:END -->
