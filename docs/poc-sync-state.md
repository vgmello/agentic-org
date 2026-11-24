# POC -> Svelte UI Sync Tracker

Tracks the exact `poc/` commit watermarks applied to the production SvelteKit UI (`apps/agentic-org-ui/`).

## How each sync pass works

1. Fetch the prototype branch with `git fetch origin poc/ui-themes-prototype`. Find owner-approved release markers in branch history:
   ```sh
   git log --reverse --format='%H%x09%s' origin/poc/ui-themes-prototype |
     awk -F '\t' '$2 ~ /^Prototype Release: /'
   ```
   A release marker can be an empty commit, so search without a path filter.
   Process only markers that have yet to be applied. If there are none, wait for the next approval.
2. Read the marker body with `git show -s --format=%B <release_sha>`. Use its approved behavior and pages to select changes, comparing the relevant watermark to the marker with `git diff <last_synced_sha> <release_sha> -- poc/`. If the marker covers a subset, port only the approved scope. Prototype changes after the marker wait for their own approval.
3. Port the approved behavior into `apps/agentic-org-ui/` using existing Svelte components and real product data. Keep prototype fixtures in `poc/`. Verify WCAG AA contrast, unit tests, E2E checks (`--workers=1`), and side-by-side POC vs Svelte screenshots.
4. Complete Codex `gpt-6.2-sol` review and a fresh final sign-off for the Svelte changes, then commit and push. Record the release marker SHA, applied prototype range, and resulting Svelte commit in the history below. Advance the watermarks for the areas actually ported. The existing entries are historical.

## Current Watermarks by Area

| Area | Last Synced POC Commit | Latest Available POC Commit | Status | Notes |
| --- | --- | --- | --- | --- |
| **Themes & Global Shell** (`ocean`, `classic`, `graphite`, `cozy`, `report`, `report-dark`, topbar/search, sidebar) | `26a49c02` *(Codex APPROVE @ `1a4557f7`)* | `2c58edcd` | Synced through `26a49c02` | Synced neutral Graphite, Graphite interior sidebar hierarchy (`13abb1f1`), sidebar pin/controls (`b73964da`), dark CmdK contrast (`ffe0e0ce`), CmdK `#` channel jump with real Chat lookup & `#` Chat shortcut (`c64b187a`), Report Day/Night masthead simplification (`355e3f85`), Cozy Glass Work/Activity/Chat + HC & `prefers-reduced-transparency` (`f1db7133`, `580b6715`, `c64b187a`). Newer prototype commits (`47d62a5b`..`2c58edcd`) refine sidebar lighting/shadows & dark headers. |
| **Index (`/home`)** | `355e3f85` *(Codex APPROVE @ `1a4557f7`)* | `2c58edcd` | Synced through `355e3f85` | Exact 1:1 parity through `355e3f85` (inline Home search combobox, KPI pillars + overdue escalation, Spotlight `Ask about this` + `spot-jump` + 36/44px targets, Executive Brief footer/expand, Projects, Upcoming milestones `0..14d`, Capacity breakdown, simplified Report Day/Night masthead). Newer prototype commit `1631cf9d` updates Home sidebar lighting. |
| **Inbox (`/inbox`)** | `ffe0e0ce` *(Codex APPROVE @ `52dcb756`)* | `2c58edcd` | Synced through `ffe0e0ce` | Exact 1:1 parity through `ffe0e0ce`: 3 primary categories + `More` dropdown (`cd5128ae`), compact `Filter inbox` & `Inbox display` (`Priority first`/`By date` + persisted `Row dividers`, `b73964da`), centered header + search (`355e3f85`, `5a9f9a25`, `f96b81b4`), Graphite matte surfaces & 3-column queue grid (`1f5f7149`, `215be52a`, `e75d920a`), Report row tints (`4622ad46`), simplified sender + WCAG 1.4.13 hover/focus preview (`915bf1e5`, `e8eecf7e`, `7449753c`), persistent Inbox comments + Request history timeline (`f9fd24ba`). |
| **Work (`/work`)** | `26a49c02` *(Codex APPROVE @ `1a4557f7`)* | `2c58edcd` | Synced through `26a49c02` | Exact 1:1 parity through `26a49c02`: Inbox-aligned header/search/toolbar, `List`/`Board` toggle, race-safe URL-synced `Filter work` & `Work display`, inline split reading panel (`WorkReadingPanel.svelte` + inline `TaskSheet`/`BugSheet`/`RequestSheet`, `f1db7133`, `0e3ec0ed`, `26a49c02`), mobile `Back to Work`, 3-way conflict field sync, focus recovery, Graphite/Cozy/Report surfaces & forced-colors. Newer prototype commits (`fc297429`, `2e3974ba`, `167f6471`) add priority bars on all tickets & L->R detail reveal. |
| **Activity (`/activity`)** | `580b6715` *(Codex APPROVE @ `1a4557f7`)* | `2c58edcd` | Synced through `580b6715` | Chronological day-grouped Activity timeline (`Today`/`Yesterday`/dates, DST-safe UTC calendar buckets, `assembly-model.ts`, `ActivityStreamPanel.svelte`, `ActivityCommandCenter.svelte`, `ActivityOpsRail.svelte`) + Cozy frosted surfaces (`580b6715`). Newer prototype commit `19c88bd5` aligns Activity controls. |
| **Chat (`/chat`)** | `1ba00168` *(Codex APPROVE @ `1a4557f7`)* | `2c58edcd` | Synced through `1ba00168` | Cozy Chat frosted chrome & matte message canvas (`c64b187a`), keyboard channel navigation (`sidebar-nav.ts`, `ChatSidebar.svelte`), desktop thread heading focus & Threads muted-root Escape recovery (`ThreadPanel.svelte`, `chat/+page.svelte`), CmdK `#` channel jump + single-key `#` shortcut (`ExecutiveCommandBar.svelte`), multiline composer & configurable Enter-to-send (`Composer.svelte`, already matches `1ba00168`). |

