# POC -> Svelte UI Sync Tracker

Tracks the exact `poc/` commit watermarks applied to the production SvelteKit UI (`apps/agentic-org-ui/`).

## How each sync pass works

1. Fetch all remote refs: `git fetch --all --prune` (checking both `poc/ui-themes` and `origin/poc/ui-themes-prototype`).
2. Inspect new prototype commits since the area's watermark:
   ```sh
   git log --oneline <last_synced_sha>..HEAD -- poc/
   git log --oneline <last_synced_sha>..origin/poc/ui-themes-prototype -- poc/
   git diff <last_synced_sha>..HEAD -- poc/
   ```
3. Port all changes into `apps/agentic-org-ui/` (never editing `poc/`), verify WCAG AA contrast, unit tests, E2E checks (`--workers=1`), and side-by-side POC vs Svelte screenshots.
4. Complete Codex `gpt-6.2-sol` review + fresh empty-context final sign-off, commit, push, and advance the watermark table below.

## Current Watermarks by Area

| Area | Last Synced POC Commit | Latest Available POC Commit | Status | Notes |
| --- | --- | --- | --- | --- |
| **Themes & Global Shell** (`ocean`, `classic`, `graphite`, `cozy`, `report`, `report-dark`, topbar/search, sidebar) | `ffe0e0ce` *(Codex APPROVE @ `52dcb756`)* | `26a49c02` | Synced through `ffe0e0ce` | Synced neutral Graphite, Graphite interior sidebar hierarchy (`13abb1f1`), sidebar pin/controls (`b73964da`), dark CmdK contrast (`ffe0e0ce`), Ocean interior navy, Cozy Glass, Report Day/Night WCAG AA through `ffe0e0ce`. |
| **Index (`/home`)** | `7449753c` *(Codex APPROVE @ `922d0df9`)* | `26a49c02` | Synced through `7449753c` | Exact 1:1 parity through `7449753c` (inline Home search combobox, KPI pillars + overdue escalation, Spotlight `Ask about this` + `spot-jump` + 36/44px targets, Executive Brief footer/expand, Projects, Upcoming milestones `0..14d`, Capacity breakdown). |
| **Inbox (`/inbox`)** | `ffe0e0ce` *(Codex APPROVE @ `52dcb756`)* | `26a49c02` | Synced through `ffe0e0ce` | Exact 1:1 parity through `ffe0e0ce`: 3 primary categories + `More` dropdown (`cd5128ae`), compact `Filter inbox` & `Inbox display` (`Priority first`/`By date` + persisted `Row dividers`, `b73964da`), centered header + search (`355e3f85`, `5a9f9a25`, `f96b81b4`), Graphite matte surfaces & 3-column queue grid (`1f5f7149`, `215be52a`, `e75d920a`), Report row tints (`4622ad46`), simplified sender + WCAG 1.4.13 hover/focus preview (`915bf1e5`, `e8eecf7e`, `7449753c`), persistent Inbox comments + Request history timeline (`f9fd24ba`). |
| **Work (`/work`)** | `26a49c02` *(Codex APPROVE @ `da7bd591`)* | `26a49c02` | Synced through `26a49c02` | Exact 1:1 parity with Inbox controls through `26a49c02`: `.work-header.queue-header` + `.work-toolbar.queue-toolbar`, inline `/` Work search, `List`/`Board` layout toggle + Board columns, race-safe URL-synced `Filter work` (`?project=`/`?assignee=`), `Work display` (`By status`/`By project` + persisted `Row dividers`), `RequestsPanel` search/clear & hit targets, Graphite matte `#18191a` canvas, Cozy Glass frosted Board cards, Report Day/Night WCAG AA contrast. |
| **Activity (`/activity`)** | `b54312b4` *(Track 4 Assembly Line first)* | `26a49c02` | Deferred until requested | Interior Ocean navy applied in Phase 1; chronological timeline (`580b6715`) & unified interior header/search (`5a9f9a25`, `2777ca94`, `f96b81b4`) ready when requested. |

## Sync History Log

| Date (UTC) | Area | POC Range | Target Svelte Commit(s) | Visual Comparison | Review Verdict |
| --- | --- | --- | --- | --- | --- |
| 2026-11-23 | Tracks 1–4 (Core shell, Work rows, Inbox split triage, Activity assembly line) | initial -> `b54312b4` | `4e238951`, `13d914ca`, `0d27c37f`, `3eb2cecb` | Verified (`scratch/combined-v6-shots/`) | Codex APPROVE |
| 2026-11-23 | Track 5 (Graphite, Cozy Glass + HC, Home scroll cue) | `8d0ee205` -> `6e2501b3` | `a80a695b`..`2c93345f` | Verified (`scratch/track5-shots/`) | Codex APPROVE |
| 2026-11-23 | Phase 1 (Index `/home`, neutral Graphite, Ocean interior navy, inline Home search, Report Day bars) | `6e2501b3` -> `7449753c` | `a4783332`, `aba2aec6`, `7ba7c4a7`, `79b0af03`, `8f25f08f`, `922d0df9` | Verified (`scratch/phase1-home-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-23 | Phase 2 (Inbox `/inbox` 1:1 parity, Graphite sidebar hierarchy & matte surfaces, dark CmdK contrast) | `b54312b4` -> `ffe0e0ce` | `75ce0c7f`, `48445b50`, `0e06cdc4`, `8f94f6a6`, `73893015`, `ef462680`, `52dcb756` | Verified (`scratch/phase2-inbox-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-24 | Phase 3 (Work `/work` Inbox-aligned header/search/toolbar, List/Board toggle, Graphite/Cozy/Report surfaces) | `13d914ca` -> `26a49c02` | `59596f46`, `facfd212`, `d954b666`, `da7bd591` | Verified (`scratch/phase3-work-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
