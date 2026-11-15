# Validation · BUG-150 Offices status flow (v1 34ed1b29)

Requested by ada in a session message on 12 Nov. This is Avery's avery/bug150-office-status-wrap, merged as `34ed1b29`, compared with its first parent `0c61952b`. It fixes N1 from `validation-offices-followup.md` under Rowan's pin (01M77Q5JF1) and its amendment as ada relayed it: the chevron sits on the same line, centred (centres within 1 px, overlapping vertically); the original wide gap is kept; and `ml-2` applies only outside the 21rem compact boundary. I checked it on 12 Nov 2026 in WebKit (the Tauri engine) and Chromium, in three parallel checks. Each check ran its own before and after worktrees and dev servers (`--mode e2e`). The 20, 32 and 40 px and 200% roots are set on the computed root after load.

The change has two parts. The chevron moves inside the `office-actions` group, which gains `justify-end`. The chevron gets `ml-2`, which becomes `ml-0` inside `@max-[21rem]`.

**Verdict: BUG-150 is met in both engines, under my reading of "fits" (below), which Rowan confirmed (01M783JY1F), with no regressions and no new findings.**
- **Wrapped flow:** every line of the status group now ends at the row's content end. Before, 34 of 660 row measurements (both engines together) had a start-aligned line.
- **Chevron on the previous item's line:** whenever the chevron fits on the line the item before it (Retry, or the pill when there's no Retry) actually sits on, it shares that line. Their centres are equal (Δ 0, 608 of 608), and the gap is the original one: the row gap of 1rem outside compact (16, 20 or 32 px by root), the group gap inside. In 12 cases, the same before, the chevron sits alone under a full pill-and-Retry line; I count that as met (see "Recorded").
- **100% text:** pixel-identical inside every office row (90 before/after pairs, 45 per engine). The chevron doesn't move.
- **Kept:** focus, keyboard order, accessible names, the accessibility tree (0 differences), Retry, the details sheet, live status, contrast, and every earlier Offices fix.

I report one pre-existing S4 (P2, which ada has since filed as BUG-166) that showed up along the way, and I note one known item (P1) without counting it. P1 and P2 are this report's numbering; the follow-up's P1–P3 are cited as "follow-up P…".

## Requirements (evidence I opened)

Paths are relative to `…/scratchpad/b150/`. Counts are per build, with both engines together: 660 office-row measurements per build, from 600 page loads (10 states × 6 widths × 5 roots × 2 engines), some pages having two rows. The `main` scroll check is per page. The pass/fail columns read pass/fail/not-applicable.

