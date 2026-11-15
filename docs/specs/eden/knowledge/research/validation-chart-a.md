# Validation: chart A (BUG-114, BUG-115, BUG-117)

Eden · 2026-11-12 · for ada (msg 01M788NZ). The change is Avery's avery/chart-chip-target-focus (`0760db2f`), merged to v1 as **`2b86877b`**. I compared it with its first parent, **`06343b32`**.

I ran three checks in parallel, all in Chromium and WebKit:
- **A:** geometry and the focus ring;
- **B:** focus after a removal;
- **C:** kept behaviour and regressions.

Evidence is in my session scratch at `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/ca/`. Paths below are relative to that folder, and the brief is `BRIEF.md`.

## Verdict

**BUG-114 and BUG-117 are met. BUG-115 is met for the keyboard in both engines and for the pointer in Chromium. With the pointer in WebKit it's partly unmet, unchanged from before.**

That WebKit pointer gap (P1) is pre-existing: the chart and the dialog returns behave the same before and after, and the project page actually improved (pointer moves went from 0/22 to 18/22). It's the same click-focus family as BUG-166 and BUG-158. I rate it S4, like BUG-115 itself (downgraded to S4 at 02:58, because the next Tab continues in place) and BUG-166.

Nothing regressed. In Fit, the ring still draws below 2 px (N1, the unfinished part of BUG-117), and the layout changed slightly, which I accept (N2). BUG-138 still reproduces (N3), and it retires with the People view.

I opened these myself:
- the Fit before/after pair at 1280 (Chromium) and the 390 WebKit Fit screenshot;
- a ring crop;
- the WebKit pointer screenshot after "Removed Atlas from Web" (no ring anywhere; focus is on the chart region);
- the BUG-123 screenshot and its facts file.

I also recomputed every BUG-115 count from B's results files (`B/results-*.jsonl`), grouped by surface, activation, engine and build. All 48 WebKit failures after the merge are pointer, and Enter and Space pass 159/159.

## Requirements

Unless a row says otherwise, the denominators are:
- **A:** 80 configurations (5 widths × 4 text sizes × 2 engines × 2 builds), with 4 chart Remove buttons each, so 160 buttons per build;
- **B:** 237 cases per build and engine. The chart has 8 removal cases (C1–C8, where C5 and C7 also have a confirm), and the project page has 6 (P1–P6 on `/projects` at 3 sizes; P1, P2, P3 and P6 on the profile at 1280). The BUG-115 rows below count each case once per size and activation;
- **C:** 36 cases × 4 configurations.

