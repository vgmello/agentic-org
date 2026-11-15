# Validation · Offices follow-up BUG-126/127/128 (v1 29530a93 vs cc83d8be)

Requested by ada (01M77JYC). This is Avery's avery/offices-followup, merged as 29530a93 (Tier B, fresh Sol zero). It fixes the A-OF1 findings N1–N5 (`validation-a-of1.md`) under Rowan's pin of 12 Nov (01M77C80). I compared it with the first parent, cc83d8be. Checked 12 Nov 2026.

**Verdict: BUG-126, BUG-127 and BUG-128 are met in WebKit and Chromium, and everything that was kept still holds. I found one new S4 (N1, fix pinned by Rowan) and three pre-existing items.**
- **BUG-126:** each row now shows one focus indicator, §17's indigo-400 2 px outline inset by 2 px. The button's own outline is `none`. That held in 60 of 60 focus walks per engine; every one of the 60 showed two indicators before. The 5 probing runs per engine have no walk.
- **BUG-127:**
  - The order is pill › Retry › chevron at every width.
  - The chevron is the last item and ends flush with the row: end gap 0 in all 100 after-build layout cases per engine (120 rows). Before, on the Unreachable row it sat 64.5 px from the end at 640–1280 (80.1 with a 20 px root, 127 at 1280/200%), and every row's chevron sat 193.7–249.3 px from the end at 390/100%.
  - The chevron is outside the `aria-hidden` status span, and Retry has no `aria-hidden` ancestor.
  - Retry still works by keyboard and pointer, and it keeps focus through "Retrying…".
- **BUG-128:**
  - **N3:** no metadata line starts or ends with "·", and "7 agents" never splits (before, 34 of 65 runs had a line starting or ending with a dot, and one more split "7 / agents"). The fix removes the dot at every width, wrapped or not; the facts are now separated by an 8 px gap. I accept that (see the note under the findings).
  - **N4:** at 390/100% the pill and chevron end-align, as in compact mode.
  - **N5:** in compact layouts the header text and the row text share one left edge. That's 16 = 16 at 320/200% and 732 = 732 at 960 with a 40 px root, with 0 mismatches (45 before).
  - **N6 rule:** wherever the chevron drops to its own line (18 rows per engine), it is end-aligned and inside the row. It never drops when it would fit beside the pill. The exception is the wrapped Unreachable row (N1), where it takes its own line although it would fit beside Retry.
- **Kept:**
  - The compact threshold now flips at a container width of exactly 21rem (336, 672 and 840 px at 16, 32 and 40 px roots), with 0 wrong flips in 267 steps per engine.
  - The accessibility tree is identical before and after (25 of 25 snapshots).
  - No pill is clipped, and every ellipsis has a `title`.
  - The pill icon stays on its word's line at 390 and 960 × 200% (BUG-92, 12 of 12).
  - Names, the countdown, the sheet, focus return, counts, live status and all 18 states hold.
  - Contrast has no value below its threshold.
  - The blitz showed no new errors.
- **Improved:** at 320/200%, `main` no longer scrolls sideways (8 px before, A-OF1 P1).

**New:** one S4 (N1). When an Unreachable row wraps at the largest text, the pill and Retry now sit at the start of the row, while every other compact row end-aligns its pill. Before, the pill was end-aligned. Rowan has pinned the fix (01M77Q5JF1): the group wraps as one end-aligned flow, and the chevron shares Retry's line when it fits.

**Pre-existing:**
- At a 320 px viewport with a 40 px root, long statuses still truncate, and the header intro scrolls sideways.
- In the error state at 320/200%, in both engines, a focused row is half hidden under the sticky top bar.

**Known and excluded:**
- BUG-119 (960 with a 40 px root).
- BUG-110 (WebKit Retry negative).
- The WebKit aborted-load "Importing a module script failed" artefact during the blitz (5 per run, the same before).

