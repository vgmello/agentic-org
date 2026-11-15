# Validation · BUG-101/102 focus outline (v1 62efec16 vs 8724c857)

Requested by ada (01M77GKG). This is Jules's jules/focus-primitives, merged as 62efec16 (Tier B, fresh Sol zero). It replaces the shared Button's faint shadcn ring with the §17 outline, and gives DropdownMenu items the §13 inset outline. I compared it with the first parent, 8724c857, which has the same UI as d49446c0. Checked 12 Nov 2026.

**Verdict: met for BUG-101 and BUG-102 in both engines. One new S4 clip, and no other regressions.**
- **Button:** all six variants, as a button and as a link, show a solid indigo-400 2 px outline at offset 2 on keyboard focus (48 of 48), with no ring. A pointer click shows none (48 of 48).
  - `aria-invalid` keeps its red cue and adds the outline.
  - Disabled buttons and links are skipped.
  - A caller's class still wins.
  - Contrast went from 1.74–1.77:1 to 4.69:1 on real surfaces (ProcessSheet Close in Chromium already had a 2 px indigo-500 outline at 3.21:1) (1.87–1.95 to 5.67 in the fixture).
- **Menu items:** an outline is drawn exactly when native `:focus-visible` is true. That held in 328 of 328 snapshots in Chromium and 327 of 327 in WebKit, across all four app menus and the fixture.
  - A menu opened by pointer and then hovered shows the background only.
  - After keyboard use, the outline follows the hovered item.
  - Through a pointer select, close and reopen, Chromium keeps the outline and WebKit clears it. That's as pinned in §13.
  - Disabled skipping, `aria-disabled`, Home, End, typeahead, Escape and focus return all still work.
- **Consumers:** in 10 feature families and 7 dialogs and sheets, 557 of 558 samples showed the correct outline. The one exception didn't recur on a re-run; it looks like the BUG-120 fade.
  - Escape closes every dialog and sheet and returns focus to its opener, 126 of 126.
  - ProcessSheet Close is correct in all 18 configurations.

**New:** one S4. In the Report a bug dialog at 320/200%, the "Draft or improve" button's outline loses its left and right sides to the dialog's own overflow. The old ring-coloured border showed on all four sides.

**Pre-existing, now easier to see because the outline sits outside the control** (the old ring was clipped in the same places):
- **S3:** dialogs without their own scrolling run past the viewport at large text, so their footer buttons can't be seen or clicked. Examples: New task, and Assign team. It's the same family as BUG-29.
- **S3:** dialog and sheet footers overflow sideways at 320 px with large text (Delete team, Add agent, New task, Assign team, Report a bug). ProcessSheet overflows at 320 and 960. Same family as BUG-31.
- **S3:** buttons at the right edge of the page lose 3–5 px of outline to `main` (24 px for chat Members). Ada has folded this into BUG-119.
- **S3:** destructive menu text (red-400) is 4.36:1 on the highlighted item. It was 3.86 before, and §13 calls for red-300.
- **S3:** at 320/200%, "Create organization" runs past the viewport's right edge.
- **S4:** a control scrolled to the edge of a scroll area loses about 4 px of its outline.
- **S4:** in WebKit, a sheet opened by pointer keeps focus on Close with `:focus-visible` false, so no outline ever shows. That's the same before; Chromium shows it.

**Known and excluded:**
- **BUG-131:** the ProcessSheet facts grid at 320 px with a 40 px root.
- **BUG-120:** the outline fades in over 0.15 s. Every reading was taken after 300 ms.
- **BUG-119:** the shell starves the main column at large text.

**Method:**
- Requirements:
  - ada's request and Jules's review requirements;
  - BUG-101 and BUG-102;
  - design-system §13 as pinned (native `:focus-visible`), §17, §18 and §20.
- Builds: each before and after ran as its own dev server in e2e mode with sample data ("Acme"). For a side-by-side comparison, the test-only fixture was copied, untracked, into the before worktree. WebKit is the Tauri engine; its keyboard walks used Option+Tab. "200% text" means the root set to 200%.
- Three parallel checks:
  - **Button contract:** the six variants as button and link, in WebKit and Chromium at 1280/100% and 320/200%, on both builds, plus 13 real instances on 9 routes.
  - **Menu items:** 4 app menus plus the fixture, flows (a) to (e), in WebKit and Chromium at 1280/100% and 320/200%, on both builds. The View menu was also checked at 960 with a 40 px root.
  - **Consumers and footers:** 10 families and 7 dialogs and sheets, in 18 configurations per build. That covers WebKit and Chromium at 320 and 960 × 200%, a 32 px root and a 40 px root; 1280/100%; and Chromium with a 20 px default at 100% and 200%, at 320 and 960. 648 samples per build.

## Requirements (evidence I opened)

