I validated Jules's non-Chat pseudo-icons (v1 08db0992 vs 0a87472f).

Verdict: met, no regressions.
- No × or › left on the project page or the org chart, in either engine, at 320–1280, 100/200% text.
- x-mark and chevron-right are byte-equal to Heroicons v2.2.0, and both are aria-hidden.
- Accessible names unchanged, and 80/80 removes work.
- Targets went from 16×20 to 24×24. 0 blitz errors. Unit guard: Tests 20 passed (20).

Report: projects/ac/agents/ac-product-designer-eden/knowledge/research/validation-non-chat-icons.md. The catalog is republished (v11): project-beacon added, org-chart refreshed.

Proposed BUG rows. Owner Jules unless you say otherwise; all of them are small.
1. S4, new: breadcrumb chevron on /projects/{id}.
   - It's text-gray-500 (3.03:1, §18 decorative) while its nav text is gray-400. §22 says the icon takes the control's colour.
   - It also sits 1.3–2.5 px low.
   - Fix: drop text-gray-500 so it inherits; lay out the nav as flex items-center, or as an <ol> per row 5.
2. S4, new: both new icons use mini (20 px) artwork forced to size-4! (16 px). At the chart's 0.75 Fit they draw at 12 px.
   - Fix: render mini at size-5 (20 px fits the 24 px button and the text-sm/6 line), or use micro at size-4.
3. S3, pre-existing but improved: the org chart Teams Fit view scales the 24 px remove button to 18 px (≤960) and 19.6 px (1280).
   - It passes WCAG 2.5.8 only through the spacing exception (15–16.4 px to the nearest target).
   - Fix: counter-scale the button (size calc(1.5rem / var(--fit-scale))). Owner: Avery (chart).
4. S3, pre-existing: removing a non-last team with Enter, on /projects/{id} and /org-chart, drops focus to <body> (2.4.3).
   - Fix: focus the next chip's Remove, else the previous one's, else "Add team…". Same rule as Chat attachment remove (P2 in my Chat report).
5. S4, pre-existing: the breadcrumb is not an <ol>/<li> and has no aria-current="page" on the last item.
6. S4, pre-existing: remove focus has outline-offset 0, but §17 says offset-2.
7. S4, pre-existing: "Add team…" has accessible name "Assign a team to Beacon", so the visible text isn't in the name (2.5.3). Fix: "Add team to Beacon".
8. Adds to BUG-112: the same intro and Sample-data clipping also happens at 960/200 (paragraphs end at x=979 on a 960 viewport), in both engines, before and after. Fix: min-w-0 max-w-prose inside the page column.
9. Adds to BUG-89: phone remove targets are 24 px (18 px in the fitted chart), but §20 asks for 44. Fix: max-[639px]:size-11.
10. Adds to BUG-70: Lucide Info (project Callout) and ChevronDown (TeamsChart) still sit beside the new Heroicons. Fix: information-circle and chevron-down, both mini.

Separately: I pinned BUG-94's sidebar layout and BUG-95's fallback order with Jules, and approved the product side of his BUG-94 run (bug94-real20-960-2).
