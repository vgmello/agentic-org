---
id: TASK-107
title: >-
  Add consistent accessible tooltips across icon controls and task cards, detail
  views, and task side panels
status: To Do
assignee: []
created_date: '2026-11-24T05:15:00Z'
labels:
  - ui
  - frontend
  - ux
  - accessibility
  - work
  - tasks
  - docs
milestone: m-0
dependencies:
  - TASK-49
documentation:
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
priority: medium
type: enhancement
ordinal: 122000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Today Bits UI Tooltip and the --color-tooltip (bg-tooltip) token are used only in Sidebar.svelte and ShellAccountMenu.svelte; there is no shared UI tooltip primitive or shell-level Tooltip.Provider, leaving icon-only buttons and compact status/type/priority/checklist/budget icons across Work cards, lists, task detail pages, task side panels/sheets, and other panels without visual hover/focus explanations. Establishes one shared Tooltip primitive and adds clear, accessible tooltips to icon controls across the app—especially throughout Tasks and the task side panel.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Adds a reusable Tooltip wrapper under apps/agentic-org-ui/src/lib/components/ui/tooltip/ built on bits-ui Tooltip using semantic look tokens (bg-tooltip, text-fg, outline-line, z-[100], keyboard focus-visible + hover open, Esc dismiss, prefers-reduced-motion compliant) and mounts a shared <Tooltip.Provider> at apps/agentic-org-ui/src/routes/(shell)/+layout.svelte without regressing Sidebar.svelte's scroll-hold pointer guard.
- [ ] #2 Applies tooltips consistently across Work items (WorkBoard.svelte, WorkList.svelte, WorkQueueView.svelte, WorkDetailPage.svelte, WorkSideColumn.svelte, ChecklistSection.svelte, BudgetChip.svelte, and task side panel/sheet drawers): work item type icons (Task/Bug/Request), priority and severity icons, card Actions (⋯) menu trigger, checklist reorder (↑/↓) and remove icon buttons, required-step lock icon, reopen icon, budget cap/hold chips, and side-panel header/action icons (close, open full page, copy ref, edit/actions).
- [ ] #3 Audits and adds tooltips to remaining icon-only buttons across shell, Chat/Thread/Pins/Members panels, Activity, and Inbox drawers, enforcing UX rules: every icon button keeps its explicit aria-label, tooltips show concise action/meaning (plus shortcut hint where applicable), never hide critical text required on touch (< 640px) devices, and never rely solely on native browser title attributes.
- [ ] #4 Updates doc-39 (§6a shared tooltip rule) and doc-49 (task board/list/side-panel icon tooltips), and passes type check, unit tests, look-token checks, and Playwright hover + keyboard-focus tooltip checks.
<!-- AC:END -->
