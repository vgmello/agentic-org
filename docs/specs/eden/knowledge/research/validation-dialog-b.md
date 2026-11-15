# Validation: dialog cut B, atomic word spans in DialogTitle (v1 d256b6c1 vs 2b86877b)

**Verdict: MET.**
- Dialog titles no longer split a word that fits the title column. Before the merge, 12 words split per engine in the fixture layout. The shipped layout had 0 splits before, too.
- Accessible names, text, the heading and the 100% layout are unchanged.
- All 23 sites are migrated.
- Nothing that had to be kept regressed.
- Two problems that already existed before the merge belong to cut A (P1, P2 below).

- **Build:** v1 `d256b6c1`, the merge of Jules's jules/dialog-title-words `06a5f7ac`. Before is the first parent, `2b86877b`.
- **Requested by:** ada, 01M78E46. **Validated:** 12 Nov 2026, by Eden.
- **Method:** three checks of the before and after builds, each in its own git worktree, in WebKit (the Tauri engine) and Chromium, with Playwright.
  - Mid-run, ada asked everyone to run browsers one at a time because the machine was overloaded. From then on the checks ran one after another. Any run cut short by that change was rerun in full.
  - All counts are per build and per engine. Nothing is pooled.
  - 24 title cases cover the 23 sites: DeleteTeamDialog has a "delete" title and a "delete refused" title.
- **Pins checked:** my 12 Nov dialog pins (journal 13:44–13:50).
  - A word that fits the column moves whole below Close, and only a word wider than the column breaks, inside the column.
  - The accessible name and copied text are unchanged, with one heading, identical at 100%.
  - Out of scope: cut A's no-overflow and 168 px column claims, and its 0.125rem Close-padding allowance.

## Requirements

**Configurations behind each row:**
- **Shipped:** the merged layout at 320, 390, 640 and 1280 px, with text at 100%, at 200% and with a 40 px root.
  - That is 288 configurations and 972 title words per run.
- **Fixture:** Jules's `closeFootprintFixture`. It reproduces cut A's geometry (a 168 px column at 320 px with a 40 px root), which is the only layout with the float-shortened line beside Close.
  - That is 192 configurations and 648 words per run.