| Requirement | Before (0c61952b) | After (34ed1b29) | Result | Evidence |
| --- | --- | --- | --- | --- |
| One end-aligned flow: every line of pill, Retry and chevron ends at the row's content end | 626/34 (start-aligned when wrapped: 12 Unreachable and 22 Retrying ("Checking…" with "Retrying…") measurements, including 640 and 960 at large roots) | 660/0 | met (fixed) | `A/out/counts.json`, `A/out/fails.json`; `A/shots/webkit-{before,after}-unreachable-320-40px.png` |
| The pill is never start-aligned on a partial line | 626/34 | 660/0 | met (fixed) | same |
| The chevron is last (DOM and visual) | 660/0 | 660/0 | met | `A/out/counts.json` |
| The chevron shares Retry's line when it fits (brief: pill or Retry + gap + chevron ≤ the line width) | literal reading 572/48/40; on the previous item's actual line 572/36/52 | literal 608/12/40; actual line 608/0/52 | met under my ruling, confirmed by Rowan (the 12 literal cases are unchanged from before; see "Recorded") | `A/out/counts.json`, `A/out/fails.json`; `A/shots/webkit-after-unreachable-390-200pc.png` |
| On a shared line: centres within 1 px, with vertical overlap | centred 572/0/88 | centred 608/0/52 (Δ centre 0.00); the overlap check counts the same as the literal row above (608/12/40) | met | `A/out/counts.json`; `C/catalog/offices-wrap-phone.png` |
| The original gap is kept: the row gap of 1rem outside compact (16/20/32 px by root, equal to before in every case); the group gap inside (0.5rem: 8, 10, 16 and 20 px at 16 and 20 px roots, 200% or 32 px, and a 40 px root) | 572/0/88 | 608/0/52; at 100% chevron offset (0,0) in all 90 pairs | met | `A/out/counts.json`; `C/out/diff-sweep.json`, `C/out/C-sweepgeom.log` |
| `ml-2` outside the 21rem compact boundary, 0 inside | 418/242: before has no margin anywhere, so it fails outside compact by design (the row gap carried the space) | 660/0: 0 inside compact, 0.5rem outside (8/16/10/16/20 px at 16 px/200%/20/32/40 px roots) | met | `A/out/counts.json`; values from `A/out/threshold-{webkit,chromium}.json` |
| Compact threshold flips at 21rem at 16 px, 200%, 20, 32 and 40 px roots | flips 5/5 per engine | flips 5/5 per engine (at 336/672/420/672/840 px containers) | met (kept) | `A/out-threshold-*.log` |
| No row overflow | 660/0 | 660/0 | met | `A/out/counts.json` |
| No sideways scroll caused by the rows | rows never overflow | the same; `main` scrolls only for the intro (see "Recorded") and BUG-119 | met | `A/out/raw-*.json` (`mainOver`), `A/out-extra.log` (the overflowing elements) |
| 100% text: no visible change | — | 87 of 90 pairs identical; 3 Chromium pairs differ by 1–2 px at 1/255, outside the rows. A superseded first run found a different set of 9 pairs (up to 14 px, delta 7/255), also outside the rows. That run-to-run variation is why I infer anti-aliasing noise | met | `C/out/C-diff-sweep.log`, `C/out/C-difflocate.log`, `C/out/run1/` |
| Contrast (§18) unchanged | pass | pass, 0 changed; lowest after values: pill text 8.41:1, pill icon 4.29:1, Retry 13.38:1, chevron 5.64:1 | met | `C/out/C-contrast.log` |
| BUG-126: one focus indicator per row | one 2 px indigo-400 outline at −2 px, no native outline, 6/6 configurations | identical | met (kept) | `B/out/focus/*.json` |
| The row's focus outline isn't cut now that the chevron is in the group | all four edges fully painted | identical | met | `B/shots/focus/wk320r40-after-unreachable-row01.png` |
| BUG-127: order pill, Retry, chevron; Retry not inside `aria-hidden`; chevron `aria-hidden` | met | identical | met (kept) | `B/out/aria/facts-*.json` |
| Keyboard order and names | Menu (320), then "HQ, HQ, Can't reach this office, Unreachable, 7 agents, opens details", then "Retry reaching HQ" | identical | met | `B/out-focus-*.log` |
| Accessibility tree | — | 0 differences in 36 before/after pairs per engine (72 snapshots) | met | `B/out/aria/*.yaml` |
| Retry: Enter, Space and pointer work; `aria-busy` while "Retrying…"; keyboard keeps focus | met | identical (pointer in WebKit: P2, BUG-166) | met | `B/out/focus/*.json` |
| The details sheet opens by keyboard and pointer, and focus returns to the row | 6/6 | 6/6 | met | same |
| Live status changes keep focus | met | identical | met | same |
| BUG-81, BUG-82, BUG-91, BUG-92, N3, N5 | met | identical | met (kept) | `B/out/aria/facts-*.json` |
| N4: end alignment below 640 outside compact | the pill and chevron end-aligned | the whole group end-aligned (line-end check 660/0 covers every width) | met (kept) | `A/out/counts.json` |
| N6 rule: the pill keeps its full word; a chevron that drops sits end-aligned inside the row | met, apart from BUG-119 truncation | the same; every lone chevron end-aligned, 0 row overflow | met (kept) | `A/out/counts.json`; `A/shots/webkit-after-unreachable-390-200pc.png` |
| Counts, §17 focus and §20 large text | counts unchanged in every state; focus as in the rows above | identical; §20: no row overflow, whole pill word apart from BUG-119 | met (kept) | `B/out/aria/facts-*.json`, `A/out/counts.json` |
| Design-system §6 (rows), §7 (status pill), §22 (icons) | as validated | unchanged: the pill, icon and chevron styles are the same; only the chevron's place in the DOM and its margin changed | met | `C/out/C-contrast.log`, product diff |
| 12 states plus the Retrying countdown, 3 sizes, 2 builds, 2 engines: no console or page errors; blitz 3 runs per build and engine | 0 | 0 | met | `B/out-aria-*.log`, `B/out-blitz-*.log` |
| BUG-119 (960 and 1100 px at a 40 px root): recorded, not counted | `main` scrolls 142 and 9 px | the same | excluded (known) | `A/out/raw-*.json` (`mainOver`), `A/out/fails.json` |

