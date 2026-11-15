# Validation: BUG-52 (responsive shell nav) and BUG-53 (unavailable pages), v1 2b1f39b vs b66dc92

- **Build under test:** v1 `2b1f39b` (Jules, merge of `jules/shell-responsive-unavailable`). **Parent:** `b66dc92`.
- **Requirements:** `knowledge/research/decision-narrow-widths-2026-11-10.md` (SHELL NAV, 320 px EXPECTATION, UNAVAILABLE PAGES), plus the agreed follow-ups: (a) drawer = min(288, 100vw − 56) so the scrim always shows; (b) no global badge in BUG-52, but the rail must render one if a nav item supplies it; (c) unavailable pages keep the wrapping text nav, no Menu, no sideways scroll at 320. Chat's own layout (BUG-49) is out of scope.
- **How:** throwaway worktrees A = `b66dc92`, B = `2b1f39b`. App screens on `bun run dev --mode e2e` (A 5251, B 5252); unavailable pages on B's static release (`bun run build`, `bun run preview`, 5253). WebKit is the main engine, Chromium a spot check. I tested as a reviewer only and changed nothing in the product repo. The worktrees and servers are gone.
- **Date:** 2026-11-10.

## Verdict

BUG-53 is met in full. BUG-52 is mostly met: the three nav forms, rail names, tooltips, aria-current, logo switcher, Menu semantics, focus trap, item-choice focus, the drawer width rule and the unchanged ≥960 layout all hold. Two gaps: **in WebKit (Safari), closing Menu with Esc, the scrim or Close drops focus to the page body when Menu was opened with the mouse** (N1), and **the badge in (b) has no way in**: `NavItem` has no badge field and the rail renders none (the icon box is ready for one). The 320 px rule still fails on Org chart (sideways scroll at every width below 960, BUG-2's toolbar plus a fixed-width search field, N2) and on Processes at 320 (a 54 px text column, N3), and on Chat (BUG-49, out of scope).

## Requirement by requirement

| # | Requirement | Result | Evidence |
|---|---|---|---|
| 1a | ≥960: full nav, as today | Met | `nav-webkit/nav.json`: 1440 and 960, all 12 routes `form: full`, 288 px, 11 links |
| 1b | 640–959: 64 px icon rail | Met | 959 and 720, all routes `rail`, 64 px, 10 items (`nav-webkit/B/959/*.png`, `B/720/*.png`) |
| 1c | Rail items: icon only, name as accessible name | Met | each item `aria-label` = name, visible text sr-only; 0 unnamed |
| 1d | Tooltip on hover and on focus | Met, one S4 gap | hover and keyboard (Option+Tab) show the item's name for all 10 items + switcher at 959 and 720×900, both engines (`nav.json` `railWalk`, `rail-extra/rail-extra.json`, `B/720/tooltip-*.png`). Gap N4: at 720×450 a focus that scrolls the rail shows no tooltip |
| 1e | aria-current marks the current page | Met | exactly one `aria-current=page` per route, matching the area (incl. `/projects/beacon` → Projects, `/org-chart?member=kai` → Org chart) |
| 1f | Org switcher becomes the logo button | Met | 48×48 link, logo only, name "Switch organization (now Acme, HQ office on This Mac)", tooltip "Acme · Switch organization" |
| 1g | No expand toggle | Met | no extra button in the rail |
| 1h | (b) Rail renders a badge if a nav item supplies one | **Partly met** | Icon box is `relative` and unclipped (injected badge drawn in full, 4/4 corner hits, `rail-extra/*-badge-720.png`), but `NavItem` is `{ name, href, icon }` (`src/lib/nav.ts:3-7`) and `Sidebar.svelte:66` only leaves a comment: a nav item cannot supply a badge, so nothing renders |
| 1i | <640: rail hidden, 56 px top bar with Menu and page title | Met | 639/600/320 all routes `topbar`, header 56 px, h1 = area name, no rail in the tree |
| 1j | No sideways page scroll | **Not met on 2 routes** | `main` scrolls sideways on Org chart at 959: 0, 720: 114 px, 639: 131, 600: 170, 320: 450 (N2); Chat 320: 45 px (chat-only, BUG-49). Every other route 0 at every width |
| 1k | No text column narrower than 200 px | **Not met on 3 routes** | Processes 320: title, subtitle and sample note in a 54 px column (N3, `nav-webkit/B/320/processes.png`); Org chart 320: "Reporting lines" tab 109 px, 3 lines (part of N2); Chat 320: message text 6 px wide (chat-only, BUG-49). Agents roster cells are inside the table's own scroll region (exempt; BUG-30) |
| 1l | ≥960 unchanged vs parent | Met | 48 pairs (12 routes × 1440/960 × page + nav strip): **0 px** different, all byte-identical |
| 2a | Menu has aria-expanded and aria-controls | Met | `false`→`true`, `aria-controls` = dialog id (`c12`), `aria-haspopup=dialog`; dialog named "Main navigation", `aria-modal=true` |
| 2b | Focus goes to the current item on open | Met | Offices (and Chat on /chat) focused on open, all sizes, both engines |
| 2c | Focus trapped while open | Met | 13 Tab and 13 Shift+Tab steps never leave the dialog (`trapEscaped: 0`), all 4 sizes |
| 2d | Esc, scrim, Close → focus returns to Menu | **Met in Chromium, not met in WebKit** after a mouse open (focus → body, N1). Keyboard open returns to Menu in both | `menu-focus-return.json` |
| 2e | Choosing an item → closes, focus to new page's h1 | Met | mouse and Enter: URL changes, drawer closed, focus on `h1#shell-page-title` ("Projects"/"Chat"), `aria-expanded=false`, one `main` |
| 2f | (a) Drawer width min(288, 100vw − 56) | Met | 639 → 288 (scrim 351), 600 → 288 (312), 320 → 264 (56), 320×256 → 264 (56) |
| 2g | Resize while open releases the trap | Met | widening to 720 closes it and focuses the rail's current item |
| 3a | Root heading and body, exact | Met | `unavailable-webkit/unavailable.json` `/`: h1 and body equal the decision text |
| 3b | Area heading and body, exact, every area | Met | all 10 areas + `/projects/beacon` + `/org-chart?member=kai`, 1440 and 320 |
| 3c | `agentic-org doctor` in a code span | Met | exactly one `<code>` with that text in every body |
| 3d | Tab titles incl. 404 | Met | "Agentic Org", "<Area> · Agentic Org", `/nope` and `/projects/beacon/extra` "Page not found · Agentic Org" |
| 3e | Home link focus: 2 px `--ring`, 2 px offset | Met | keyboard focus: `solid 2px`, offset `2px`, colour = resolved `--ring` (oklch 0.585 0.233 277.1), both engines (`unavailable-*/…-home-focus.png`) |
| 3f | No role="status", no buttons | Met | 0 and 0 on every path |
| 3g | (c) wrapping text nav, no Menu, no sideways scroll at 320 | Met | nav wraps to 4 rows at 320, no Menu, `hscroll 0`, nothing past the right edge (320×900 and 320×256) |
| 3h | Nav keeps the other areas reachable | Met | client-side clicks Settings → Chat → Org chart, Back, home link: heading, title and aria-current follow |
| 4 | Regression set 1440×900 and 960×640 | Met | 124 pairs, 121 byte-identical; 3 differ only in Chat's "Waiting N min" clock (see §4) |
| 5 | check, unit, build, e2e on 2b1f39b | Met | §5 |

## New findings

### N1 (S3): WebKit drops focus to the page body when Menu closes after a mouse open

- **Repro:** Safari/WebKit, 600×900 (or any width below 640). Click Menu, then press Esc (or click the scrim, or Close).
- **Expected:** focus returns to Menu (decision; WCAG 2.4.3).
- **Actual:** focus is on `body`. Opening Menu with the keyboard (focus Menu, Enter) returns to Menu, and Chromium returns to Menu either way. The cause is that WebKit doesn't focus a button on click, so the dialog's default "return focus to what had it before" finds the body.
- **Evidence:** `menu-focus-return.json` (webkit open:mouse → `body` ×3; open:keyboard → `button Menu` ×3; chromium → `button Menu` ×6); `menu-webkit/*/menu.json` `esc`, `scrim`, `closeBtn`.
- **Code:** `src/lib/platform/shell/MobileNavigation.svelte:32` `if (closeReason !== 'resize') return;` leaves the dismiss case to Bits UI's default. Fix: on `dismiss`, `preventDefault()` and `trigger?.focus()` explicitly. The e2e (`shell-responsive.spec.ts`) runs only on Chromium, so it can't catch this.

### N2 (S3): Org chart scrolls sideways at every width below 960, worst at 320

- **Repro:** Org chart (Teams) at 720, 639, 600 or 320.
- **Expected:** the page never scrolls sideways; only the chart canvas scrolls inside its own region.
- **Actual:** `main` scrolls sideways by 114 / 131 / 170 / 450 px. The toolbar row (New team, Add person, …) doesn't wrap (that's BUG-2's cause, catalogued only at 960×640), and at 320 the 256 px search field also runs off the edge (label + input end at 342 px) and "Reporting lines" squeezes to 109 px on 3 lines. Same in Chromium.
- **Evidence:** `nav-webkit/B/320/org_chart.png`, `nav.json` `B/*/org-chart` `mainX`, `offRight`.
- **Code:** `src/routes/(shell)/org-chart/+page.svelte:360` (`w-64` on the search input); toolbar row as in BUG-2. Could be filed as an extension of BUG-2 to the new widths rather than a separate bug.

### N3 (S3): Processes header squeezes to a 54 px column at 320

- **Repro:** Processes at 320 wide.
- **Expected:** no text column narrower than 200 px; the Show control wraps below the title.
- **Actual:** the title, subtitle and "Sample data" note sit in a 53–54 px column beside "Show [Organization]", breaking mid-word ("Proces/ses · Organi/zation", 14 lines of subtitle). Both engines.
- **Evidence:** `nav-webkit/B/320/processes.png`; `nav.json` `B/320/processes` `narrow`.
- **Code:** `src/routes/(shell)/processes/+page.svelte:64`: the title block is `min-w-0 flex-1` (basis 0) in a `flex-wrap` row, so it shrinks instead of wrapping the label to the next line. Give it a minimum basis (e.g. `basis-[200px]` or `min-w-[200px]`).

### N4 (S4): At 720×450 (200% zoom) a focus that scrolls the rail shows no tooltip

- **Repro:** 720×450, Offices. Focus Chat, then Option+Tab down the rail. When focus reaches the item that makes the rail scroll (Activity), no tooltip shows; the next ones do.
- **Expected:** a tooltip on focus for every item.
- **Actual:** WebKit: Activity shows none (`rail-extra.json`, scrollTop 0 → 34). Chromium shows it on that path but missed the switcher after the rail scrolled back, and hovering Activity in the walk. The tooltip closes when its rail scrolls. The accessible name is unaffected.
- **Code:** `src/lib/components/Sidebar.svelte:29` (rail is the `overflow-y-auto` scroller) with the Tooltip.Root at `:59`.

### Note (not a finding): Back with Menu open

Browser Back while Menu is open closes it and lands on the previous page with focus on `body` (both engines). The decision doesn't cover Back; listed so it's a known behaviour.

## 4. Pixel comparisons

- **Nav probe, ≥960 (A vs B):** 12 routes × 1440×900 and 960×900, whole page and the 288 px nav strip: 48 pairs, **48 byte-identical, 0 px** differ.
- **Main screen set (validate.mjs, 31 steps from Welcome to 404, dark and light):** 1440×900 and 960×640: **124 pairs, 121 byte-identical, 3 differ** (751 px each): `default-dark/20-chat`, `960x640-dark/20-chat`, `960x640-light/20-chat`. The only text change is "Waiting 8 min" → "Waiting 9 min" and "Waiting 11 min" → "Waiting 12 min" in the Needs-you items (the mock's wall-clock age crossed a minute between runs); the diff boxes are those lines. Not a regression.
- **Behaviour diffs:** only `31-not-found` title `""` → `"Page not found · Agentic Org"` (BUG-53, intended). No failures; the same 3 console messages in A and B (the 404).

## 5. Local checks on 2b1f39b (B worktree, each through `~/.agency/bin/heavy`)

- `bun run check`: `COMPLETED 1160 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS` (exit 0)
- `bun run test`: `Test Files  50 passed (50)` / `Tests  1344 passed (1344)` (exit 0)
- `bun run build`: `Wrote site to "build"` / `✔ done` (exit 0)
- `bun run test:e2e --workers=2` (`PW_PORT=4391`; builds, then the `chromium` and `production` projects): `273 passed (8.2m)` (exit 0)

## Already catalogued, seen again, not re-reported

BUG-2 (Org chart toolbar at 960, mainX 98), BUG-30 (Agents roster cells wrap at 61–138 px; at <960 the table scrolls in its own region), BUG-49 (Chat at 320: 45 px sideways, 6 px message column).

## Not covered

Dialogs and sheets at 320 (full-width rule) were outside this brief's list and weren't measured. The "Needs you" count itself is BUG-55.

## Method and files

All under `workspace/validation-bug-52-53-shots/`:

- `validate.mjs` (copied from validation-f1-03; overflow scope changed to `main`), output `regress/` (`summary.json`, `results.json`, `diff/`).
- `nav.mjs` (new): forms, items, switcher, overflow, narrow text, rail walk, badge probe, ≥960 pixel compare. Output `nav-webkit/` (A and B, 7 widths × 12 routes) and `nav-chromium/` (B, 959/720/639/320 × 6 routes).
- `menu.mjs` (new): Menu at 639, 600, 320 ×900 and 320×256. Output `menu-webkit/`, `menu-chromium/` (600, 320).
- `menu-focus-return.mjs` / `.json`: N1 by open and close method, both engines.
- `rail-extra.mjs`, `rail-extra/`: N4 and the badge render check.
- `unavailable.mjs` (adapted from srv18.mjs): output `unavailable-webkit/`, `unavailable-chromium/`.
- `landmarks.mjs`, `srv18.mjs`, `blitz.mjs`: copies of the earlier scripts (path-adjusted), not re-run.
- Jules's screens (`app-jules-shell/.../shell-merge-2b1f39b/screens`) agree with what I saw in Chromium; they include no WebKit run, which is why N1 wasn't visible there.
