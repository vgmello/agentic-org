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

| Area | Last Synced POC Commit | Latest Approved Release Marker | Status | Notes |
| --- | --- | --- | --- | --- |
| **Themes & Global Shell** (`ocean`, `classic`, `graphite`, `cozy`, `report`, `report-dark`, topbar/search, sidebar) | `53728c05` | `53728c05` | Synced through `53728c05` | Phase 7 synced `--detail-panel-surface`, `--workspace-secondary-surface`, and `--assembly-surface` tokens: Classic & Report Night `#1e2939` for detail/Assembly/Release gates and `#1b2533` for `Work by stage` & Assembly table headers; Report Day white `#ffffff` Assembly; Cozy `#463c30` Assembly canvas keeping `#352b22` dock; Graphite `#18191a` Assembly canvas; Ocean `--ocean-header` on Project headers; non-blurred Inbox menu surfaces matching Work. |
| **Index (`/home`)** | `53728c05` | `53728c05` | Synced through `53728c05` | Exact 1:1 parity through `53728c05` (no Index structural changes in `3c4e3541..53728c05`; inherits Phase 7 theme surface tokens). |
| **Inbox (`/inbox`)** | `53728c05` | `53728c05` | Synced through `53728c05` | Phase 7 matched Inbox `More` / `Filter` / `Display` dropdown surfaces (`.triage-menu-surface`) to Work and unified reading-pane surfaces (`--detail-panel-surface`: `#1e2939` in Classic/Report Night, `#ffffff` in Report Day). |
| **Work (`/work`)** | `53728c05` | `53728c05` | Synced through `53728c05` | Phase 7 defaulted Project Work List views to Status grouping with `Blocked` first while preserving Board and Org Work defaults, and aligned reading-panel surface tokens across themes. |
| **Activity (`/activity`)** | `53728c05` | `53728c05` | Synced through `53728c05` | Phase 7 added saved `Assembly line display` -> `Row dividers` preference (off by default, `data-assembly-dividers`), independent `.activity-panel-scroll` with `Scroll for more` button that hides at the bottom, `--assembly-surface` / `--workspace-secondary-surface` theming, and project-scoped embedding with Stations slot dots, Milestones, Needs a human, Gate status, 7d Throughput + Avg. cycle time + Waiting on people now, and chronological Line events. |
| **Projects (`/projects`)** | `53728c05` | `53728c05` | Synced through `53728c05` | Phase 7 reordered primary project tabs (`Overview`, `Activity`, `Work`, `Insights`, `Conversations`, `Team`, followed by served `Features`, `Releases`, `Docs`, `Settings`) with Work-style underline tabs + icon-only `More` menu with Bits UI tooltip; added `ProjectReleaseStrip` under the toolbar on Overview & Activity; replaced Project Activity with project-scoped Assembly line + Activity feed toolbar switcher & Release gates dock; added shared `Charts`/`Tables` toggle, `Median cycle time` KPI tile, `Token usage & cache mix`, and `Current release delivery` on Project Insights. |
| **Chat (`/chat`)** | `53728c05` | `53728c05` | Synced through `53728c05` | Exact parity through `53728c05` (inherits shared `#1b2533` `--workspace-secondary-surface` chrome in Classic/Report Night). |

## Sync History Log

