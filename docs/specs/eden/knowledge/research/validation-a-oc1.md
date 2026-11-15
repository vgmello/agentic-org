# Validation · A-OC1 Org chart (v1 92ca950e vs ef0ce00b)

Requested by ada (01M76HCV). Avery's avery/org-chart-toolbar at 24a626bf, Tier B, Sol3 zero findings. Scope: BUG-70 OC-2, OC-4, OC-5, OC-9 and OC-10, plus BUG-90. Checked 12 Nov 2026.

**Verdict: met. No regressions.** Every A-OC1 requirement works in WebKit and Chromium at every width and text size I checked.
- The owner appears once, as a plain row.
- The three creation actions are kept, and the View menu is right for each view and mode.
- The Callout is quiet.
- No gray-500 text remains apart from the search placeholder.
- The chart frames are named and focusable, and they scroll on their own.
- On a phone, the default view is List.
- People text never goes below 12 px, and Fit-blocked behaves exactly as agreed, including the equal-width boundary.

**One new S4:** the People "lead" badge is read twice by screen readers.

**Shared-primitive gaps the new View menu now exposes (pre-existing):**
- menu items have no visible focus (S2);
- buttons use the faint shadcn ring (S3);
- items and buttons are 32 px tall on phones (S3, BUG-89 family).

**Design notes:** the People chart now scrolls at 1280 with no hint that there's more, and on People, Add person and View sit on separate rows.

**Method:**
- Requirements:
  - my gap sheet rows OC-2/4/5/9/10 (`bug-70-per-screen-gaps.md`);
  - Rowan's slice order, BUG-90;
  - Avery's A-OC1 notes with the agreed amendments (lead badge, Fit-blocked rule);
  - design-system §9, §12, §13, §17, §18 and §20.
- Builds: before is ef0ce00b and after is 92ca950e, each its own dev server in e2e mode with sample data ("Acme").
- Three parallel checks:
  - Requirements and behaviour: WebKit and Chromium, 1280×800 and 390×844, 10 runs.
  - Layout, reflow and Fit: 2 engines × 320/639/640/960/1280 × 100/200% text × 3 views × 2 builds = 128 records, plus 20 behaviour checks.
  - Accessibility, keyboard, blitz and catalog: WebKit 1280/100 and 640/200, plus Chromium 1280/100.
- 200% text means the root font size set to 200%.
- WebKit is the Tauri engine. In WebKit, keyboard walks use Option+Tab, because macOS WebKit skips buttons on plain Tab unless Full Keyboard Access is on.

## Requirements (evidence I opened)