| Requirement | Before | After | Result | Evidence |
| --- | --- | --- | --- | --- |
| **BUG-114:** the chart project Remove is 24 px (1.5 rem) at every Fit scale | 0/160 (18 px at ≤960, 19.64 at 1280, 100%) | 160/160 (24×24 at 100%, 40/40; 30/48/60 at the 20 px root, 200% and 40 px root) | Met | `A/geom/geom-{chromium,webkit}.json` |
| The card still scales; the icon is centred; no overflow or overlap | 160/160 | 160/160. The icon offset is 0 in 160/160, it's inside the card in 160/160, and it overlaps a sibling in 0/160 | Met (see N2) | same |
| Actual size (scale 1) unchanged | 160/160 | 160/160 | Met | same |
| **BUG-117:** outline offset 2 on both implementations (chart and project chips) | 0px in 24/24 | 2px in 24/24 (computed) | Met (see N1 for the rendered value in Fit) | `A/ring/ring-all.json`, `A/ring/shots/` |
| The ring isn't clipped and has ≥3:1 contrast | 24/24 | 24/24. Clipped 0/24 per build. The lowest per-sample median contrast is 3.41 (project chip, 40 px root); the lowest 10th percentile is 3.29 (WebKit, chart Fit, 40 px root, anti-aliased edges) | Met | same |
| **BUG-115, chart:** after a removal, focus goes to the next Remove, else the previous one, else "Actions for {team}", else the region; keyboard (Enter and Space, 3 sizes, both engines, immediate and confirmed) | 0/96 (all on `body`) | 95/96 (the 1 miss is a harness timeout at Chromium 390 C7 Enter; its screenshot shows the right result, and the rerun passed 27/27) | Met | `B/results-*.jsonl`, `B/B-quant.txt` |
| BUG-115, chart, pointer, Chromium | 0/24 | 24/24 | Met | same |
| **BUG-115, chart, pointer, WebKit** | 3/24 | 3/24. 21 stay on the chart region; the 3 passes are C8 only, where the region is the expected target | **Not met, pre-existing (P1)** | same; `B/shots/after-webkit-w1280-C1-web-first-of-3-pointer.png` |
| A card that disappears sends focus to the chart region (C8) | 3/18 (only WebKit pointer, where the click had already focused the region; the rest on `body`) | 18/18 | Met | same |
| A project in two cards keeps focus in the card the user worked in (C4) | 0/18 (15 on `body`, 3 WebKit pointer on the region) | 15/18. The 3 misses are WebKit pointer, on the chart region (P1). In all builds and engines, no chart sample landed in another card | Met (P1 aside) | same |
| Never `body` and never a removed node, after a removal (keyboard and Chromium pointer, chart and project page) | `body` 230/230 | 0 on `body`; 0 detached in all 948 samples | Met | same |
| **BUG-115, project page:** next chip, else previous, else "Add team…"/"Assign team…" (keyboard and Chromium pointer; `/projects` at 3 sizes, the profile at 1280) | 0/110 | 110/110 | Met | same |
| BUG-115, project page, pointer, WebKit | 0/22 | 18/22. Immediate moves pass 18/18 (profile included); the confirmed last unlink (P6) lands on `body` 4/4 | **Partly unmet: improved, the confirm unchanged (P1)** | `B/shots/after-webkit-w1280-P6-scratch-only-confirm-pointer-confirm.png` |
| Cancel and Escape return to the opening Remove (keyboard; Chromium pointer) | 100/100 | 100/100 | Met | same |
| Cancel and Escape return to the opener, WebKit pointer | 0/20 | 0/20 (chart: region 12; project page: `body` 8) | **Not met, unchanged (P1)** | `B/probe-{before,after}-webkit.log` |
| A failed removal keeps focus where it was and changes nothing ("Couldn't remove…"; the dialog alert says "The office went away.") | 66/72 | 66/72. Judged strictly (focus on the trigger, or in the dialog with its error), the 6 misses are all WebKit pointer immediate failures: 3 on the chart leave focus on the region the click focused, and 3 on the project page leave it on `body` (measured; the harness had scored these 3 as passes). Both are P1. Nothing changed in 72/72 per build | Met except P1 | `B/results-*.jsonl` |
| Delayed confirmation: an older confirmation doesn't steal focus from a later Cancel opener (projects, profile, chart; 1280; both engines) | 6/6 | 6/6 | Met. Before passes trivially (it has no focus plan), so this can't show fail-then-pass | `B/shots/` D-delayed |
| The result is announced: Sonner's polite live region holds the toast text | Same text | Same text | Met (DOM only, not tested with VoiceOver) | `B/summary.txt` |
| **Kept, BUG-129:** the View menu rejects stale focus (Escape, item select, click outside, first arrow and Tab on all three tabs, live remount; tests in `C/C-results-full.json`) | 40/40 | 40/40 (20 per engine). The click-outside tests pass on stale focus, but focus lands on `body`: that's BUG-158 | Kept | `C/facts/*bug129-*` |
| **Kept, BUG-123:** blocked Fit explains itself and doesn't activate (pointer, and the refusal clearing when Fit becomes available); two-stage Escape in the spec sequence (open, Escape, Escape) | 4/4 + 2/2 | 4/4 + 2/2 | Kept. My keyboard variant in C-full (Enter on the blocked item, then Escape; `C/C-results-full.json`, test "C2 BUG-123 keyboard") fails 2/2 per build: that's BUG-138 (N3). The pointer and refusal tests (4/4 per build) are in the same file | `C/C-bug123b.log`, `C/C-results-full.json` |
| Chart/List, Fit and Actual size, Collapse all and Expand all; chips (Shared, No team, Assign team…); "Actions for {team}" after a removal | 16/16 | 16/16 | Kept | `C/shots/*controls-*`, `*chips-*`, `C/facts/*menu-after-removal-*` |
| Accessibility trees (Teams chart region, org-chart main, `/projects`, `/projects/beacon`; 1280 and 390) | — | 16/16 pairs identical | Kept | `C/aria/*.yml` |
| Visual sweep at 1280 and 390, both engines | — | 40/44 pairs pixel-identical. The other 4 are all Teams Chart Fit (N2) | Met with N2 | `A/sweep/sweep-all.json` |
| Console and page errors | 4 (WebKit, People at 1440, ResizeObserver loop) | 4 (the same) | No new errors; blitz 0 in 6 runs per build | `C/errors/` |
| Merged e2e tests for BUG-114/115/117 (Chromium; the repo's WebKit grep excludes them) | — | 53/53 | Pass | `C/C-repo-e2e.log` |
| BUG-119 at 960 with a 40 px root: known, not counted | Occluded | Occluded (same) | Excluded | `A/bug119/` |

## Findings

| # | Sev | Kind | Where | Expected → actual | Fix outcome |
| --- | --- | --- | --- | --- | --- |
| P1 | S4 | BUG-115 partly unmet, unchanged from before except the project page's immediate moves, which improved (0/22 → 18/22) | WebKit, pointer. `/org-chart` Teams Chart at all 3 sizes; `/projects` and `/projects/{id}` | The pinned BUG-115 order applies to pointer too, and focus is never `body`. Actual: chart removals leave focus on the "Teams chart" region (21/24), immediate failures leave it on the region (chart, 3/3) or `body` (project page, 3/3) instead of the trigger, and Cancel/Escape land on the region or `body` (20/20). On the project page a pointer-confirmed last unlink lands on `body` (4/4). S4 like BUG-115 (02:58 downgrade) and BUG-166: the next Tab continues in place. Cause: B's focus probe measured that a WebKit click doesn't focus the button, so focus is on the region or the dialog content when the plan runs. Inference, from reading the code: the plan's guard ("focus moved elsewhere, leave it") then exits. Screenshots: `B/shots/after-webkit-w1280-C1-web-first-of-3-pointer.png`, `B/shots/after-webkit-w1280-P6-scratch-only-confirm-pointer-confirm.png` | After a pointer removal in WebKit, focus reaches the pinned target. Cancel and Escape return to the opening Remove. The plan treats the chart region, or focus inside the closing dialog, as "not moved away". Same click-focus family as BUG-166 and BUG-158 |
| N1 | S4 | The unfinished part of BUG-117: the offset was 0 before; the width was already 1.5 | Fit, chart Remove, both engines | The ring is 2 CSS px (§17). Actual: the computed offset is 2 and the width 2, but the scale shrinks them to 1.5 at 0.75 and 1.64 at 0.8185 (8/8 Fit samples). Before: offset 0, width 1.5 at 0.75 and 1.64 at 0.8185. Screenshots: `A/ring/shots/*-after-*-chart-fit-focused.png` | Counter-scale the ring as the target is, e.g. `outline-offset: calc(2px / var(--fit-scale))`, and the same for the width |
| N2 | — | New in after, accepted | Teams Chart Fit, 1280 and 390, both engines | The larger target pushes Design's "Shared" badge onto its own line (matching the Web card). Cards with projects grow by 2–5 px, and Design by about 17 px. No overlap or overflow. I opened the 1280 pair: `A/sweep/shots/chromium-1280-teams-chart-fit-{before,after}.png` | None. I accept it as the cost of a 24 px target. Recorded, not counted |
| N3 | S4 | Pre-existing: **BUG-138** (blocked Fit activation loses the explanation), still reproducing | `/org-chart` People, View open, 320/100%, both engines | After Enter or Space on the blocked "Fit to width", the first Escape should dismiss only the explanation. Actual: it closes the explanation and the menu, and focus goes to View (4/4 per build, Enter and Space × 2 engines, the same before and after). The spec sequence (open, Escape, Escape) still passes 2/2 per build (`C/C-bug123b.log`). The Enter and Space records are in `C/facts/*__bug123-{Enter,Space}.json`: the `[role=tooltip]` node stays in the DOM, but the screenshot shows no visible explanation, so I judge BUG-138 visually. Screenshots: `C/shots/{before,after}-{chromium,webkit}__bug123-Enter-on-blocked.png` | BUG-138's outcome: the explanation stays after activation. Already RETIRED in BUGS.md (16:35 triage, D36); it comes back only if IA-02 slips out of IA-M1 |

Recorded, not counted:
- **A click outside the View menu leaves focus on `body`:** People, 20 px root, 8/8 per build (4 widths × 2 engines), the same before and after. This is BUG-158, which Avery is fixing now.
- **WebKit ResizeObserver loop on People at 1440:** pre-existing, 4 in each build. It's like BUG-108, but on a different view (BUG-108 is Reporting lines). The view retires under D36.
- **Avery's chart fix and the new triage:** chart cut A was KEEP in the IA v2 triage, and D36 retires the Teams chart. These fixes still matter until IA-02 lands.

## Follow-ups (not run)
- **Delayed confirmation:** a mutation check (capture `restoreLast` at resolve time) to show the delayed-confirmation fix fails without it.
- **Profile route sizes:** the profile route ran at 1280 only; 390 and 960 weren't run.
- **WebKit e2e:** the merged e2e tests aren't run in WebKit by the repo config.
- **Assistive technology:** VoiceOver, and the pointer flow in the real Tauri shell.

## Final lines (verbatim, from the logs)
- A:
  - `A-geom done: 40/40 configs ok` (chromium and webkit)
  - `A-ring done: 48/48 samples ok`
  - `A-sweep done: 40/44 pairs pixel-identical, 0 errors`
  - `A-119 done: 4/4 configs ok`
- B:
  - `B-focus after chromium done: 236/237 pass, 1 errors`
  - `B-focus after webkit done: 189/237 pass, 0 errors`
  - `B-focus before chromium done: 99/237 pass, 0 errors`
  - `B-focus before webkit done: 79/237 pass, 0 errors`
  - rerun: `B-focus after chromium done: 27/27 pass, 0 errors`
- C:
  - `C-full.log`: `  140 passed (5.7m)`, after `  4 failed`. The 4 failures are N3's variant, the same before and after.
  - `C-bug123b.log`: `  12 passed (54.9s)`
  - `C-repo-e2e.log`: `  53 passed (2.3m)`

Catalog: `org-chart-fit-remove` (2b86877, validated).