| Req | Before (8724c857) | After (62efec16) | Result |
| --- | --- | --- | --- |
| Button: six variants, button and link, keyboard focus | 3 px ring at 50% plus a ring-coloured border, 1.74–1.77:1 on real surfaces, except ProcessSheet Close in Chromium (indigo-500, 3.21:1); 1.87–1.95 in the fixture | Solid indigo-400 (`oklch(0.673 0.182 276.935)`), 2 px, offset +2, no ring, 48/48 | met |
| Button: pointer click | — | `:focus-visible` false and no outline, 48/48 | met |
| Button: `aria-invalid` | Red ring plus border | Red ring and border kept, plus the indigo outline | met |
| Button: disabled skipped, caller class wins, Enter and Space activate, href navigates, hover unchanged | yes | yes | met |
| Menu item: outline exactly when native `:focus-visible` is true | No outline; failed in 176/327 (Chromium) and 148/327 (WebKit) snapshots | 0 mismatches in 328 (Chromium) and 327 (WebKit) | met |
| Menu item: 2 px solid indigo-400 at −2 over `white/5` | — | as specified; 4.69:1 on `gray-800` and 4.03:1 on the highlight (`B/catalog/org-chart-view-menu-desktop.png`) | met |
| Menu: pointer-opened hover shows the background only | yes | yes | met |
| Menu: select, close and reopen | — | Chromium keeps the outline (View, message, fixture) and WebKit clears it; the outline matches the native state every time | met (as pinned) |
| Menu: disabled skipped, `aria-disabled` focusable, Home/End/typeahead, Enter/Space, Escape and focus return, outside click | yes | yes | met |
| Menu: outline not clipped on the first and last items, including scrolled menus at 320/200% | — | Pixel bands 100% indigo when drawn | met |
| Consumers and footers: §17 outline | Ring | 557/558; the one exception (Chromium 960, 40 px root) didn't recur | met |
| Escape and focus return, all dialogs and sheets | 125/126 | 126/126 | met |
| Outline fully visible | Ring clipped in the same places | 637 of 648 geometry flags match before. The 11 that differ are all in the Add agent sheet and the Report a bug dialog at 320/960 with large text. Each control sits at a different scroll position in the two builds (for example y=768 vs 529), so the clip and cover flags move between rows (in Add agent they swap between neighbours; in Report a bug the same buttons gain or lose `covered`), while widths and sideways scroll are identical in both builds; they belong to P2 and P6, not to this change. New: the Draft-or-improve clip (N1). The geometry check flags its rows the same way before and after, so N1 rests on the screenshots: the ring-coloured border showed on all four sides before, and the outline loses two sides after | met, except N1 and P1–P6 |

## Findings (verified on the screenshots myself)

