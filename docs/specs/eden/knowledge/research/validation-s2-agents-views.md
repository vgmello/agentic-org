# Validation · S2 Agents views nav (v1 673e1cb7 vs 83f1b90d)

Requested by ada (01M764SRQXQ21H2T44MAGG053X). Avery, avery/agents-tabs @ 446ed434, Tier B, Sol READY.

**Verdict: met.**
- "Agents" and "Org chart" links sit at the top of both pages, directly under the top bar.
- The right link is the current page, and the bar is a named landmark, not a tab list.
- The links go to plain paths, and Back restores the search or the selected member.
- The Org chart's own tabs and toolbar are unchanged.
- The bar looks as §12 specifies at every width and text size, in WebKit and Chromium.

**One new S3:** after you use the bar, focus drops to the page body instead of the page heading the way the sidebar does it, so keyboard users start again from the top of the sidebar.

**Tomas's question (the Offices badge):** pre-existing (S4), not a BUG-76 regression. Details below.

**Method:**
- Requirements:
  - Rowan's bug70-nav-freeze item 8;
  - design-system §2, §12, §17, §18 and §20.
- Sample data ("Acme").
- 200% text means the root font-size set to 200%.
- WebKit is the Tauri engine; Chromium is the spot-check.

## Requirements (evidence I opened)

| Req | Before (83f1b90d) | After (673e1cb7) |
| --- | --- | --- |
| Bar on both pages, under the top bar | No bar, and only one nav ("Main") | `nav "Agents views"` sits directly under the top bar (0 px apart), inside main and before the content. Links "Agents" → /agents and "Org chart" → /org-chart (`c1-agents.json`, `c1-org-chart.json`) |
| aria-current | — | /agents: Agents="page", Org chart none. /org-chart: the reverse |
| Not a tablist | — | The nav has no role and no tabs inside it. The inner "Org chart views" tablist is unchanged: Teams, People, Reporting lines, with arrow keys, Home and End (`c4-tabs.json`, before and after) |
| Landmarks | Main nav, main | Main nav, main, "Agents views" nav: two navs, both uniquely named |
| Plain paths | — | /agents?q=noor → Org chart gives `/org-chart` with no q. Back gives `/agents?q=noor`, with "noor" in the box and 1 row. /org-chart?member=ines → Agents gives `/agents`. Back gives `?member=ines`, and Inês's panel reopens (`c3-*.json`) |
| Org chart unchanged | 36 buttons, 3 tabs, 2 inputs | 36 buttons, 3 tabs, 2 inputs, plus the bar's 2 links. The toolbar is identical, and the ?member deep link works |
| Keyboard | — | Tab order runs: sidebar → Switch organization → Agents → Org chart → page content. Enter navigates; Enter on the current link adds no history entry. The focus ring is 2 px indigo-400 and visible on all 4 link/page combinations. WebKit reaches links with Option+Tab, which is the macOS default and was already the case |
| Focus after navigation | Sidebar: focus goes to `h1#shell-page-title` | Sidebar: still the heading. **Bar: `body`**, and the next Tab goes to sidebar "Chat" (F1; `focus-compare.json`, WebKit and Chromium) |
| §12 look and contrast | — | Current link: white text with a 2 px indigo-400 underline, so it isn't shown by colour alone. Other link: gray-400, 5.64:1. Underline 4.71:1 |
| Layout, 320–1280 at 100/200%, both engines | Document sideways scroll 0/32; main sideways 6 (/agents at 200% from 320 to 960) | Document sideways scroll 0/32, and the same 6 main cases as before. Links never wrap or clip, and are 58 px tall (114 px at 200%). The bar scrolls 16 px of its own padding at 320/200% (F3). WebKit and Chromium give identical numbers (`matrix.json`) |
| ?q and ?add intents | — | Still work with the bar present |

**Tests at 673e1cb7:**
- e2e (`org-chart-responsive` and `agents-roster`, `--repeat-each=2`, Chromium and WebKit): "234 passed (6.0m)".
- Unit (`AgentsTabs`): "Tests 2 passed (2)".

