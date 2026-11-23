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
| **Themes & Global Shell** (`ocean`, `classic`, `graphite`, `cozy`, `report`, `report-dark`, topbar/search, sidebar) | `7449753c` *(Codex APPROVE @ `922d0df9`)* | `c64b187a` | Synced through `7449753c` (+7 newer commits queued) | Synced neutral Graphite, Ocean interior navy, Cozy Glass, Report Day/Night WCAG AA (`>= 4.0:1` bars) through `7449753c`; newer commits `13abb1f1` (Graphite sidebar text hierarchy), `2777ca94`/`5a9f9a25`/`f96b81b4` (unified inline page-header search & Workspace), `b73964da` (sidebar controls), `355e3f85` (Report masthead simplification) queued for Phase 2. |
| **Index (`/home`)** | `7449753c` *(Codex APPROVE @ `922d0df9`)* | `c64b187a` | Synced through `7449753c` | Exact 1:1 parity through `7449753c` (inline Home search combobox, KPI pillars + overdue escalation, Spotlight `Ask about this` + `spot-jump` + 36/44px targets, Executive Brief footer/expand, Projects, Upcoming milestones `0..14d`, Capacity breakdown); verify `f96b81b4`/`355e3f85` in Phase 2. |
| **Inbox (`/inbox`)** | `b54312b4` *(initial Track 3 port)* | `c64b187a` | **Phase 2 In Progress** (`b54312b4..c64b187a`) | **Phase 2 active**: 3 primary categories (`cd5128ae`), simplified intro & Activity-matched sender + hover/focus preview (`915bf1e5`, `e8eecf7e`, `7449753c`), centered Inbox heading (`355e3f85`), unified inline page-header search & Workspace (`2777ca94`, `5a9f9a25`, `f96b81b4`), Graphite Inbox surfaces/backgrounds/queue/divider (`1f5f7149`, `215be52a`, `e75d920a`), spacing & Report row tint (`4622ad46`), Inbox comments + audit entries (`f9fd24ba`). |
| **Work (`/work`)** | `13d914ca` *(Track 2 compact rows)* | `c64b187a` | Pending (`..c64b187a`) | **Phase 3**: `work-header queue-header` + `queue-toolbar` aligned with Inbox (`320d2596`, `eadf0aa8`, `b73964da`), unified inline page-header search & Workspace (`2777ca94`, `5a9f9a25`, `f96b81b4`), Graphite Work surfaces (`215be52a`, `4622ad46`), work item reading panel (`f1db7133`). |
| **Activity (`/activity`)** | `b54312b4` *(Track 4 Assembly Line first)* | `c64b187a` | Deferred until Index/Inbox/Work land | Interior Ocean navy applied in Phase 1; chronological timeline (`580b6715`) & unified interior header/search (`5a9f9a25`, `2777ca94`, `f96b81b4`) queued after Index, Inbox, and Work. |

## Sync History Log

| Date (UTC) | Area | POC Range | Target Svelte Commit(s) | Visual Comparison | Review Verdict |
| --- | --- | --- | --- | --- | --- |
| 2026-11-23 | Tracks 1–4 (Core shell, Work rows, Inbox split triage, Activity assembly line) | initial -> `b54312b4` | `4e238951`, `13d914ca`, `0d27c37f`, `3eb2cecb` | Verified (`scratch/combined-v6-shots/`) | Codex APPROVE |
| 2026-11-23 | Track 5 (Graphite, Cozy Glass + HC, Home scroll cue) | `8d0ee205` -> `6e2501b3` | `a80a695b`..`2c93345f` | Verified (`scratch/track5-shots/`) | Codex APPROVE |
| 2026-11-23 | Phase 1 (Index `/home`, neutral Graphite, Ocean interior navy, inline Home search, Report Day bars) | `6e2501b3` -> `7449753c` | `a4783332`, `aba2aec6`, `7ba7c4a7`, `79b0af03`, `8f25f08f`, `922d0df9` | Verified (`scratch/phase1-home-shots/` + `product_page_gallery.md`) | Codex APPROVE (fresh `xhigh`) |
