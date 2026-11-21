---
id: TASK-68
title: 'UI Step 6: Docs, Features, Releases screens, and Knowledge search'
status: In Progress
assignee:
  - '@ui-subagent'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T10:11:00Z'
labels:
  - ui
  - docs
  - late-v1
milestone: m-0
dependencies:
  - TASK-66
documentation:
  - docs/plans/v1-ui-screens.md
  - docs/specs/features-ux.md
  - docs/specs/decisions-and-tech-debt-ux.md
priority: medium
type: feature
ordinal: 68000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement Step 6 of v1-ui-screens.md: project Docs tab by kind/section with reader, editor, history, diff compare, version issuing, and New document menu; Features tab and feature page (New, Retire, Break into tasks); Releases tab and release page; and 'Why is...' knowledge search in /search.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 /projects/[id]/docs, /projects/[id]/docs/[docId], /projects/[id]/features, /projects/[id]/features/[featureId], and /projects/[id]/releases match their UX specs.
- [x] #2 Knowledge search ('Why is...') works on /search against e2e/fakes/ module mocks.
- [x] #3 Type check, unit tests, lint, build, and Playwright specs pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Build src/lib/modules/docs (types, unavailable production api, pure present/markdown/diff/frontmatter helpers + unit tests, DocsTab by kind and section + New menu, DocumentPage reader/editor/history/compare/issue version) and e2e/fakes/modules/docs.ts + JSON data.
2. Build src/lib/modules/features (types, unavailable production api, pure present/progress/retirement helpers + unit tests, FeaturesTab grouped by lifecycle, FeaturePage with New, Retire, Break into tasks, task link/unlink, affected bugs, supporting docs, release backlinks, cost/history) and e2e/fakes/modules/features.ts + JSON data.
3. Build src/lib/modules/releases (types, unavailable production api, pure present helpers + unit tests, ReleasesTab and ReleasePage with milestones, explicit features/tasks, product/technical changelogs, intended flags vs confirmed deployment attempts) and e2e/fakes/modules/releases.ts + JSON data.
4. Upgrade src/lib/modules/search and /search for 'Why is...' knowledge search (permitted docs + features, scope, status, excerpts, freshness/coverage notice, exclusions) backed by e2e/fakes/modules/search.ts.
5. Enable Features, Releases, Docs tabs in projects SERVED, wire thin SvelteKit routes under /projects/[id]/{docs,features,releases}, update unit/e2e tests and e2e/gate-manifest.json within journey budget.
6. Run svelte-autofixer on all created/edited Svelte files, format:check, lint, check, test, build, test:e2e:verify, and targeted Playwright chromium + production specs.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implemented UI Step 6 on branch feat/task-68-ui-step6-docs-features-releases: src/lib/modules/{docs,features,releases,search} with unavailableCall production adapters and e2e/fakes/ mocks; routes /projects/[id]/docs(/new,/[docId]), /projects/[id]/features(/[featureId]), /projects/[id]/releases(/[releaseId]), and /search ('Why is...' knowledge search). Verified: svelte-autofixer 5.57.3 (0 issues/suggestions across all 23 Svelte files), format:check, eslint, svelte-check (0 errors/warnings), vitest (156 files), vite build (productionImportGuard), test:e2e:verify (57/60 journeys), Playwright chromium + production specs (docs-features-releases 6/6, projects 18/18, production-unavailable 7/7). Left In Progress for lead review & merge.
<!-- SECTION:NOTES:END -->
