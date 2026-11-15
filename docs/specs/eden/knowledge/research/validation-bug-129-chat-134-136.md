# Validation · BUG-129 View menu focus (v1 5ba5d947) and Chat BUG-134/135/136 (v1 6072bf08)

Requested by ada: BUG-129 by session message on 12 Nov, and the Chat batch in 01M77VGP1G. Checked 12 Nov 2026 in WebKit (the Tauri engine) and Chromium, in five parallel checks. Each check ran on its own before and after worktrees and dev servers (`--mode e2e`). A real WebKit 20 px default can't be launched, so the 20 px and 40 px roots are set on the computed root after load. WebKit was walked with Option+Tab.

- **Part 1:** Avery's avery/bug129-view-focus, merged as 5ba5d947 (Tier B, fresh Sol zero), compared with the first parent f3936708. That parent's UI is identical to 29530a93; the merges in between are Rust only.
- **Part 2:** Jules's jules/chat-large-text, merged as 6072bf08, compared with the first parent 5ba5d947.

## Part 1 · BUG-129: the chart View menu rejects stale focus while closed

**Verdict: BUG-129 is met in both engines for keyboard closes (Escape, selecting an item, Tab), with no regressions. Closing by pointer still leaves focus on `body`, in both builds; that's pre-existing, filed as P3.**
- **Escape and select:**
  - After the fix, Escape and selecting an item return focus to the View button every time: People at 320 px with 16, 20 and 40 px roots and at 1280; Teams and Reporting at 320/20 px and 1280 (10/10 each); plus B's four sizes per tab, once per combination. Focus never ends on `body` or inside the menu.
  - Before, WebKit at 320 px with a 20 px root lost focus to `body` in 26 of 50 Escapes on People, 8 of 10 on Teams and 5 of 10 on Reporting. Selecting failed in 3 of 10 on People, 8 of 10 on Teams and 1 of 10 on Reporting. Chromium never reproduced it.
  - So the bug was wider than People.
- **Closed content:** the closing menu content is now `inert`. A programmatic `.focus()` into a closed item got in 0 of 40 times per configuration; before, it got in 40 of 40. Tab and Option+Tab never landed inside it.
- **Tab out of the open menu:** the View menu's own Tab-out in WebKit, which lost focus to `body` in 5 of 16 combinations before, now lands on the chart region (or the next control) in 16 of 16.
- **Kept:**
  - Arrows, Home and End, typeahead, Shift+Tab and every item (Chart/List, Fit, Expand all, Collapse all) behave as before, in all 32 combinations per build. Focus returns to View after each. Tab out is unchanged except that it improved (above).
  - The BUG-101 menu outline and BUG-123's blocked-Fit explanation, two-stage Escape and late measurement are unchanged.
- **Live team changes:** a live delete or create never leaves a stale menu, portal or focusable closed content, and View remounts closed and works on the first ArrowDown. When the last team goes, focus falls to `body`, but that's pre-existing (P1).

**Mechanism** (measured in the focus logs; the reading is mine):
- Before, focus reached the trigger about 4 ms after Escape. Then, 2–49 ms later, the menu's first item took focus inside the closed, non-inert content. The content then unmounted, and focus fell to `body`.
- After, the same late first-item focus still fires (seen once in 50 repetitions), but `inert` rejects it.
- So the fix masks the late focus rather than removing its source. The outcome is correct; the root cause is noted for Avery and Rowan, not filed.

**Pre-existing items** (the same in before and after; none caused by 5ba5d947):
- **P1 (S3):** when the last team is deleted live while focus is on View or in its menu, View unmounts and focus falls to `body`.
- **P2 (S3):** confirming the Delete team dialog returns focus to the removed "Actions for {team}" button, so focus ends on `body`.
- **P3 (S4):** closing a menu by pointer (clicking the trigger or empty space) leaves focus on `body`.
- **P4 (S4):** with View open, clicking another tab only closes the menu; the tab isn't selected.
- **Already filed:** BUG-137 (the Fit tooltip covers View) and BUG-138 (activating blocked Fit hides its explanation) still reproduce, unchanged.

### Requirements (evidence I opened)

Paths are relative to the scratch folder `…/scratchpad/b129/`.