**Method:**
- **Requirements:**
  - ada's request;
  - BUG-126, BUG-127 and BUG-128 (N3–N5, plus Rowan's N6 rule);
  - the A-OF1 pins that must be kept (BUG-81, -82, -91 and -92, and the rem threshold);
  - design-system §6, §7, §17, §18, §20 and §22.
- **Three parallel checks**, each on its own before and after worktrees and dev servers (`--mode e2e`), in WebKit (the Tauri engine) and Chromium:
  - **A: requirements and states.** The focus walk, order and accessibility, N3, Retry, the 18 states and the blitz. The checks ran at five configurations: 1280, 640 and 390 at 100%, 320/200%, and 960 with a 40 px root, giving 65 runs per engine (5 configurations × 13 states). The 18 behaviour states ran at 1280 and 390 in both engines, and at 320/200% in WebKit.
  - **B: layout matrix.** 320, 390, 640, 960 and 1280 at 100% and 200%, with 16, 20, 32 and 40 px roots, across five statuses. Plus a threshold sweep in 8 px steps and a stress case with long names and hosts.
  - **C: accessibility.** The accessibility tree, the keyboard walk, focus return, contrast measured from pixels and from computed values, and the blitz.
- **BUG-92 re-measure:** none of the three checks measured BUG-92, so I added a fourth measurement (`D/`). It covers icon and word on the after build at 390 and 960 × 200%, in both engines.
- **Roots:** a real WebKit 20 px default can't be launched, so the 20 and 40 px roots are set on the computed root.
- **Keyboard:** WebKit was walked with Option+Tab.

## Requirements (evidence I opened)

Paths are relative to the scratch folder below.

| Requirement | Before (cc83d8be) | After (29530a93) | Result | Evidence |
| --- | --- | --- | --- | --- |
| BUG-126: one focus indicator per row, §17 | Two in 60 of 60 walks per engine: the button's own outline (`auto 3px` in WebKit, `auto 1px` in Chromium) plus the span's indigo-400 | One in 60 of 60 per engine: the span's solid 2 px `oklch(0.673 0.182 276.935)` at −2 px. The button's outline is `none`, with no focus box-shadow or pseudo-element | met | `A/out/check-*.json`; `A/shots/check/before-webkit-1280-online-row0-focus.png` vs `after-webkit-1280-online-row0-focus.png`; `C/out/kbd/*.json` (8 of 8 configurations) |
| BUG-127: pill › Retry › chevron, chevron last and end-aligned | DOM order pill, chevron, Retry. The Unreachable chevron is 64.5 px from the end at 640–1280 (80.1 with a 20 px root, 127 at 1280/200%). It isn't last in 11 cases and isn't end-aligned in 12 | DOM and x order pill, Retry, chevron. Chevron end gap 0 in all 100 after cases per engine (120 rows) | met | `A/out/check-*.json` "order"; `B/out/matrix-{webkit,chromium}.json` |
| BUG-127: chevron outside `aria-hidden`; Retry never under one | The chevron is inside the hidden status span in all 95 flagged before cases outside the BUG-119 set | The chevron is a sibling and the last child, `aria-hidden=true` itself. Retry has no hidden ancestor and is in the tree for unreachable and count error | met | `C/out/aria/dom-*.json`, `C/out/aria/*-unreachable.yaml`, `*-error.yaml` |
| BUG-127: on wrap, pill and Retry together, chevron alone | At 320/200% Retry dropped below the chevron | The pill and Retry form one group, and the chevron sits alone on its own line, end-aligned, with nothing overflowing. Retry wraps under the pill only where the two can't share a line (see N1) | met, with N1 | `B/out/probe.json`; `B/shots/probe/*-after-retry-split-*.png` |
| BUG-127: Retry works | yes | Keyboard (Tab, then Enter) and pointer show "Retrying…", keep focus and open no dialog, in 10 of 10 configurations | met | `A/out/check-*.json` "retry"; `C/out/kbd/*.json` |
| N3: no orphan "·"; facts stay whole | 35 N3 failures in 65 runs per engine: 34 had a line starting or ending with "·" (for example "This Mac (you're here) ·" while counting). The 35th, at 320/200%, split the count: "It isn't running · 7 / agents" | 0 lines start or end with "·". The dot is removed at every width, and the facts are separated by an 8 px gap. "7 agents" is on one line everywhere, including long reasons and the shared HQ machine | met | `A/out/check-*.json` "meta"; `C/out/aria/dom-*.json` (0 separator dots) |
| N4: below 640 outside compact, pill and chevron end-aligned | The chevron is 193.7–249.3 px from the end at 390/100% | Chevron 0 px from the end, 16 px from the pill | met | `B/out/matrix-*.json` (390, 100%); `B/shots/reps/*-390-100pc-default.png`; the catalog phone view |
| N5: compact header and rows share one left edge | 45 mismatches outside the BUG-119 set (for example 32 vs 16 px at 320/200% and 40 vs 16 px at 320/40 px), plus 780 vs 732 px in the 960/40 px set | 0 mismatches: 16 = 16 and 732 = 732 | met | `B/out/matrix-*.json` (`n5-edge` flags) |
| N6 rule: full word; if it doesn't fit, chevron on its own line, end-aligned, inside the row, no overflow | n/a | 18 own-line chevrons per engine: 0 not end-aligned, 0 outside the row, 0 row overflow. 0 dropped when they would fit beside the pill; the one exception is beside Retry (N1). The full word holds at 320/200%, but not at a 320 px viewport with a 40 px root (P1) | met (N1; P1 pre-existing) | `B/out/matrix-*.json` |
| Rem compact threshold (`@container (width < 21rem)`) at 16, 32 and 40 px | n/a | 267 steps per engine: 0 wrong flips, flipping at a container of exactly 336, 672 and 840 px. In 142 of the steps a viewport rule would have given the other answer | met | `B/out/threshold.json` |
| BUG-91: no clipped pill; ellipsis keeps the full text | 0 clipped | 0 clipped. Every ellipsis has `title` and keeps the status in the row's accessible name | met | `B/out/matrix-*.json`, `B/out/stress.json` |
| BUG-92: the pill icon stays on its word's line at 390 and 960 × 200% | not re-measured | 12 of 12 (Update needed, Status unknown, Unreachable, both engines): icon and label centres 0 px apart, pill height equals one line (48 px), `white-space: nowrap`, icon `flex-shrink: 0`. The badge component is unchanged by this merge | met | `D/bug92.json`; `D/shots/webkit-390-200-unknown.png` |
| Kept: BUG-81 name, BUG-82 countdown, sheet, focus return, counts, live status, states | holds | "HQ, HQ, This Mac (you're here), Online, 7 agents, opens details" (and "This Mac (you're here)" on the shared machine too). The countdown label matches the visible text at 45 and 60 s. Focus returns after Escape and Close (16 of 16). The 18 states pass: probing, online, details sheet, slow, count error and its Retry, empty, unreachable, unknown reason, Status unknown, restarting, restart not ready, restart stuck, live updates, countdown 45 and 60 s, HQ on a shared machine, and named HQ | met | `A/out/behaviour-*.json`; `C/out/kbd/*.json` |
| Accessibility tree | n/a | 25 of 25 snapshot pairs identical once probing settles (C: 52 pairs, also identical) | met | `A/out/aria/`, `C/out/aria/` |
| Contrast (§18) | 0 below threshold | 0 below threshold. Lowest: text 5.64, pill icon 4.28, chevron 5.64, outline 4.69. Pixel and computed values agree within 0.6 | met | `C/out/contrast/*.json`, `C/out/contrast-*.log` |
| Blitz | 0 errors in Chromium; 5 per run in WebKit (aborted-load artefact) | The same | met | `A/out/blitz.json`, `C/out/blitz-*.log` |
| BUG-119 expected set (960, 40 px root) | `main` scrolls 190 px sideways | 142 px (the intro and Add office). Unreachable shows "Unrea…", and Status unknown and Update needed truncate, each with `title`. The truncated pill fills the row, and Retry wraps under it as before. The chevron is on its own line, end-aligned | known, unchanged in kind. With a 40 px root, the same regime runs to a 1176 px viewport (the list is the viewport minus the 720 px sidebar). Up to 1104 px, `main` scrolls sideways (142 down to 5 px), and from 1024 to 1176 px the Unreachable pill and Retry split onto two lines (`pill-retry-split`). I infer that's N1's start-aligned wrap from the same CSS, without having measured it | `B/shots/probe/*-after-bug119-*.png`; `B/out/threshold.json` |

## Findings (verified on the screenshots myself)

| Id | Sev | Where | What I saw | Fix (outcome) |
| --- | --- | --- | --- | --- |
| N1 | S4, new (alignment) | `/offices?mock=unreachable` at 320/200%, and at 320 and 390 with a 40 px root. Both engines. Probably also at 1024–1176 px with a 40 px root, where BUG-119's sidebar leaves a 304–456 px list: the sweep flags the same split there, but I didn't measure the alignment or see the row at those widths. Rowan has added 1100 px with a 40 px root to the fix's test (01M77QYQYN) | The pill (217.9 px), the 16 px gap and Retry (95 px) need 328.9 px, and the row has 288 (410.5 against 351 at 390 with a 40 px root). So Retry wraps under the pill. The order is right. But the pill and Retry now sit at the start of the row: the pill ends 70.1 px short of the end at 320/200% (15.7 at 320/40 px, 78.7 at 390/40 px), and Retry 193 px short. Every other compact row end-aligns its pill. Before, the pill ended flush (0). The chevron also takes a third line on its own, although it would fit beside Retry (Retry ends at 111, the chevron starts at 264). `B/shots/probe/webkit-after-retry-split-320-200pc.png` vs `webkit-before-retry-split-320-200pc.png` | **Pinned by Rowan (01M77Q5JF1), as I proposed.** The pill › Retry › chevron group wraps as one end-aligned flow, with no start-aligned fallback. The chevron stays last, outside `aria-hidden`, ends at the row end, and shares Retry's line whenever it fits there, never forcing a third line. Tests at 320/200% and at 320 and 390 with a 40 px root (±1): every wrapped line ends at the row content end; the chevron's top equals Retry's when it fits; the pill and Retry show their full text. A `justify-content: flex-start` mutant must fail. Tier B. |
| P1 | S4, pre-existing (BUG-119 family) | At a 320 px viewport with a 40 px root (compact), Status unknown and Update needed rows, both engines | The pill truncates with an ellipsis: the label gets 198 px against a natural width of 231 or 220 px. `title` and the accessible name stay intact, and the chevron is on its own line. It's identical before. At 320/200% the full word fits | Fold into BUG-119's expected set: its large-text test should also cover a 320 px viewport with a 40 px root, and the fix should keep the full word there (N6 rule). |
| P2 | S3, pre-existing, reduced (A-OF1 P1; BUG-112 family) | `/offices` header at a 320 px viewport with a 40 px root, every state, both engines | `main` scrolls sideways by 66 px (90 before). The intro block's `max-w-xl` is wider than the column, and the Sample data note wraps one word per line. At 320/200% it's now 0 (8 before). `B/shots/probe/webkit-after-overflow-320-40px.png` | The intro, the note and Add office stay inside the column at any root size, with no sideways scroll (§20). That's the same outcome as BUG-112 for Projects. |
| P3 | S4, pre-existing | `/offices?org=fails` (count error) at 320/200%, both engines; Shift+Tab from the count-error Retry back to the row | The focused row scrolls into view under the sticky top bar: 144 of its 280 px are covered, including the name and the top of the outline. Focus is only partly hidden, so it's still visible (WCAG 2.4.11 is met; 2.4.12 is not). Before, 144 of 289 px. `C/shots/obscured-wk320x200-after-error.png` | A focused control is never hidden under the sticky top bar. For example, the scroll container gets a `scroll-padding-top` equal to the bar's height (§17). |

Not findings:
- **The metadata dot is gone at every width (N3's fix).** A-OF1 only asked that no line start or end with it. Other meta lines in the design system join facts with " · " (§5 count lines, the Home "Working on {task} · {project}" line). I accept the gap-only separation for office rows: their meta is a short location and a count, the 8 px gap reads clearly at 1280 and 390 (`A/shots/check/after-webkit-1280-unreachable-row0-focus.png`, `B/shots/reps/webkit-after-390-100pc-default.png`), and it can't orphan when it wraps. I'll note the variant in design-system §6.
- **"Add office" sits indented from the left edge at 390/100%,** before and after (the catalog phone view). It's pre-existing and outside this change, so I'll look at it with the header follow-up (P2).
- **The shell's Menu button and sidebar items use an indigo-500 outline.** This is A-OF1 P4, already recorded as a shell follow-up.
- **The count-error "Checking…" spinner icon** couldn't be measured from pixels while it spins (4 "SHOT FAILED" per engine, before and after). Every other icon was measured.
- **The probing state** turns Online in under a second, so its 5 runs per engine have no focus walk. The same row was walked in the other states.
- **The obscured probe's default-state rows** (320/200% and 960/40 px) report the focused row as fully covered, before and after. That's an artefact: in that state the row is the last focusable control in `main`, so the probe's starting `focus({ preventScroll: true })` lands on it directly, and it is never scrolled into view. Only the error-state measurement (P3) is a real Shift+Tab walk.

## Evidence

Scratch: `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/of2/`.

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| Requirements and states | `A/` | `scripts/{lib,check,behaviour,geom,blitz}.mjs`; `out/check-*`, `out/behaviour-*`, `out/geom.json`, `out/blitz.*`, `out/aria/` (50); 716 shots; 2 catalog shots | See below |
| Layout matrix | `B/` | `scripts/{common,matrix,threshold,stress,probe,reps}.mjs`; `out/{matrix-webkit,matrix-chromium,threshold,stress,probe}.json`; `summary.txt`; 350 shots | See below |
| Accessibility | `C/` | `scripts/{lib,aria,keyboard,contrast,blitz,wrapprobe,obscured}.mjs`; `out/aria/` (124), `out/kbd/`, `out/contrast/`, 25 logs; 536 keyboard shots, 56 contrast shots | See below |
| BUG-92 re-measure | `D/` | `scripts/bug92.mjs`, `bug92.json`, `run.log` (regenerated from `bug92.json`, because the console output wasn't saved), 12 shots; built from a detached worktree at 29530a93 (port 5391) | `DONE bug92: 12 cases, 0 failed` |

**A (requirements and states)**
- After: `DONE check webkit after: 65 runs, 0 errors, focus not-ok 0, N3 not-ok 0, BUG-127 not-ok 2, retry not-ok 0, console errors 0`. Chromium gives the same line.
  - The 2 BUG-127 not-ok cases come from a strict same-line check on the pill and Retry. One is N1 at 320/200%; the other is the BUG-119 case at 960/40 px.
- Before: `… focus not-ok 60, N3 not-ok 35, BUG-127 not-ok 5 …`.
- Behaviour: "18 states, 0 failed" in every configuration.
- `DONE geom: 24 cases`. This is the script's console output; no log file was saved, and `out/geom.json` holds the 24 entries.
- `DONE blitz: 10 runs, 0 failed, 30 console/page errors; catalog 2 shots`. All 30 are the WebKit aborted-load artefact, the same before.

**B (layout matrix)**
- `DONE webkit: 200 cases, 112 flagged (before 100, after 12; of which 960/40px before 5, after 5)`. Chromium gives the same line.
  - That's 100 before and 100 after cases per engine.
  - The 7 after cases flagged outside BUG-119 per engine are P2 (5), P1 (2) and N1 (3). Some cases carry more than one flag.
- `DONE threshold: 534 steps, 0 wrong flips, 284 steps where container and viewport rules disagree, 56 flagged`.
  - The 56 are 28 per engine, all with a 40 px root at 960–1176 px, where the 720 px rem sidebar leaves a 240–456 px list. That's BUG-119's regime.
  - Each of the 28 has the Unreachable pill and Retry on separate lines (`pill-retry-split`).
  - Up to 1104 px, `main` also scrolls sideways (142 down to 5 px).
  - Up to 1016 px, the pill also truncates.
- `DONE stress: 40 cases, 32 flagged (before 20, after 12; 320/200% after 2)`.
- `DONE probe: 32 records`.

**C (accessibility)**
- **Keyboard:** `keyboard done: 4 runs` in each of `kbd-wk`, `kbd-wk960`, `kbd-cr`, `kbd-cr960`, `kbd2-cr` and `kbd2-cr960`, and `keyboard done: 8 runs` in `kbd2-wk`.
- **Aria:** `aria done: 22 snapshots` in each first run (wk/cr × 1280 and 320x200).
  - Each first run also logged `FAIL … restartStuckSheet … strict mode violation: getByText('Restarting…') resolved to 2 elements`, before and after. That's a selector clash in my script, and the `-stuck` re-runs (`aria done: 2 snapshots` ×4) cover the state.
  - The settled re-runs give `aria done: 6 snapshots` ×4.
- **Contrast:** `contrast done: 352 measurements, 4 below threshold, 0 pixel/computed deviations > 0.6` ×2. The 4 are the spinner's failed screenshots, not contrast failures.
- **Blitz:** `blitz done: 2 runs, 0 console/page errors` ×2.
- `wrapprobe done`; `obscured done`.

**My own checks**
- I recomputed N1's widths from `B/out/probe.json`:
  - At 320/200%: the pill is 217.9 px, the gap 16 px and Retry 95 px, which needs 328.9 px against 288.
  - The pill ends at 233.9 and the row content at 304.
  - Before, the pill ended at 304.
- I took P1's values (198 against 231 and 220, identical before and after) and the N4 before-gaps from `B/out/matrix-*.json`.
- Each worktree got its own `bun install --frozen-lockfile`. All servers are stopped and all worktrees removed. Nothing in the product repo was edited.