| Date (UTC) | Area | POC Range | Target Svelte Commit(s) | Visual Comparison | Review Verdict |
| --- | --- | --- | --- | --- | --- |
| 2026-11-23 | Tracks 1–4 (Core shell, Work rows, Inbox split triage, Activity assembly line) | initial -> `b54312b4` | `4e238951`, `13d914ca`, `0d27c37f`, `3eb2cecb` | Verified (`scratch/combined-v6-shots/`) | Codex APPROVE |
| 2026-11-23 | Track 5 (Graphite, Cozy Glass + HC, Home scroll cue) | `8d0ee205` -> `6e2501b3` | `a80a695b`..`2c93345f` | Verified (`scratch/track5-shots/`) | Codex APPROVE |
| 2026-11-23 | Phase 1 (Index `/home`, neutral Graphite, Ocean interior navy, inline Home search, Report Day bars) | `6e2501b3` -> `7449753c` | `a4783332`, `aba2aec6`, `7ba7c4a7`, `79b0af03`, `8f25f08f`, `922d0df9` | Verified (`scratch/phase1-home-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-23 | Phase 2 (Inbox `/inbox` 1:1 parity, Graphite sidebar hierarchy & matte surfaces, dark CmdK contrast) | `b54312b4` -> `ffe0e0ce` | `75ce0c7f`, `48445b50`, `0e06cdc4`, `8f94f6a6`, `73893015`, `ef462680`, `52dcb756` | Verified (`scratch/phase2-inbox-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-24 | Phase 3 (Work `/work` Inbox-aligned header/search/toolbar, List/Board toggle, Graphite/Cozy/Report surfaces) | `13d914ca` -> `26a49c02` | `59596f46`, `facfd212`, `d954b666`, `da7bd591` | Verified (`scratch/phase3-work-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-24 | Phase 4A + 4B (Activity timeline, Cozy Chat + keyboard nav, Cozy Work canvas, Work inline split reading panel, CmdK `#` channel jump, Report masthead simplification) | `355e3f85` -> `1ba00168` | `66a7248e`, `43801221`, `c91bc26b`, `7c81b7f7`, `17bf8c8b`, `0d3bd5d0`, `8871dc9e`, `86f520c5`, `50a8583a`, `1a4557f7` | Verified (`screenshots/phase4/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh` final) |
| 2026-11-24 | Phase 5A + 5B (Cross-theme sidebar lighting & dark interior headers, Graphite `#1b1c1d` queue sheen, Cozy `.work-ref`, Work 3-bar priority marks on all tickets + full-width L->R reading panel reveal + `searchCloseSeq`/whitespace clear guards, Activity & Projects aligned toolbars, gateway CSP `/create`, icon-only tooltip rule) | `1ba00168` -> `2c58edcd` *(pre-release baseline)* | `410f8cab`, `3dea3a0d`, `2098ffbe`, `faf43562`, `bd113179`, `2fca7698`, `cc9c3f68` | Verified (`screenshots/phase5/` + `screenshots/index.html`) | Codex APPROVE (fresh `xhigh` final @ `cc9c3f68`) |
| 2026-11-24 | Phase 6 (`Prototype Release: 3c4e3541` — Classic crisp radii, Graphite-only sidebar edge shade & `--side-panel-surface`, `S`/`M` keyboard navigation sequences, Work right-hand sliding panel with expand/restore + 2-level grouping + 4-stage/6-status board toggle, Activity full-space Portfolio Assembly + FLIP transitions + collapsible Release gates dock, icon-only Bits UI tooltips) | `2c58edcd` -> `3c4e3541` | `b6f4521e` | Verified (`screenshots/phase6/` + `screenshots/index.html`) | Codex APPROVE (`xhigh` + fresh `max`) |
| 2026-11-24 | Phase 7 (`Prototype Releases: 5da47e1d + 53728c05` — Project nav reorder + underline tabs + `More` menu tooltip, `ProjectReleaseStrip` on Overview & Activity, Project Activity Assembly/Feed toolbar switcher + saved `Row dividers` Display preference + project Release gates dock with agent slot dots/telemetry/Line events + independent scroll `Scroll for more`, Project Work default Status grouping with Blocked first, Project Insights shared Charts/Tables toggle + Median cycle time + Token mix + Current release delivery, cross-theme Assembly/detail/Inbox menu surfaces) | `3c4e3541` -> `53728c05` | pending commit | Verified (`screenshots/phase7/`) | pending `xhigh` + fresh `max` |
