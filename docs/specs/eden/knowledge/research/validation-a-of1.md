# Validation · A-OF1 Offices rows (v1 64a68813 vs 3c6557a4)

Requested by ada (01M77743). This is Avery's avery/offices-rows, merged as 64a68813 (Tier B, fresh Sol zero, exact-head gate PASS). Scope:
- OF-2, OF-3, OF-4 and OF-5;
- BUG-81, BUG-82, BUG-91 and BUG-92;
- The pinned decisions in Avery's `contract-pin.json`, each rated in its own table below.

Checked 12 Nov 2026.

**Verdict: met for OF-2 to OF-5 and BUG-81/82/91/92, with two new S3s.** Of the eleven pinned decisions (Rowan's, with `scope` shared with ada and `bug110`/`font_proof` ada's), three UI pins are only partly or not met (see "Pinned decisions"). Rowan ruled on the open questions on 12 Nov (01M77C80), and the ratings below include his rulings:
- `extreme_layout` is **not met**: at 960 px with a 40 px root, long statuses truncate and the page scrolls sideways. This is blocked by BUG-119; Rowan confirmed the deferral, with no further A-OF1 change.
- `rem_layout` is partly met: the chevron isn't last when Retry shows (N2), and the gutter cap doesn't reach the page header (N5). Rowan accepted the chevron dropping to its own line (N6, withdrawn).
- `ellipsis_scope` is partly met: the ellipsis also appears at the extreme, where the full text was pinned.
- `host_presentation` is met. Rowan confirmed it covers the repair prose only; the row meta follows §6 (sans with `overflow-wrap:anywhere`), as built.

What works:
- The rows are flat and divided, and the square HQ tile is gone.
- Unplaced is a labelled section with a quiet Callout, and amber appears only on its icon.
- The disabled actions are outline buttons, each with one gray-400 reason.
- BUG-81 and BUG-82 are fixed: the names say "This Mac (you're here)", and the countdown label matches what's shown.
- BUG-91 and BUG-92 are fixed: no pill is clipped, and no icon wraps above its word.
- The compact rows switch on a rem threshold (`@container (width < 21rem)`) at 16, 32 and 40 px roots, not on viewport width.
- Text and icon contrast now pass everywhere, and the old disabled-primary failures are gone. Retry's and Close's focus rings still fail (P2, BUG-102).
- The details sheet, Retry, focus return, the counts and live status are all unchanged.

**New S3s:**
- The office row draws two focus rings.
- The chevron sits between the pill and Retry instead of trailing.

**New S4s:**
- an orphan "·" when the meta line wraps;
- the pill and chevron aren't end-aligned below 640 outside compact mode;
- the header and row gutters don't line up in compact layouts.

