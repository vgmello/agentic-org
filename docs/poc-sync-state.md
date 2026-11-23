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
| **Work (`/work`)** | `13d914ca` *(Track 2 compact rows)* | `26a49c02` | **Phase 3 Active** (`..26a49c02`) | **Phase 3 next**: `work-header queue-header` + `work-toolbar queue-toolbar` aligned with Inbox (`320d2596`, `eadf0aa8`, `b73964da`), unified page-header search (`2777ca94`, `5a9f9a25`, `f96b81b4`), Graphite matte Work canvas (`215be52a`, `4622ad46`), work item reading panel & enriched details (`f1db7133`, `0e3ec0ed`, `26a49c02`). |
| **Activity (`/activity`)** | `b54312b4` *(Track 4 Assembly Line first)* | `26a49c02` | Deferred until Index/Inbox/Work land | Interior Ocean navy applied in Phase 1; chronological timeline (`580b6715`) & unified interior header/search (`5a9f9a25`, `2777ca94`, `f96b81b4`) queued after Index, Inbox, and Work. |

## Sync History Log

| Date (UTC) | Area | POC Range | Target Svelte Commit(s) | Visual Comparison | Review Verdict |
| --- | --- | --- | --- | --- | --- |
| 2026-11-23 | Tracks 1–4 (Core shell, Work rows, Inbox split triage, Activity assembly line) | initial -> `b54312b4` | `4e238951`, `13d914ca`, `0d27c37f`, `3eb2cecb` | Verified (`scratch/combined-v6-shots/`) | Codex APPROVE |
| 2026-11-23 | Track 5 (Graphite, Cozy Glass + HC, Home scroll cue) | `8d0ee205` -> `6e2501b3` | `a80a695b`..`2c93345f` | Verified (`scratch/track5-shots/`) | Codex APPROVE |
| 2026-11-23 | Phase 1 (Index `/home`, neutral Graphite, Ocean interior navy, inline Home search, Report Day bars) | `6e2501b3` -> `7449753c` | `a4783332`, `aba2aec6`, `7ba7c4a7`, `79b0af03`, `8f25f08f`, `922d0df9` | Verified (`scratch/phase1-home-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
| 2026-11-23 | Phase 2 (Inbox `/inbox` 1:1 parity, Graphite sidebar hierarchy & matte surfaces, dark CmdK contrast) | `b54312b4` -> `ffe0e0ce` | `75ce0c7f`, `48445b50`, `0e06cdc4`, `8f94f6a6`, `73893015`, `ef462680`, `52dcb756` | Verified (`scratch/phase2-inbox-shots/`) | Codex APPROVE (fresh `xhigh`) |