## Sync History Log

| Date (UTC) | Area | POC Range | Target Svelte Commit(s) | Visual Comparison | Review Verdict |
| --- | --- | --- | --- | --- | --- |
| 2026-11-23 | Tracks 1–4 (Core shell, Work rows, Inbox split triage, Activity assembly line) | initial -> `b54312b4` | `4e238951`, `13d914ca`, `0d27c37f`, `3eb2cecb` | Verified (`scratch/combined-v6-shots/`) | Codex APPROVE |
| 2026-11-23 | Track 5 (Graphite, Cozy Glass + HC, Home scroll cue) | `8d0ee205` -> `6e2501b3` | `a80a695b`..`2c93345f` | Verified (`scratch/track5-shots/`) | Codex APPROVE |
| 2026-11-23 | Phase 1 (Index `/home`, neutral Graphite, Ocean interior navy, inline Home search, Report Day bars) | `6e2501b3` -> `7449753c` | `a4783332`, `aba2aec6`, `7ba7c4a7`, `79b0af03`, `8f25f08f`, `922d0df9` | Verified (`scratch/phase1-home-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-23 | Phase 2 (Inbox `/inbox` 1:1 parity, Graphite sidebar hierarchy & matte surfaces, dark CmdK contrast) | `b54312b4` -> `ffe0e0ce` | `75ce0c7f`, `48445b50`, `0e06cdc4`, `8f94f6a6`, `73893015`, `ef462680`, `52dcb756` | Verified (`scratch/phase2-inbox-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-24 | Phase 3 (Work `/work` Inbox-aligned header/search/toolbar, List/Board toggle, Graphite/Cozy/Report surfaces) | `13d914ca` -> `26a49c02` | `59596f46`, `facfd212`, `d954b666`, `da7bd591` | Verified (`scratch/phase3-work-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-24 | Phase 4A + 4B (Activity timeline, Cozy Chat + keyboard nav, Cozy Work canvas, Work inline split reading panel, CmdK `#` channel jump, Report masthead simplification) | `355e3f85` -> `1ba00168` | `66a7248e`, `43801221`, `c91bc26b`, `7c81b7f7`, `17bf8c8b`, `0d3bd5d0`, `8871dc9e`, `86f520c5`, `50a8583a`, `1a4557f7` | Verified (`screenshots/phase4/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh` final) |