| Id | Sev | Where | What I saw | Fix (outcome) |
| --- | --- | --- | --- | --- |
| N1 | S4, new | `/projects/atlas` › Report a bug › "Draft or improve" (Title), 320/200% and a 32 px root, both engines | The left and right sides of the outline are clipped by the dialog content's overflow; only the top and bottom bars show. Before, the ring-coloured border showed on all four sides. Focus is still visible. `A/shots/real-after-webkit-320-200-bug-dialog-draft-title.png` vs `real-before-…` | All four sides of the focus outline stay visible inside the dialog. |
| P1 | S3, pre-existing (BUG-29 family) | New task (`/projects/atlas` › Tasks) at 320 and 960 × 200%, 32 and 40 px roots, and the Chromium 20 px default at 200%; Assign team (`/projects`) at 320, and at 960 with a 40 px root; both engines | The dialog is taller than the 800 px viewport and can't scroll. New task's footer sits 77–358 px below the viewport and its Close up to 377 px above. Assign team's footer is 3–185 px below and its Close up to 205 px above. Delete team at 320 with a 40 px root goes off-screen too (93 px below), so they can't be seen or clicked; at 320/200% the Create task click timed out. The Report a bug dialog scrolls; these don't. `C/shots/after-webkit-960-200pct-dialog:New task-Create_task.png` | Every dialog stays inside the viewport and scrolls, with its footer reachable and enough inner room that a focused control's outline never touches the scroll edge (§13, §20). |
| P2 | S3, pre-existing (BUG-31 family) | Sideways scroll at 320 px, from 200% up to a 40 px root: Delete team 19–104 px, the Add agent sheet 18–82 px, New task 363–533 px, Report a bug 395–574 px, Assign team 79 px (40 px root and the 20 px default at 200%). With the 20 px default at 100% (320), New task scrolls 108 px and Report a bug 128 px. ProcessSheet scrolls 5–9 px at 960 (200% and 32 px root), 116–121 px at 960 with a 40 px root or the 20 px default at 200%, and 103–133 px at 320, beyond BUG-131's facts grid | The footers overflow the dialog sideways. In Delete team the buttons stick out past the dialog edge and the overlay covers the right side of Cancel's outline. `C/shots/after-webkit-320-200pct-confirm:Delete team-Cancel.png` | The footer buttons stack inside the dialog at narrow widths, with no sideways scroll (§20). |
| P3 | S3, pre-existing, folded into BUG-119 by ada | Page buttons at the right edge, at 320 and 960 with large text: New task, Add agent, Assign, Report a bug; chat Members at 960 with a 40 px root or the 20 px default at 200% | 3.2–5.5 px of the outline's right side is clipped (Members loses 24.2 px) by `main` and the shell's overflow. At 960 × 200% the sidebar also overlaps the main column. `C/shots/after-webkit-960-200pct-project-tasks-New_task.png` | The main column leaves room for the outline at its edge. Filed by ada under BUG-119. |
| P4 | S3, pre-existing, improved | Destructive menu items ("Delete…" in the team row actions, and the fixture's "Delete item"), both engines | The red-400 text is 4.36:1 on the highlighted item (3.86 before), below 4.5:1 for 14 px text. §13 calls for `text-red-300`. `B/shots/after-webkit-1280-16-team-a-last.png` | Destructive item text is at least 4.5:1 on both the normal and highlighted menu surfaces, and stays red. |
| P5 | S3, pre-existing | `/create` "Create organization" at 320/200%, both engines | In the Button check at 320/200% the button starts at x=48 and is 304.8 px wide, so it runs to 352.8, past the 320 px viewport, and its text and outline are cut off. In the consumer check it sits at x=15, 305 wide, and loses 3.8 px of outline at 200%, then runs to 381 px with a 40 px root. I haven't established why the two checks place it differently; both show it doesn't fit at large text. `A/shots/real-after-webkit-320-200-create-submit.png` | The button fits inside the viewport, and its label wraps or the layout reflows (§20). |
| P6 | S4, pre-existing | Report a bug Cancel at 1280/100%; chat Send | A control focused at the edge of a scroll area is scrolled into view, but its outline isn't: about 4 px is clipped. `C/shots/after-webkit-1280-100pct-dialog:Report a bug-Cancel.png` | Focusing a control in a scroll area brings its whole outline into view (§17). |
| P7 | S4, pre-existing, WebKit only | `/offices` and `/processes?place=team:web` sheets, opened by pointer | Focus starts on Close with `:focus-visible` false and stays false through 14 Option+Tab presses, so no outline ever shows. The same before; Chromium turns it on with Tab. `A/probe2.json` | In both engines, the first keyboard move inside a sheet opened by pointer shows the focus outline. |

Not findings:
- **The one bad consumer sample** (Chromium 960, 40 px root, Report a bug "Draft or improve" in Steps: 3 px, offset 0, ring colour) measured clean on a re-run. It reads as the BUG-120 fade.
- **The Add agent sheet's team options** (Design, Engineering…) report `:focus-visible` with no outline and no ring. They aren't the shared Button; it's a follow-up for the sheet's own controls, not this change.
- **The fixture's scroll container** clips the bottom edge of the outline at 320/200%. It's a test-only fixture; P6 covers the real case.
- **Moving a task through its card menu by pointer** leaves focus on `body`, because the card re-renders in a new column. That was the same before, and it's outside this change.
- **The ChartFitMenuItem "unavailable" state** wasn't exercised in the real People tab here. It was validated in BUG-123 (`validation-chat-bug94-99.md`), and the fixture covered `aria-disabled`.

## Evidence

Scratch: `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/focus101/`.

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| Button contract | `A/` | `measure.mjs`, `summarize.py`, `probe2.mjs`, `results.json`, `summary.txt`, `probe2.json`, 216 shots | `FINAL combos=8 …`; `SUMMARY combos=8 fails_after=11 fails_before=98` (the 11 after-fails are N1, P5, P7, the fixture's own bottom clip on its destructive link at 320/200% in both engines, and one sampler miss whose screenshot shows the outline intact); `PROBE2 linksEnterAndClickNavigate=true` |
| Menu items | `B/` | `scripts/`, `results-{before,after}-{webkit,chromium}*.json`, `summary.txt`, 425 shots, 1 catalog shot | `menus {before,after} {webkit,chromium}: 60 flow records, 1 errors` (the error is P1's Create task timeout at 320/200%; the re-run gave `6 flow records, 0 errors`); `scroll {before,after}: 12 measurements`; `catalog: Actual size fv=true outline=solid 2px oklch(0.673 0.182 276.934998) offset=-2px` |
| Consumers and footers | `C/` | `run.mjs`, `analyse.mjs`, `diff.mjs`, `results-{before,after}.json`, logs, shots, 2 catalog shots | before `DONE configs=18 rows=648 afterRows=0 afterFlagged=0 errors=0`; after `DONE configs=18 rows=648 afterRows=648 afterFlagged=351 errors=0` (the flags are N1 (its rows flagged scroll-edge-clip, as before), P1–P3, P5, P6, 90 rows for the Add agent team options, and the one sample that didn't recur); re-run `DONE configs=1 rows=36 afterRows=36 afterFlagged=18 errors=0`; `CATALOG DONE` |

- The final lines are quoted from each script's console output. Not all of them were saved to a file. The saved results (`results*.json`, `summary.txt`, `probe2.json`, logs) hold the numbers behind them.
- Each worktree got its own `bun install --frozen-lockfile`.
- All servers are stopped and all worktrees removed. Nothing in the product repo was edited.