(N6, the compact chevron dropping to its own line, was withdrawn after Rowan's ruling.)

**Pre-existing:**
- At 320/200% the page header overflows by 8 px. It was 40 px before.
- Retry and Close use the faint shadcn ring (BUG-102).
- At a 40 px root on 960, long statuses truncate instead of showing in full. That's the shell problem in BUG-119.
- The shell sidebar focus is indigo-500, and the page has no skip link.

**Method:**
- Requirements:
  - my gap sheet rows OF-2 to OF-5 (`bug-70-per-screen-gaps.md`);
  - BUG-81, BUG-82, BUG-91 and BUG-92;
  - every decision pinned in Avery's `contract-pin.json` (eleven keys: Rowan's UI pins, `scope` shared with ada, and ada's `bug110` and `font_proof`);
  - design-system §6, §7, §9, §10, §17, §18, §20 and §22.
- Builds: before is 3c6557a4 and after is 64a68813. Each ran as its own dev server in e2e mode with sample data ("Acme").
- Three parallel checks:
  - Requirements, copy and behaviour: WebKit and Chromium at 1280×800 and 390×844, 18 states each, 144 state runs.
  - Layout, reflow and compact rows:
    - 2 engines × 320/390/640/960/1280 × 100%/200%, plus 20/40 px roots × 2 builds = 112 cases per engine;
    - 52 threshold cases;
    - 160 stress cases with long names and hosts.
  - Accessibility, keyboard, blitz and catalog: WebKit at 1280/100 and 640/200, plus Chromium at 1280/100, with 13 states compared in the accessibility tree.
- 200% text means the root font size set to 200%. A 20 px or 40 px root is the computed root font size: WebKit can't launch with a real 20 px default, as ada noted.
- WebKit keyboard walks use Option+Tab.

## Requirements (evidence I opened)

| Req | Before (3c6557a4) | After (64a68813) | Result |
| --- | --- | --- | --- |
| OF-2 flat divided rows | Rows inside a `bg-white/5` card, radius 11 px | Transparent list with dividers. Title "HQ", meta "This Mac (you're here) · 7 agents", then the pill and a `chevron-right` | met (chevron placement: N2, N4) |
| OF-3 no HQ tile | Square indigo "HQ" tile | No tile in any state | met |
| OF-4 Unplaced | Amber box with amber-100 text | Heading "Unplaced · 1 agent" in gray-400. A quiet Callout with no role and no live region. Amber only on the icon (7.77:1) | met |
| OF-5 disabled actions | "Add office" as a disabled indigo primary (2.27:1); amber reason | "Add office" and "Place…" are outline buttons, natively disabled, each with an `aria-describedby` gray-400 reason (5.64:1) | met |
| BUG-81 label in name | Row named "…the office's machine…". The shared machine was named "studio-mac.local" while showing "This Mac (you're here)" | Row "HQ, HQ, This Mac (you're here), Online, 7 agents, opens details", the shared machine included. The sheet's Where field matches | met |
| BUG-82 countdown | Label "restarting shortly" or "at 06:40" beside visible "in 40 s" / "in 60 s" | Label equals the visible text at 40→30 s and 60→50 s (page clock). The live region keeps its stable wording ("restarting shortly" under 60 s, otherwise "at HH:MM"), as designed | met |
| BUG-91 never clipped | Pill clipped with no ellipsis at 320/200% and 960/40 px | 0 clipped, 0 outside the row. Truncation uses an ellipsis, with `title` and the aria-label carrying the full status | met |
| BUG-92 icon stays with word | Icon wraps above the word at 390, 960 and 1280 at 200% and 40 px | The pill stays on one line with the icon inline in every case | met |
| Readable name ≥6ch, whole words | Name 0 px at 960/40 px; "unexpectedly", "components" and "diagnostics" broke mid-word | ≥6ch or the full name, and names never break mid-word in any stress case. The 28 mid-word flags after the change are all the host token, which may wrap anywhere | met |
| States | n/a | Probing ("Checking…" → Online), restarting (countdown), restart not ready and stuck, error (with Retry), slow (skeleton), empty ("0 agents" / "No offices to show."), unreachable ("Retry reaching HQ"), unknown, shared machine. All correct, with 0 console errors in 144 runs | met |
| Kept behaviour | n/a | The sheet opens by click and Enter. Esc and Close return focus to the row (6/6, also after 10 rapid opens). Retry keeps focus through "Retrying…". Counts and live pushes work | met |
| Normal-width guard | n/a | At 1280/100% the pill sizes are identical (72.7, 108.9, 128.3 and 84.3 px × 24, one line). The changes are only the intended OF-2/3 ones | met |
| Contrast, text and icons (§18) | Disabled controls 2.27 and 4.05 | 0 below the threshold in 359 measurements × 3 configurations. Pill icons 4.29–7.22 | met (focus rings: P2) |
| Accessibility tree | n/a | Every difference is intended: the row names (BUG-81/82), the reported host as a `code` node. No other change | met |

## Pinned decisions (`contract-pin.json`, "decisions"; Rowan's unless marked ada)

| Pin | What it asks | After (64a68813) | Result |
| --- | --- | --- | --- |
| `scope` | A-OF1's scope (ada / Rowan) | Process. The change stays within OF-2 to OF-5 and BUG-81/82/91/92 | n/a (process) |
| `icon` | The existing Lucide `ChevronRight` `size-5`, `aria-hidden` | In every row; Lucide stays until OF-1's migration | met |
| `machine_copy` | Shared labels; the repair prose adds the reported host | "This Mac (you're here)" in the row, name and sheet. "This Mac (reported as studio-mac.local)" in the Why field and the repair steps | met |
| `host_presentation` | Mono with break-all; This Mac first; no new Host line in Copy details | The repair prose has a mono `<code>` with break-all, after This Mac. `copyDetails` is unchanged in the diff. The row-meta host of a remote office is sans with `overflow-wrap:anywhere`, as §6 prescribes for meta lines. Rowan confirmed the pin covers the repair prose only (01M77C80) (`B/out/shots/stress/webkit-after-remote-long-host-320-200pc.png`) | met |
| `extreme_layout` | At 960 with a 20 px default at 200% (a 40 px root): the full nowrap pill, then an end-aligned chevron on the same row; list padding capped; whole words and nothing overflowing; normal-width guard | The list padding is capped, whole words hold, and the normal-width guard holds (identical pills at 1280/100%). But long statuses truncate ("Status…", "Unreac…"), the chevron drops to its own line, and `main` scrolls sideways by 190 px. The shell leaves a 240 px list | **not met**: blocked by BUG-119 (P3); accessible through ellipsis, title and name. Rowan confirmed the deferral (01M77C80); the BUG-119 test must cover Unreachable, Status unknown and Update needed |
| `ellipsis_scope` | Full text only at the identified extreme; the BUG-91 ellipsis elsewhere | The ellipsis works elsewhere, with 0 clipped, plus `title` and the name. At the extreme it also truncates | **partly met** (with `extreme_layout`) |
| `named_hq` | Keep the comma-delimited role, normalise separators, prove word order | Visible "Main office · HQ", name "Main office, HQ, This Mac (you're here), …"; a satellite gets no HQ suffix | met |
| `bug110` | The WebKit Retry negative is excluded; full-gate WebKit positives required | Process (Avery's gate). Not re-tested here | n/a (process) |
| `rem_layout` | A rem-derived threshold; names ≥6ch or full; full pill, chevron and end alignment; common gutter cap; threshold checked at 16/32/40 | `@container (width < 21rem)` flips correctly in 24/24 cases at 16/32/40 px roots. Names are ≥6ch, with no mid-word breaks (the 28 stress flags are all the host token). But the chevron isn't last when Retry shows (N2), and the cap leaves out the page header (N5). The chevron dropping to its own line when it nearly fits is accepted (Rowan 01M77C80; N6 withdrawn). At its own 40 px case the full pill fails (P3, as under `extreme_layout`). The "px mutant 40" clause is Avery's test guard and wasn't re-run here | **partly met** |
| `font_proof` | Real Chromium default 20 px, plus WebKit at computed roots of 20/40 px | Process. I used computed 20/40 px roots in both engines, as stated in Method | n/a (process) |
| `font_guard` | No rem media query or new media utility in the compact-row definition; keep the sm/lg gutters | Holds. The fixes for N4 and N5 are written to respect it | met |

## Findings (verified on the screenshots myself)

| Id | Sev | Where | What I saw | Fix |
| --- | --- | --- | --- | --- |
| N1 | S3, new | Office row, keyboard focus, every width, both engines | Two focus rings. The stretched span draws §17's indigo-400 2 px outline, and the row `<button>` also keeps the browser's own outline (`auto 3px` in WebKit, `auto 1px` in Chromium). That puts a second, darker rectangle around the name block. Before, there was one ring. `C/out/kbd/wk1280-after-row-ring-closeup.png` vs `wk1280-before-row-ring-closeup.png` | `outline-none` on the row button (the span carries the ring), plus an e2e assertion that the button's own `outline-style` is `none` when it has focus. |
| N2 | S3, new | Unreachable row, every width, both engines | The order is pill › chevron › Retry, so the chevron sits mid-row (64.5 px from the end at 640–1280). At 390 it reads "Unreachable › Retry", and at 320/200% Retry drops to its own line. §6 and the pin put the chevron last. `A/shots/after-chromium-390-unreachable.png`, `B/out/shots/matrix/webkit-after-1280-100pc-unreachable.png` | Outcome: the chevron is always the trailing item, and Retry stays beside its pill. In compact mode the chevron sits inside the `aria-hidden` status span, which is `w-full` there, so this changes Rowan's "pill then chevron" layout. **Rowan ruled (01M77C80):** pill, Retry, then the chevron last and end-aligned. The chevron moves out of the `aria-hidden` status span, Retry is never inside an `aria-hidden` subtree, and when the row wraps, the pill and Retry stay together and the chevron drops alone. |
| N3 | S4, new | Row meta whenever it wraps (long stop reasons, 320/200%, 960/40 px) | "· 7 agents" starts a line with an orphan dot. `A/shots/after-chromium-1280-hq-shared-machine.png` | Outcome: no line starts or ends with the separator, and each fact ("7 agents") stays whole. One way is to drop the dot when the meta wraps and separate the facts with a gap instead. The mechanism is Avery's, with Rowan. Test: a long stop reason at 1280/100% and 320/200% shows no line beginning with "·". |
| N4 | S4, new | Below 640 outside compact mode (390/100%) | The pill and chevron wrap under the name, aligned to the start, so the chevron is 194–249 px from the row end. Compact mode end-aligns them, so the two modes disagree. `B/out/shots/matrix/webkit-after-390-100pc-default.png` | `ml-auto` on the status group, so it trails whether or not it wraps, as in compact mode. No new media utility (Rowan's font guard). |
| N5 | S4, new | Compact layouts (320/200%, 960/40 px) | Only the list's gutters are capped. The page header keeps `px-4 sm:px-6 lg:px-8` in rem, so the header text and the row text don't line up: 32 vs 16 px at 320/200%, 780 vs 732 px at 960/40 px. | Outcome: in compact layouts the header text and the row text share one left edge, and the sm/lg gutters stay everywhere else (Rowan's font guard). The wrapper can't container-query its own padding, and the list bleeds out with `-mx`, so this needs an outer container or the padding moved onto the children. The mechanism is Avery's, with Rowan. |
| N6 | withdrawn (Rowan 01M77C80) | Compact mode | When the pill, the gap and the chevron are wider than the row, the chevron drops to its own line. At 320/200% (a 288 px content width), Unreachable misses by about 2 px and Status unknown by about 40 px. Update needed also drops at a 32 px root. The pin says "end-aligned chevron within the same row", while Avery's test asserts the next line. `B/out/shots/matrix/webkit-after-320-200pc-statusUnknown.png`, `B/out-threshold.log` | Outcome per the pin: the chevron on the same row as the pill. A smaller gap alone saves only about 16 px, so long statuses need another answer: truncating the pill first, or allowing the own-line fallback. **Rowan ruled (01M77C80):** the pill keeps its full word, and the chevron drops to its own line, end-aligned, inside the same row target. The status is content and the chevron is decoration. `@max-[21rem]:gap-x-2` stays, so common statuses fit on one row first. Not a defect. |
| P1 | S3, pre-existing, reduced | `/offices` header at 320/200% (all states, both engines) | `main` scrolls sideways by 8 px (40 before). The intro paragraph and the "Sample data" note overflow, and the note wraps one word per line beside the badge. This is the same family as BUG-112 (Projects intro). `B/out/shots/page/webkit-after-320-200pc.png` | `min-w-0` on the header's text column, and let the sample-data line wrap under the badge (`flex flex-wrap`). Fix it together with BUG-112. |
| P2 | S3, pre-existing (BUG-102) | Retry ("Retry reaching HQ", the count error) and the sheet's Close | Focus is the shadcn `ring-3` at 50% (about 1.8:1), not §17. `C/out/kbd/*-unreachable-retry-focus.png` | Covered by Jules's BUG-101/102 primitives cut. |
| P3 | S4, follows BUG-119 | 960 px at a 40 px root (beyond the WCAG 200% bar) | The shell sidebar leaves a 240 px list, so long statuses truncate ("Status…", "Unreac…", "Updat…") instead of showing the full pill. The ellipsis, `title` and name keep it accessible. The page also scrolls sideways by 190 px (210 before). `B/out/shots/matrix/webkit-after-960-40px-statusUnknown.png` | Deferred to BUG-119, the shell's narrow layout, with no further A-OF1 change (Rowan 01M77C80). The BUG-119 test must cover Unreachable, Status unknown and Update needed. |
| P4 | S4, pre-existing (shell) | Sidebar items and org switcher | Focus outline is indigo-500, not indigo-400, and there is no "Skip to content" link. | A shell follow-up (J-POLISH). |

Not findings:
- "Restarting in 590 s" for a long wait: the copy is unchanged from before. Minutes would read better; that's a follow-up idea, not a defect.
- During a countdown, the row's name updates every second (a consequence of BUG-82). The live region stays stable, so screen readers aren't spammed. I accept it.
- WebKit logged "Importing a module script failed" only when a navigation was aborted mid-load in the blitz. It was 0 with loads allowed to settle, and the same before the change.
- The 390 "Add office" sits indented (pre-existing). It moves with N5's wrapper fix.
- The 28 mid-word breaks the stress pass flagged after the change are all in the host token, which §6 lets break anywhere. No name breaks mid-word.

## Evidence

Scratch: `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/aof1/`.

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| Requirements, copy, behaviour | `A/` | `results/*.json` (8), 144 shots, `aria-*.yml` | 8 `results/*.json` files × 18 states, all complete, 0 script or console errors |
| Layout, reflow, compact rows | `B/` | `out/matrix-*.json`, `threshold.json`, `stress.json`, 270 shots | `DONE webkit: 112 cases, 31 flagged (before 13, after 18)`; `DONE chromium: 112 cases, 31 flagged (before 13, after 18)`; `DONE threshold: 52 cases, 16 flagged` (all chevron-position flags, 0 wrong flips); `DONE stress: 160 cases, 80 flagged (before 44, after 36)` |
| Accessibility, keyboard, blitz, catalog | `C/` | `out/aria/` (78), `out/kbd/`, `out/contrast/`, `out/blitz/`, 2 catalog shots | `aria done: 26 snapshots` ×3; `keyboard done: 2 runs` ×3; `contrast done: 359 measurements, 20 below threshold` (WebKit and Chromium at 1280) and `14 below threshold` (WebKit 640/200), all of them before the change; after, 0; `blitz done: 2 runs, 0 console/page errors` (Chromium); `blitz done: 2 runs, 22 console/page errors` (WebKit, both configurations: the aborted-navigation artefact, see Not findings); `blitz-settled done: 0 errors`; `catalog done: 2 shots, 0 errors` |

- Each worktree got its own `bun install --frozen-lockfile`.
- All servers are stopped and all worktrees removed. Nothing in the product repo was edited.