| Req | Before (ef0ce00b) | After (92ca950e) | Result |
| --- | --- | --- | --- |
| OC-2 Owner | Tinted card with an amber crown, 68–156 px tall | `section "Owner"`: "AK" avatar, "Alex Kim · Owner", 40 px tall, no amber, no svg. The owner isn't in any team or tile, and the counts are unchanged ("8 agents · 3 people · 5 teams"). | met |
| OC-4 Toolbar | New team, Add person, Add agent, List, Actual size, Expand all, Collapse all; overflows at 390 | New team (primary), Add person and Add agent each open their dialog, plus **View ▾**. Its items per view and mode: Teams/Chart has List, Actual size/Fit, Expand all, Collapse all. Teams/List has Chart only. People has Fit, Expand, Collapse. Reporting has Actual size/Fit, Expand, Collapse. Every command works: Teams 0.8185↔1 and cards 5→3→5; People 29→20→29 lines; Reporting nodes 12→3→12. Esc returns focus to View, and focus goes back to View after every command. | met |
| OC-5 Unassigned note | Bare amber-200 `<p>` | Quiet Callout (gray-300 on white/3%, 1 px white/10, no live role) with the same words, "1 project has no team yet." It hides at zero after Assign team. The plural form couldn't be shown, because the sample has only one unassigned project. | met |
| OC-9 / BUG-90, 320×640 | Chart top at y 846–866 (below the first screen); no own frame | List (the default below 640) or Chart starts at y 590–599 in both engines. The frames are `role=region` "Teams/People/Reporting chart", `tabindex=0`, `overflow-x:auto`, and scroll by arrow keys. No page scrolls sideways in any of the 128 records. | met (thin: about 45 px of chart shows, see D2) |
| Default view | Always Chart | Below 640 the default is List, from 640 Chart. An explicit choice survives 639↔640 both ways and tab switches, and reload resets it. | met |
| BUG-90 People Fit ≥ 12 px | Smallest text 4.68 px at 390 and 8.97 px at 1280 | Smallest semantic text is 12 px at every width. At 100% the scale is 1 and the frame scrolls. At 200% the scale is 0.5 with text still 12 px (24 CSS px). The Teams 0.75 and Reporting 0.45 floors are unchanged, as agreed. | met |
| Fit-blocked rule | — | When the chart is wider than its frame even at the floor, Fit is `aria-disabled`, still focusable, with a tooltip and `aria-describedby` "The chart is already at its smallest readable size. Scroll to see the rest." Selecting it does nothing. When the chart fits, Fit is normal, with no description. Boundary: at 1435 px the frame is 1075 and the chart 1076, so Fit is blocked; at 1436 px both are 1076, so it's available. Changing the text size re-evaluates it live. | met |
| Lead badge | 10.4 px, no marker, no lead context | 12 px with `data-fit-text`. The tile name is "Hana Weiss, person, team lead". | met (see N1) |
| OC-10 gray-500 | 24 elements ("—", "×", "No people", "N reports") | 0, apart from the search placeholder (`placeholder:text-gray-500`, OC-8's row, P3) | met |
| No regressions | — | The tabs (arrows, Home and End), the selected-member strip, the Assign team dialog and the creation dialogs all work. The accessibility tree changes only where intended (View, the named frames, the owner text, the lead name, the Callout). There are no new console errors. In WebKit, the ResizeObserver loop error now happens only on Reporting (it used to happen on all three tabs). | met |

## Findings (verified on the screenshots myself)

| Id | Sev | Where | What I saw | Fix |
| --- | --- | --- | --- | --- |
| P1 | S2, pre-existing primitive, now on every chart command | View menu (and every `DropdownMenu`), all widths, both engines | The highlighted item gets only `bg-white/5` with `outline: none`, about 1.1:1 on gray-800. Keyboard users can barely see which item is active. §13 and §17 ask for the indigo outline. `C/kbd/focus/webkit-1280-menu-item-focus.png` | In `ui/dropdown-menu` items: `data-highlighted:outline-2 data-highlighted:-outline-offset-2 data-highlighted:outline-indigo-400` (§13). This fixes every menu, "Actions for …" included. |
| P2 | S3, pre-existing primitive | View, New team, Add person, Add agent (shared `Button`) | Focus shows shadcn's `ring-3` in indigo-500 at 50% (about 2:1), with no outline. §17 says to replace it. `C/kbd/focus/webkit-1280-view-trigger-focus.png` | In `Button`, swap `focus-visible:ring-*` for `focus-visible:outline-2 outline-offset-2 outline-indigo-400`. |
| P3 | S3, pre-existing (BUG-89 family) | Below 640 px: toolbar buttons and the new View are 32 px; menu items are 32 px; tabs 36 px; search 32 px; "Actions for {team}" 32×32; "Assign team…" 24 px | §20 asks for 44 px targets below 640. `B/shots/m-webkit-after-320-100-teams.png` | Comes with the BUG-89 primitives (J-TOKENS, J-TABS, J-FORMS): `max-[639px]:min-h-11` on Button, menu items, tabs and inputs. |
| N1 | S4, new | People chart, lead tile | The tile's name "Hana Weiss, person, team lead" already says lead, and the visible 12 px "lead" badge is read again. | `aria-hidden="true"` on the badge. |
| D1 | S3, design | People chart at desktop widths (1076 px chart in a 920 px frame at 1280) | Fit is correctly blocked and the frame scrolls. But the only cue is a card cut off at the right edge, and WebKit's overlay scrollbar is hidden. `A/shots/after-webkit-1280-people-fit-keyboard.png` | Add a right-edge fade on chart frames while `scrollLeft < scrollWidth − clientWidth` (`pointer-events-none absolute inset-y-0 right-0 w-8 bg-linear-to-l from-gray-900`), plus the existing tooltip text as a visible `text-xs text-gray-400` line under the toolbar when Fit is blocked. |
| D2 | S4, design | People and Reporting, 320–1280 | "Add person" and "View" sit on two right-aligned rows. At 320 that costs about 45 px, so only about 45 px of chart shows in the first 640 px. "Sample data" also appears twice (owner row and toolbar). `B/shots/m-webkit-after-320-100-people.png` | Put the primary action and View on one row (`flex flex-wrap justify-end gap-2`). Show "Sample data" once, on the owner row. |
| P4 | S4, pre-existing | Owner region, screen readers | It reads "Alex Kim · Owner Sample data of Acme. Full access…": the sr-only "of Acme" comes after the badge. | Move the sr-only text before the Sample data badge. |
| P5 | S4, pre-existing | Search, all widths | The placeholder is gray-500 (3.04:1), and at 320/200% it's cut to "Search membe". | `placeholder:text-gray-400`; shorter placeholder "Search" below 640 (OC-8). |
| P6 | S4, pre-existing, WebKit only | Reporting lines while resizing | Page error "ResizeObserver loop completed with undelivered notifications", 6 of 6 runs (Teams and People are now 0 of 6, which is better). | Defer ReportingChart's observer work to `requestAnimationFrame`. |

Follow-ups (not bugs):
- Teams (floor 0.75) and Reporting (floor 0.45) still shrink text below 12 px in Fit: "Shared" is 7.8 px and "Owner (you)" is 5.4 px. Their floors were agreed to stay unchanged in A-OC1, so the People readable floor is a later slice.
- View's List/Chart could become radio items with a check mark (§13), so the current mode is announced.

## Evidence

Scratch: `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/aoc1/`.

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| Requirements and behaviour | `A/` | `results.json`, `fit-results.json`, 136 shots | `validate done: 10 runs, 0 step errors`; `fit done: 6 runs, 0 errors` |
| Layout, reflow and Fit | `B/` | `matrix-*.json`, `behave.json`, 150 shots | `matrix complete: 64 records, 0 errors` (WebKit and Chromium); `behave complete: 20 records, 0 errors` |
| Accessibility, keyboard, blitz, catalog | `C/` | `a11y/`, `kbd/`, `blitz/`, 10 catalog shots | `a11y done: 6 configs, 0 errors`; `catalog: 10 screenshots, 0 page errors` |

- Each worktree got its own `bun install --frozen-lockfile`. Symlinked node_modules made Vite block the Inter font, so those runs were discarded.
- All servers are stopped and all worktrees removed. Nothing was edited in the product repo.