## Findings

1. **S3, focus, new: the bar drops focus to the page body.** After clicking a link or pressing Enter on it, focus is on `body`. The next Tab goes to the sidebar's "Chat", so it takes 11 stops to get back to the content.
   - Sidebar navigation focuses the destination's heading instead (`focusDestinationHeading`).
   - Expected: the same arrival behaviour as the sidebar. Focus the page heading `h1#shell-page-title` after a bar navigation, so screen readers announce the page and the next Tab enters the bar or the content.
   - Evidence: `s2/webkit/out/{webkit,chromium}/after/focus-compare.json`.
2. **S4, alignment, new bar against old content gutters.**
   - The bar uses the §2 gutters (16, 24 and 32 px). The page content below it uses a fixed `px-8`.
   - Below 1024 px the bar therefore starts 16 px (at 320) or 8 px (at 640 and 960) left of the content, twice that at 200% text.
   - Fix the content, not the bar: `px-4 sm:px-6 lg:px-8` on /agents, /org-chart and /offices. I've added this rule to design-system's product decisions.
   - Evidence: `s2/layout/shots/after-webkit-agents-960x640-100-top.png`.
3. **S4, layout, spec-level.** At 320/200% the bar scrolls sideways by 16 px of its own right padding, though both links are fully visible. This comes from §12's `overflow-x-auto` + `px-4`. It can stay as it is.
4. **S4, focus ring, spec-level.** The inset focus ring touches the label, because the §12 tab has no side padding. Fixed in §12: the tab gets `px-1`. Evidence: `c2-focus-agents-Orgchart.png`.
5. **Pre-existing, no worse:**
   - /agents at 200% text from 320 to 960: main scrolls sideways (AG-1 and SH-18).
   - Each page title appears twice, in the top bar and on the tab (PageHeader work, §4/§5).
   - The inner Org chart tabs are a filled pill group, which §12 says not to use.
   - The sidebar still marks "Org chart" current on /org-chart; that's expected until S1.
   - WebKit logs a ResizeObserver console error on /org-chart at 1280/200%, in both builds.

## Tomas's question: the OfficeStatusBadge icon above "Online" at 320 px / 200% text

**Not a regression and not new: S4, pre-existing.**
- 905dfb2f (before BUG-76), 2efce5d5 (BUG-76) and 673e1cb7 render it identically: the badge is 128 × 84, the icon sits above the label, and the word stays on one line. WebKit and Chromium give the same result in all 702 cases measured.
- One line doesn't fit: "Online" needs 145.5 px, and the row has 128 px inside its padding. So this isn't "fits but wraps".
- Cause:
  - The Offices page uses a fixed `px-8`. At 320 px that leaves 128 px inside the row.
  - OfficeStatusBadge passes `flex-wrap` to StatusPill, so the icon drops above the word.
- Fix:
  - **Gutters:** use the standard narrow-width gutter (finding 2). That gives 192 px, so Online, Starting, Invited, Offline and Stopped fit beside their icon.
  - **Badge:** drop `flex-wrap` from the badge, and add `min-w-0` to the pill and its wrapper, so the long labels (Update needed, Unreachable, Checking…, Status unknown) truncate with an ellipsis per the product decision.
  - This also fixes BUG-76 F1: long labels on the 320/200% row are cut off with no ellipsis, which is still open at 673e1cb7.
- Correction to my BUG-76 report: F2's "one line fits" compared against the outer width of the row. The inside width is 192 px, so it doesn't fit. I've corrected the report.
- Evidence: `s2/badge/shots/real-row-{905dfb2f,673e1cb7}-webkit-320@200-Online.png` (identical), `s2/badge/probes/sweep-{webkit,chromium}.json`.

## Catalog

Updated at 673e1cb, both validated:
- Agents roster, now with the bar.
- Org chart · Teams, now with the bar.