## Findings (verified on the screenshots myself)

None are new in 34ed1b29.

| Id | Sev | Where | What I saw | Fix (outcome) |
| --- | --- | --- | --- | --- |
| P2 (BUG-166) | S4, pre-existing | `/offices?mock=unreachable`, Retry activated by pointer after it has focus, WebKit only, every size, both builds | Focus moves to `body` in every sample but one per run (90–91 of 91–92 across the WebKit runs); Chromium keeps it on Retry. "Retrying…" and `aria-busy` still show. I infer it's WebKit's macOS rule that a click doesn't focus a button. This contradicts `validation-offices-followup.md`, which recorded pointer Retry as keeping focus in 10 of 10 configurations; I infer that check sampled focus differently (for example, after a keyboard focus that the click didn't move), and this measurement supersedes it `B/out/focus/wk*-{before,after}.json` (`retry.pointer.focusLost`). I rate it S4, not S3 like BUG-59 and BUG-23: a pointer user's next action doesn't depend on Retry's focus, and the keyboard path keeps it | After Retry is clicked, focus stays on Retry (or moves to the row), never `body`. |

Recorded, not counted:
- **P1, known:** the shell's "Menu" button at 320 px has a 2 px indigo-500 (`rgb(97,95,255)`) focus outline, not §17's indigo-400, in both engines and builds. That's A-OF1 P4, already recorded as a shell follow-up (`validation-offices-followup.md`).
- **Not BUG-110:** the brief lists BUG-110 as known, but in ada's catalog BUG-110 is a WebKit create-org setup flake in offices.spec.ts, not a focus issue, so P2 is a separate item.
- **Not re-checked:** the follow-up's P3 (in the error state at 320/200%, a focused row is half hidden under the sticky top bar) stays open; this validation didn't cover it.
- **Chevron alone under a full line.** At 390/200%, 390 with a 32 px root, 640 with a 40 px root, 960/200% and 960 with a 32 px root, the pill and Retry fill line 1, and the chevron sits alone on line 2, end-aligned (5 Unreachable configurations plus Retrying at 1280 with a 40 px root, in both engines: 12 cases). Before was the same. A literal reading of "Retry + gap + chevron fits" would ask for the pill to wrap alone so that Retry and the chevron share line 2. My ruling: "fits" means on the line Retry actually sits on. A lone end-aligned chevron is the N6 rule. Keeping the pill and Retry together keeps the status and its action on one line, where they fit. Rowan, who owns the pin, confirmed this reading (01M783JY1F): the pin forbids a forced extra line and requires the chevron last and end-aligned, but doesn't ask the pill to wrap early. No row. `A/shots/webkit-after-unreachable-390-200pc.png`.
- **The intro scrolls sideways at a 320 px viewport with a 40 px root:** 66 px, every state, both builds. That's the page intro, not a row. It's already open as follow-up P2 in `validation-offices-followup.md`: S3, pre-existing, the BUG-112 family (A-OF1 P1). It's unchanged by this merge.
- **WebKit after a pointer close of the details sheet:** focus returns to the row, but no ring shows, because `:focus-visible` isn't set. That's the same in both builds, and the same family as BUG-148.
- **Sub-pixel overshoots:** at most 0.02 px past the content end in several configurations, for example the pill at 320/200% (304.01 against 304, `C/out/C-largegeom.log`), and the pill or chevron at 640, 960 and 1100 with 200% or a 32 px root (`A/out/raw-*.json`). They're invisible and within the 1 px tolerance.