| # | Requirement | Before | After | Result | Evidence |
|---|---|---|---|---|---|
| 1 | A word that fits the column never splits mid-word | Shipped: 0 splits per engine. Fixture: 12 splits per engine (project, Assign, Delete, Change, Teams, Members, Remove ×2, Rename, Offboard, and "Engin/eering" ×2 in move team, at 320/200 and 390/root40) | Shipped and fixture: 0 per engine | Met | db/A/results/A-summary.json; A-before/after-webkit-move-team-320-root40-fixture.png (opened) |
| 2 | A word wider than the column breaks only inside it | Shipped: 3 per engine. Fixture: 4 per engine. All inside the column (rightVsColumn < 0) | The same counts. Each break stays inside the title column, but that column can itself be wider than the dialog's content box (P1). For example, delete refused at 320/root40 ends 14.81 px past the content box (the same before). In move team the word now moves whole below Close, then breaks ("Engineeri/ng", 200.17 px against 168) | Met | the same, plus the screenshot pair |
| 3 | Accessible name, heading name, textContent, innerText and copied text are unchanged | Baseline: 96 configurations per engine (4 widths at 100% text). At 200% text and with a 40 px root, only textContent was compared: 0 mismatches over 288 configurations per run | 96/96 identical per engine. The Chromium accessibility tree is identical. Selecting the title, triple-clicking it and dragging across it give no doubled or missing spaces | Met | db/B/out/B-compare.json, `B-compare DONE: 192 paired configs, 0 problems` |
| 4 | One title heading, at the same level | 96/96 per engine: one dialog-title heading, level 2 | The same | Met | db/B/out/*/results.json |
| 5 | Identical at 100% text (within 0.5 px, same line count) | Baseline | Largest difference 0.000 px; line count changed in 0 of 96 per engine | Met | db/B/out/B-compare.json |
| 6 | All 23 sites use `text=` | Mixed: titles passed as children | 23/23 use `text=` (22 files; ProjectTeamDialog has 2). The only children titles left are the wrapper's fallback and a test fixture | Met | git grep of d256b6c1 |
| 7a | Descriptions still break inside their own box | 0/24 cases per engine with a word outside the box | 0/24 per engine. Long team names: before 0/3 and after 0/3 samples per engine | Met (the box itself overflows, see P1) | db/C/C5-compare.json, C3-longname.json |
| 7b | Close box and focus order unchanged | Baseline | Close: 0/96 configurations differ by more than 0.5 px, per engine. Tab and Shift+Tab sequences identical on 24/24 | Met | db/C/C5-compare.json, C1-*.json |
| 7c | Focus trap and Esc | Trap 24/24: focus stayed inside the dialog over 14 Tab presses. Report, team-members and channel didn't complete a full cycle in 14 presses. Esc closes 24/24. Opened from the keyboard: returns to its opener 8/11, and to the menu trigger 3/11 | The same | Met (unchanged) | db/C/C1-*.json, C4-keyopen*.json |
| 7d | Sheets unchanged | Baseline | 0/8 markup and 0/8 geometry differences (4 sheets × 2 engines); the sheet source is unchanged | Met | db/C/C2-sheets.json |
| 8 | §18 title contrast; §17 ring on Close | 14.67:1; solid 2 px ring, offset 2, 4.69:1 | The same (23/24 cases sampled; Close on team-members wasn't reached within 14 Tabs) | Met | db/C/C1-*.json |
| — | Visual sweep: 8 dialogs at 1280/100% and 320/200%, in both engines | 32 pairs | 11 pairs have pixel differences; I could see none: small glyph differences, and a settle difference on channel at 1280 | Met | db/C/C6-pixdiff.json, crops/ |
| — | Jules's tests on the after build | — | Unit tests 93/93. e2e: WebKit 298 passed, Chromium 298 passed, with a real 20 px default font 192 passed | Met | db/B/B-unit.log, B-e2e-*.log |

## Findings

Neither finding is a B failure. Both exist in the before build in both engines, and both belong to cut A, which isn't merged yet.

- **P1 · S3 · pre-existing · cut A.** At large text, dialog content is wider than the dialog. The numbers are the same in both builds and both engines:
  - In the shipped layout, title words extend past the content box in 59 of 288 configurations (96–97 words), on all 24 title cases, by up to 253 px.
  - The title column is wider than the content box in 87 configurations, and the dialog scrolls horizontally in 70–71.
  - At 320 px, for example, offboard, remove person, and delete with a long team name are wider than the window.
  - For offboard, the cause I checked is the destructive footer button ("Offboard Sam Okafor"), which doesn't wrap and widens the dialog's grid. I didn't check the cause at the other sites.
  - The title, description and button are cut off at the right edge. For example, the description runs to x=589 in a dialog box from x=40 to x=280.
  - Shot: db/C/shots/webkit-after-offboard-320-200.png (opened).
  - Expected outcome: cut A's no-overflow rule, with the footer buttons wrapping or stacking inside the dialog.
- **P2 · S3 · pre-existing · cut A.** In the Assign dialog at 390 px with 200% text in WebKit, the title word "to" (x 276–302, on line 1 at y 27) lies inside Close's box, which starts at x 262 and runs from y 14 to y 78 (its right edge wasn't recorded). The screenshot shows "to" under the X glyph. Source: db/C/C8-catalog.json, res/assign/{before,after}/390.
  - Shot: db/C/dialog-b-phone.jpg (opened); the before twin is the same.
  - This breaks ada's cut A rule: a dropped title word may overlap Close's padding by 0.125rem at most, and never the glyph.
  - Expected outcome: cut A's float keeps every title word off the glyph at 390/200%.

**Notes:**
- When a dialog was opened from a menu item, Esc returns focus to the menu's trigger (3 of 11 keyboard-opened sites). That's correct, because the menu item no longer exists once the menu has closed.
- The word span ships as `inline-block [overflow-wrap:anywhere]` with no max-width. Rowan's pin said "max-width 100%, break-word". The measured behaviour meets requirements 1 and 2 either way.
- Sheet titles still use `overflow-wrap:anywhere` and can split mid-word. They're out of scope.
- The fixture layout scrolls horizontally in 24 of 192 configurations per engine, identically in both builds. That comes from its synthetic 168 px geometry and isn't a product reading.
- In piece B's script, "Report a bug" fails its expected-title check 4 times per engine in both builds, although its names match. That's an artefact of the script, not of the product.

Paths: the evidence is under the session scratchpad, `db/{A,B,C}/`. The scripts, logs, JSON and screenshots there aren't committed.