| Requirement | Before (f3936708) | After (5ba5d947) | Result | Evidence |
| --- | --- | --- | --- | --- |
| Escape returns focus to View: People, WebKit, 320 px with a 20 px root (BUG-129's repro) | 24/50 trigger, **26/50 body**, 0 inside | 50/50 trigger | met (fixed) | `A/out/run-*-webkit-all.log`, `run-*-webkit-extra20.log`; `A/out/shot-before-webkit-people-320-20-FOCUS-ON-BODY.png` vs `shot-after-webkit-people-320-20-crop.png` |
| Escape returns focus to View: People at 16 and 40 px roots and 1280, both engines; Chromium at 320/20 px | 10/10 each; Chromium 25/25 | 10/10 each; 25/25 | met | `A/out/run-*-all.log` |
| Escape and select on Teams and Reporting at 320/20 px (WebKit) | Teams Escape 2/10 and select 2/10 (body 8/10 each); Reporting Escape 5/10, select 9/10 | 10/10 each | met (fixed) | same |
| Selecting an item returns focus to View | People 7/10, Teams 2/10, Reporting 9/10 at WebKit 320/20 px; otherwise 10/10 | 10/10 everywhere | met (fixed) | same |
| Closed content is `inert`, and stale focus can't enter it | Content closed but not inert. Programmatic focus entered it 40/40 per configuration | `closed` + `inert` in every sampled repetition. Programmatic focus entered 0/40 | met | `A/out/reps-*.jsonl` (`samples`, progfocus lines) |
| Clicking outside (or the trigger again) returns focus to View | body 32/32 per build (B), 10/10 per configuration (A), 5/5 (C) | the same | not met, pre-existing (P3); not part of BUG-129's keyboard fix | `B/out/checks.log`, `A/out/run-*-all.log` (outside lines), `C/out/pointer.log` |
| Tab and Option+Tab never land inside closed content | never inside, but WebKit 320/20 px fell to body 9/10 (Escape then Tab, or Tab out) | People chart 10/10 | met | `A/out/run-*-all.log` (esctab and tabout lines) |
| No menuitems exposed to screen readers while closed | Chromium's tree still exposed the closing menu (5 nodes on Teams, 4 on People) | 0 nodes (WebKit's tree can't be read; I infer it also hides inert content) | met | `B/out/probe.json` |
| First ArrowDown/Enter/Space opens the menu and focuses the first item | 95/96 (one WebKit miss) | 96/96 | met | `B/out/checks.log`, `B/out/probe.json` |
| Arrows wrap, Home and End, typeahead (e, c, f) | pass | identical | met | `B/out/compare.log` |
| Tab and Shift+Tab close the menu and move on | Tab 27/32 (5 WebKit Tabs to body); Shift+Tab 32/32 | Tab 32/32, Shift+Tab 32/32: the chart region, or "Actions for Design" in Teams List; Shift+Tab goes to Add agent or Add person | met (improved) | `B/out/compare.log` |
| Items work, and focus returns to View | 124/124 | 124/124. Collapse all goes from 5 to 3 visible nodes and Expand all back to 5; Fit changes the transform; List and Chart swap | met | `B/out/matrix-*.json` |
| BUG-101 outline: pointer hover shows background only; after keyboard use the outline follows | pass | identical (2 px indigo-400 at −2) | met | `B/out/checks.log` |
| BUG-123: blocked Fit explains, doesn't activate, two-stage Escape, late measurement, pointer | pass | identical (Escape twice ends on View 10/10) | met | `B/out/bug123*.json`, `A/out/run-*-all.log` (fitblocked lines) |
| Live delete or create with View open or focused: remounts closed, nothing stale, focus not lost | 0 stale, 0 leftovers. Focus on body when View unmounts (P1) or after Delete team (P2) | the same: menu closed 12/18 (the other 6 stayed open on a live item, correctly), stale 0/18, first ArrowDown and Escape 14/14 | met (P1 pre-existing) | `C/out/summary.log`, `C/out/*-all.json`; `C/shots/after-webkit-1280-100-People-s4open-empty.png` |
| Blitz: rapid open and close, tab switches, back and forward, live pushes | 0 errors | 0 errors (one WebKit module-import artefact in a before run, BUG-140) | met | `C/out/summary.log` |

### Findings (verified on the screenshots myself)

| Id | Sev | Where | What I saw | Fix (outcome) |
| --- | --- | --- | --- | --- |
| P1 | S3, pre-existing | `/org-chart` People, both engines, every size: the last team is deleted live while focus is on View or in its open menu | View unmounts with the chart, and focus falls to `body`, 4 of 4 per combination. When a team is created again, View remounts closed and works, but focus is still on `body`. `C/shots/after-webkit-1280-100-People-s4open-empty.png` (no focus anywhere; "No teams yet") | When the View trigger or its menu has focus and the chart unmounts, focus moves to a stable target in the People view, never `body`. For example the empty state, made focusable, or the People tab. |
| P2 | S3, pre-existing | `/org-chart` Teams, Chart (card ⋯) and List (row ⋯), Delete team confirmed, both engines, every size | The dialog returns focus to "Actions for {team}", which the reload then removes, so focus ends on `body` (2 of 2 per combination). The toast shows, and View still works. `C/shots/after-webkit-1280-100-Teams-s5Chart.png` | After a successful delete, focus goes to a surviving neighbour (the next team's actions button, or the chart region), never `body`. |
| P3 | S4, pre-existing | Any View menu closed by pointer (clicking the trigger again, or empty space), all tabs and sizes, both engines | Focus ends on `body` in every run (32 of 32 per build in B, 10 of 10 per configuration in A, 5 of 5 per build and engine in C's pointer check). The next Tab starts from the top of the page | After a pointer dismissal, focus goes back to the trigger unless the click landed on a focusable control. This is the shared `DropdownMenu` behaviour, so the outcome applies to every menu. |
| P4 | S4, pre-existing | `/org-chart` with View open, clicking another tab (Teams, People, Reporting) | The first click only closes the menu. The tab isn't selected, and focus goes to `body`; it takes a second click. I infer the modal menu blocks pointer events outside it | A click on another control while a menu is open closes the menu and acts on that control. |

Not findings:
- **BUG-137 and BUG-138** still reproduce unchanged: the Fit explanation covers the View trigger, and activating blocked Fit hides its explanation. Both are already filed for Avery. `B/catalog/org-chart-view-menu-desktop.png`.
- **The late first-item focus** behind BUG-129 still fires during the close (once in 50), and `inert` absorbs it. It needs no row, but it is worth Avery and Rowan knowing the source remains.
- **Pressing Enter and Escape 30 times with no waits** sometimes ends with the menu open and focus on a live item, never stale content. That happened in 4 of 12 runs (3 before, 1 after); I infer it's key presses racing the animation on a loaded machine. With waits it passes 30 of 30.
- **One keyboard open in about 300** focused the menu container rather than the first item, before and after alike. The next arrow recovers it.
- **My brief listed "clicking outside returns focus to View" as part of BUG-129.** The bug and Avery's fix are about keyboard closes, so I've filed the pointer case as P3 rather than counting it against this merge.

### Evidence (Part 1)

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| A: BUG-129 repro and focus return | `A/` | `probe.mjs`; `out/run-*.log`, `out/reps-*.jsonl`, `out/summary-*.json`; screenshots `out/shot-*.png` | `DONE before webkit all configs=34 reps=355`; `DONE after webkit all configs=34 reps=355`; `DONE before chromium all configs=34 reps=355`; `DONE after chromium restAfterChromium configs=24 reps=240`; `DONE before webkit extra20 configs=1 reps=25`; `DONE after webkit extra20 configs=1 reps=25`; `DONE before webkit shots configs=2 reps=2`; `DONE after webkit shots configs=2 reps=2`; `DONE before webkit failshot configs=1 reps=1`; the crashed first after-Chromium run ends `Node.js v26.13.0` (`run-after-chromium-all.log` is byte-identical to `run-after-chromium-all-crashed.log`); `smoke-after-webkit-shots.log`: `DONE after webkit shots configs=2 reps=2` |
| B: keyboard, pointer and kept behaviour | `B/` | `matrix.mjs`, `bug123.mjs`, `probe.mjs`, `shots.mjs`, `compare.py`, `checks.py`; `out/*.json`, `out/*.log`; 6 shots; 2 catalog shots | `DONE webkit before combos=16 errors=0` (and the same for webkit after, chromium before and chromium after); `DONE compare combos=32 differing_fields=46`; `DONE checks fails=165`; `DONE probe runs=20`; `DONE bug123 runs=16 errors=12`; `DONE bug123 runs=4 errors=0`; `DONE shots=8`; `out/trial.log`: `DONE webkit after combos=1 errors=0` |
| C: live team changes and blitz | `C/` | `probe.mjs`, `pointer.mjs`, `summarize.mjs`; `out/*-all.{log,json}`, `out/pointer.log`, `out/summary.log`; 196 shots | `DONE before chromium all: scenarios=28 blitz=3 probeErrors=0`; `DONE before webkit all: scenarios=42 blitz=3 probeErrors=0`; the same two for after; `SUMMARY END`; `out/pointer.log`: `POINTER DONE before/chromium trigger-click {"body":5} outside {"body":5} kbd-open+outside {"body":5} \| before/webkit trigger-click {"body":5} outside {"body":5} kbd-open+outside {"body":5} \| after/chromium trigger-click {"body":5} outside {"body":5} kbd-open+outside {"body":5} \| after/webkit trigger-click {"body":5} outside {"body":5} kbd-open+outside {"body":5}` |

- **A's Chromium runs:**
  - After: the first run died on a page-load timeout under load. `restAfterChromium` finished the remaining configurations.
- **B's `checks fails=165`** breaks down as:
  - 63 from the closed-menu accessibility snapshot: 32 in the before build, where the closing menu really is exposed (Chromium's tree shows 5 and 4 nodes); and 31 in the after build, which I infer is Playwright's `ariaSnapshot` ignoring `inert` for about 100 ms, since Chromium's tree shows 0 nodes;
  - 32 from a colour-string rounding mismatch (the same indigo-400);
  - 64 from the pointer dismissal (P3);
  - 6 before-only misses.
- **B's `bug123 … errors=12`** are the pointer steps at the sizes where Fit isn't blocked, so they don't apply.
- **My own recount** from A's per-repetition data: People at WebKit 320/20 px Escape gives before 25 trigger and 27 body, and after 51 trigger, including the screenshot and failure-capture repetitions. Over all Escape and select repetitions, before has 52 body and after has 0.

## Part 2 · Chat BUG-134/135/136: large text in the header, Ask cards and add rows

**Verdict: BUG-134, BUG-135 and BUG-136 are met in both engines, with one new S4 polish item (N1).**
- **Layout sweep:** 120 combinations per build and engine (widths 320 to 1280; 100%, 200%, and 20 and 40 px roots; channel, DM, long title, Ask and Approval, sidebar). Before: 715 overflow, clip or orphan flags. After: 0, excluding the filed BUG-119 case.
- **BUG-134:** at 320 px the header now reflows inside its column. Sideways scroll went from 322/320 (200%) and 402/320 (40 px root) to 320/320. A long channel title wraps whole hyphen segments (3 lines at 200%, 4–5 at a 40 px root depending on the title) and never overflows. A single unbreakable token can only break mid-token, and it stays inside the column. That's why it passes, unlike P1, where the heading is a normal word that has room to break at a word boundary.
- **BUG-135:** Ask and Approval options wrap inside the card with their full labels ("No, allow (default)" is no longer clipped), and their focus outline is no longer cut sideways. The Announcement link wraps inside its row (Chromium, 320 px with a 40 px root: 9 px overflow before, 0 after).
- **BUG-136:** "+ New message" and "+ Add channel" keep the plus icon beside the first line of text at every size. The icon is now 24 px at 1280, matching the other sidebar rows.
- **Kept:** keyboard order, accessible names and the accessibility tree are identical before and after (the tree differs only in clock text). Answering by keyboard and pointer gives the same result, as do the sidebar labels, message metadata and the DM avatar size from BUG-94..99. Blitz: 0 console and 0 page errors in both builds and engines.

### Requirements (evidence I opened)

Paths are relative to `…/scratchpad/b129/`.

| Requirement | Before (5ba5d947) | After (6072bf08) | Result | Evidence |
| --- | --- | --- | --- | --- |
| No sideways scroll on `/chat` at any width or text size (BUG-134, §20) | 322/320 at 200%, 402/320 at a 40 px root (320 px) | 320/320 everywhere | met (fixed) | `D/D-compare.log`, `E/out/summary.txt` |
| The header reflows; Conversations, Members and Search keep whole outlines | Conversations cut 4 px at 200%; at 40 px cut 36 px right and 4 px left; Members cut 4 px left | All fully visible, Conversations wraps "Conversations / 2" | met (fixed) | `E/shots/webkit-before-320-r40-header-focus-viewport.png` vs `webkit-after-320-r40-header-focus-viewport.png` |
| A long channel or DM title wraps at word or hyphen breaks and never overflows | Title not rendered at 320 (0 px wide) | Full title, 3 lines (200%) and 4–5 lines (40 px root; 4 for E's `#quarterly-planning-…`, 5 for D's `#design-review-for-the-new-payments-team`); a single token with no break point (`#aaaa…`, 40 letters) breaks mid-token at every size, which is unavoidable and stays inside the column, whole hyphen segments | met | `E/shots/*-long-title.png`, `D/results-after-*.json` |
| Ask and Approval options stay inside the card with full labels (BUG-135) | "No, allow (def…" ran out of the card at 200% (320/640/960) and a 40 px root (1280) | Wrap inside the card, full label, 106–182 px tall when wrapped | met (fixed) | `E/shots/webkit-before-320-200-ask-option-focus-viewport.png` vs `webkit-after-320-200-ask-option-focus-viewport.png` |
| Option names equal visible labels (WCAG 2.5.3) | match | match, including wrapped | met | `E/out/summary.txt` |
| Option focus outline not clipped (§17) | cut 3.9 px right (200%), 42.2 px right and 4 px left (40 px root); Approve cut 4 px left (Chromium) or 3.4 px right (WebKit) at 40 px | Solid 2 px indigo-400 at offset 2, no horizontal clip | met (fixed); vertical cut is P2 below | `E/out/summary.txt` |
| Announcement link wraps inside its row | Chromium 320 px, 40 px root: 9 px overflow | 0 overflow; still reachable, Enter opens it | met (fixed) | `E/out/summary.txt` (`overflowPx:9`) |
| Plus icon beside the first text line on the add rows (BUG-136, §22) | Orphaned at 200% (New message) and a 40 px root (both rows) | Beside the first line; 24 px at 1280, capped at 32 px | met (fixed) | `D/results-*.json`, `D/D-compare.log` |
| Add rows: names, open by keyboard and pointer, Escape returns focus | "New message", "Add channel"; Escape to the row (1280) or the Conversations toggle (320 drawer) | identical | met | `E/out/summary.txt` |
| Ask answering (Enter, Space, pointer, Reject… with text) | "Answered by Alex Kim: …", focus to the card | identical | met | `E/out/summary.txt` |
| Contrast (§18) | same | Options 12.15:1, Approve 4.58:1, add rows 10.51:1, title 14.67:1, link 7.3:1 | met | `E/out/summary.txt` |
| Accessibility tree unchanged | — | 0 differences after ignoring clock text | met | `E/out/aria-diff/`, `E/logs/aria-normalized.log` |
| Kept BUG-94..99: sidebar labels, message metadata, DM avatar | as validated | identical | met | `E/out/summary.txt`, `D/results-*.json` |
| No errors under a blitz (3 runs per build and engine) | 0 console, 0 page | 0 console, 0 page | met | `E/out/blitz-*.json` |
| BUG-119 (960 px, 40 px root, 240 px main column): recorded, not counted | Reply… outside its column | the same | excluded (filed) | `D/D-extra-chromium.log` |

### Findings (verified on the screenshots myself)

| Id | Sev | Where | What I saw | Fix (outcome) |
| --- | --- | --- | --- | --- |
| N1 | S4, new in 6072bf08 | `/chat?c=c-project-atlas`, the Ask card's option row, measured at 1280/100% in WebKit (the 2 px height change also shows in Chromium, `D/D-compare.log`) | The option buttons became 34 px tall with 8 px padding and left-aligned text, while Reply… beside them stayed 32 px with 10 px padding and centred text. Before, all three were 32 px, 10 px, centred (`D/D-probe.log`). At 100% the 2 px difference is barely visible (`D/catalog/chat-ask-desktop.png`); it's a consistency item, measured rather than seen. The same change hit the Approval card: Approve and Reject… also went from 32/10/centred to 34/8/left, but they still match each other, so the mismatch is only Ask options against Reply… | Options and Reply… in a card share one height, padding and text alignment at 100%; when an option wraps, it grows in height only, and single-line options keep the shared size. |
| P1 | S3, pre-existing | The Conversations drawer heading at 320–1280 px with 200% text or a 40 px root, both engines | "Conversations" breaks mid-word (2–3 lines in a 95–127 px column, `overflow-wrap: anywhere`), 9 of 9 large-text cases (`D/D-extra-chromium.log`). Unchanged by this merge | The drawer heading breaks only between words (§20); if a word can't fit, the column grows or the heading scales, it never splits. |
| P2 | S4, pre-existing, slightly worse in 6072bf08 | `/chat?c=c-project-atlas` at 320 px with 200% text or a 40 px root, both engines | The message list sits at its 4rem minimum (128/160 px) under a tall header, so a focused option is only partly visible: 55–58% after, 62–63% before (`E/logs/area.log`). The header grew from 425 to 499 px (200%) and 581 to 683 px (40 px root), and the wrapped options got taller. `E/shots/webkit-after-320-200-ask-option-focus-viewport.png` | A focused control is fully visible: the message list keeps a usable height at large text, or focus scrolls the list so the whole control shows. It may belong with BUG-119 (shell at large text); ada's call. |
| P3 | S4, pre-existing | Ask card option buttons (outline variant), every size | The button border is 1.37:1 against the card fill (§18 asks 3:1 for UI boundaries). The text is 12.15:1 and the fill still separates the button, faintly | The outline button's boundary reaches 3:1 against the surface it sits on, here the card. |

Not findings:
- **The search placeholder** cut to "Search · f" at 320 px with a 40 px root: a placeholder, pre-existing, and the field keeps its name.
- **Ask options have no group or radiogroup**, before and after. If we want them announced as a set, that's a design decision I'll take up separately; nothing regressed.
- **At 320 px, Escape from New message or Add channel** returns focus to the Conversations toggle (the drawer closes), not to the add row. That's the e2e contract, and the same before and after.
- **WebKit "Importing a module script failed"** artefacts under the blitz (34 before, 36 after) are BUG-140. Chromium `goBack`/`goForward` harness failures (2 of 3 before, `Protocol error … Not attached to an active page`; 1 of 3 after, `page.goBack: Timeout 30000ms exceeded`) carried no errors; I infer they're a Playwright artefact.

### Evidence (Part 2)

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| D: layout at every width and text size | `D/` | `D-matrix.mjs`, `D-compare.mjs`, `D-extra.mjs`, `D-probe.mjs`, `D-cardshots.mjs`; `results-*.json`; `shots/`; 4 catalog shots | `[before/webkit] DONE combos=120 errors=0 flaggedCombos(excl BUG-119)=65 flags=357`; `[before/chromium] DONE combos=120 errors=0 flaggedCombos(excl BUG-119)=65 flags=358`; `[after/webkit] DONE combos=120 errors=0 flaggedCombos(excl BUG-119)=0 flags=0`; `[after/chromium] DONE combos=120 errors=0 flaggedCombos(excl BUG-119)=0 flags=0`; `DONE before flags=715 after flags=0 (excl BUG-119); errors=0`; `DONE extra webkit`; `DONE extra chromium`; `DONE probe`; `DONE cardshots` |
| E: behaviour, accessibility and kept | `E/` | `E-run.mjs`, `E-blitz.mjs`, `E-compare.mjs`, `E-merge.mjs`, `E-shots.mjs`, `E-area.mjs`; `out/`, `logs/`, `shots/` | `DONE before-webkit: configs=3 stepErrors=2 console=0 pageErrors=0` (my script's long-title wait; reruns `DONE before-webkit: configs=1 stepErrors=0 console=0 pageErrors=0` twice and `MERGE DONE: stepErrors=0 console=0 page=0`); `DONE after-webkit: configs=3 stepErrors=0 console=0 pageErrors=0`; `DONE before-chromium: configs=3 stepErrors=0 console=0 pageErrors=0`; `DONE after-chromium: configs=3 stepErrors=0 console=0 pageErrors=0`; `COMPARE DONE: ariaDiffFiles=16`; `ARIA NORMALIZED DIFFS: 0`; `SHOTS DONE: 4 shots`; `AREA DONE: 8 cases`; `BLITZ DONE before-webkit: runs=3 console=0 page=0 importArtefact=34 actionFailures=0`; `BLITZ DONE after-webkit: runs=3 console=0 page=0 importArtefact=36 actionFailures=0`; `BLITZ DONE before-chromium: runs=3 console=0 page=0 importArtefact=0 actionFailures=2`; `BLITZ DONE after-chromium: runs=3 console=0 page=0 importArtefact=0 actionFailures=1`; trial `DONE after-chromium: configs=1 stepErrors=0 console=0 pageErrors=0`; discarded first blitz harness (it clicked hidden sidebar links, a bug in my script) in `logs/blitz-harness-v1/`: `BLITZ DONE after-chromium: runs=3 console=0 page=0 importArtefact=0 actionFailures=4`, `BLITZ DONE after-webkit: runs=3 console=0 page=0 importArtefact=37 actionFailures=3` |

All five checks stopped their servers and removed their worktrees; `git worktree list` in the product repo shows none of them.