## Evidence

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| A: geometry matrix | `A/` | `scripts/A-{lib,collect,analyse,threshold,extra}.mjs`; `out/raw-*.json`, `out/fails.json`, `out/counts.json`, `out/threshold-*.json`; 22 shots | `DONE collect webkit: 600 measurements, 0 errors`; `DONE collect chromium: 600 measurements, 0 errors`; `DONE threshold webkit: 10 flip, 0 no flip`; `DONE threshold chromium: 10 flip, 0 no flip`; `DONE extra`; `DONE analyse: 1200 measurements; failures before 462, after 44`; superseded trial `out-smoke.log`: `DONE collect webkit: 49 measurements, 10 errors` (a wrong wait for the shared-machine state) |
| B: behaviour, accessibility and kept | `B/` | `scripts/B-{lib,focus,aria,blitz,debug}.mjs`; `out/{focus,aria,blitz}/`; `shots/` | `DONE focus: 6 runs, 0 fatal, 0 console/page errors` (WebKit and Chromium); `DONE aria: 78 cases, tree differences before/after 0, after problems 0, console/page errors 0` (both); `DONE blitz webkit: 6 runs, 0 console/page errors, 0 runs with a failed phase`; `DONE blitz chromium: 6 runs, 0 console/page errors, 0 runs with a failed phase` |
| C: visual sweep, contrast, catalog | `C/` | `scripts/C-*.mjs`; `shots/{sweep,large,contrast,crops-sweep,crops-large}/`; `out/`; `catalog/` (4 shots) | `C-shots webkit sweep done: 90 shots, 0 failed`; `C-shots chromium sweep done: 90 shots, 0 failed`; `C-shots webkit large done: 12 shots, 0 failed`; `C-shots chromium large done: 12 shots, 0 failed`; `C-diff sweep done: 90 pairs, 3 differ, 0 errors`; `C-diff large done: 12 pairs, 6 differ, 0 errors` (the 6 are the intended Unreachable reflow); `C-difflocate done: 3 pairs, max delta overall 1`; `C-band webkit-1100r40-unreachable done: 1 differing lines above y=1420`; `C-largegeom done`; `C-sweepgeom done: 20 groups`; `C-contrast done: 56 measurements, 0 below threshold, 0 changed before->after`; `C-dom done`; `C-catalog done: 4 shots, 0 console/page errors`; first smoke `C-shots webkit large done: 6 shots, 0 failed` (its shots were superseded); superseded first run in `out/run1/` (viewport-only captures, so its large shots missed the reflow below 800 px): `C-diff sweep done: 90 pairs, 9 differ, 0 errors`, `C-difflocate done: 9 pairs, max delta overall 7`, `C-diff large done: 12 pairs, 0 differ, 0 errors` (its four `C-shots` lines match the final run's) |

- **A's 44 after failures** are the 12 literal-reading cases (counted twice: once in the "fits" row and once in the overlap check) and 20 intro-scroll pages at 320 px with a 40 px root. None is a row-geometry failure under my ruling.
- **The intro and BUG-119 scroll figures** (66, 142 and 9 px) are `mainOver` in `A/out/raw-*.json`.
- **C's band check** found one differing pixel row at y=841 (a single pixel at x=86, in the sidebar) above the intended reflow at 1100 with a 40 px root. It's outside the office rows; I count it with the anti-aliasing noise.
- **Cleanup:** all three checks stopped their servers and removed their worktrees. `git worktree list` in the product repo shows none under `b150`.
