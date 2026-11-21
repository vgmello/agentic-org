---
id: TASK-68
title: 'UI Step 6: Docs, Features, Releases screens, and Knowledge search'
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
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
- [ ] #1 /projects/[id]/docs, /projects/[id]/docs/[docId], /projects/[id]/features, /projects/[id]/features/[featureId], and /projects/[id]/releases match their UX specs.
- [ ] #2 Knowledge search ('Why is...') works on /search against e2e/fakes/ module mocks.
- [ ] #3 Type check, unit tests, lint, build, and Playwright specs pass.
<!-- AC:END -->
