---
id: TASK-80.5
title: >-
  Tess 3/5 (v1): Embedded Tess Side Panel UI (⌘K fast launcher + persistent
  right-hand drawer, context pill, calm tool telemetry, Action Proposal Cards &
  badged offline fallback)
status: To Do
assignee: []
created_date: '2026-11-25T00:01:00Z'
labels:
  - ui
  - ux
  - tess
  - chief-of-staff
  - shell
  - home
  - a11y
milestone: m-0
dependencies:
  - TASK-80.4
documentation:
  - .backlog/docs/specs/doc-39 - Spec-App-Shell-and-Navigation-UX.md
  - apps/agentic-org-ui/src/lib/platform/shell/ExecutiveCommandBar.svelte
  - apps/agentic-org-ui/src/lib/platform/shell/shell-ai.ts
  - apps/agentic-org-ui/src/lib/modules/home/features/ExecutiveBriefCard.svelte
  - apps/agentic-org-ui/src/lib/modules/home/features/DecisionSpotlight.svelte
  - apps/agentic-org-ui/src/lib/home/model.ts
parent_task_id: TASK-80
priority: high
type: feature
ordinal: 128000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers the native embedded Tess experience inside Agentic Org UI so interacting with Tess feels like the application itself is deeply intelligent rather than a raw claude -p terminal dump. Splits ⌘K (instant Search & Commands with clean handoff) from a persistent, collapsible right-hand Tess Side Panel (⌘/ and ✨ Ask) that docks beside <main> on wide desktop (preserving route navigation and working memory) and adapts to a modal Sheet on compact/mobile viewports, rendering ambient route context pills, calm collapsible tool-progress summaries, clickable entity mini-rows, server-validated Action Proposal / Confirmation Cards with 'Edit in form' escape hatch and immutable server receipts, smart /chat de-duplication, and an explicitly badged offline quick-summary fallback.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 TESS-UX-1..3 Hybrid Surface & Adaptive Reflow: Simplifies ExecutiveCommandBar ⌘K to Search & Commands only (removes 2-tab modal switcher, placeholder 'Search, run a command, or ask Tess…', hands off 'Ask Tess about …', ✨ Ask, ⌘/, and DecisionSpotlight '✨ Ask about this' into new modules/tess/ TessPanel.svelte mounted in (shell)/+layout.svelte). Docks non-modally beside <main> (360-420px, <main> >=720px) on wide desktop (>=1440px or >=1280px collapsed rail) persisting across route navigation; switches to modal right-hand Sheet below docking width and full-width 100dvh Sheet <640px with registerModalBack, 320px & 200% zoom reflow, and zero horizontal overflow.
- [ ] #2 TESS-UX-4..5 Smart /chat De-duplication & Ambient Context Pill: Pressing ⌘/ while already viewing Tess's DM in /chat focuses the DM composer instead of opening a duplicate side panel; on other routes/channels opens Tess scoped to that view. Renders a dismissible Ambient Context Pill above TessComposer reflecting the active route/entity or clicked DecisionSpotlight item, passes structured context_ref with the prompt, and allows 1-click detach (44px touch target on narrow/coarse).
- [ ] #3 TESS-UX-6..7 Calm Tool Telemetry, Stop & Rich Native Artifacts: Translates SSE tool.progress events into plain product language ('Checking projects…', 'Still checking…' after 2s) with an in-place Stop button (POST /tess/turns/{id}/cancel) and collapses settled calls into a single progressive-disclosure line ('Checked 4 projects · 1.4s ▾', zero raw CLI stdout/JSON by default). Renders pure streaming Markdown + canonical entity ref auto-links + TessEntityList clickable mini-rows (navigating <main> while keeping docked panel open) + up to 3 contextual follow-up chips on the latest turn only.
- [ ] #4 TESS-UX-8 Action Proposal / Confirmation Cards: Renders server-validated ui.block action_proposal events via TessActionProposalCard.svelte showing structured dry-run parameters, risk/reversibility badge (icon + word), consequence sentence, verb-object primary CTA (aria-busy during POST /tess/proposals/{id}/apply), 'Edit in form' pre-filled escape hatch, and 'Cancel'. Zero state mutates until explicit user click + server confirmation, after which the card becomes an immutable receipt with 'Open <entity> →' link.
- [ ] #5 TESS-UX-9..12 Coherent Identity, Badged Offline Fallback, 6-Theme & A11y: Keeps Home ExecutiveBriefCard strictly read-only (zero right-side action buttons); adds thread switcher + 'Open in Chat' escalation in TessHeader; displays honest runtime badges ('Ready' / 'No runtime' / 'Offline') and badges local answerExecutiveQuestion() fallback clearly as 'Offline quick summary · Local data only' while replacing its misleading unmatched fallthrough with an honest capability message + manual shortcut. Passes WCAG 2.2 AA across all 6 themes (ocean/classic/graphite/cozy/report/report-dark), prefers-reduced-motion, milestone-only .sr-only aria-live='polite' announcements, role='alert' inline errors without duplicate toasts, [data-tess-panel] single-key shortcut guard, Vitest + Playwright E2E, and doc-39 §6a update.
<!-- AC:END -->
