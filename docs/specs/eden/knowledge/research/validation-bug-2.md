# Validation · BUG-2 Org chart toolbar wrapping (v1 034628d6 vs b595515b)

Requested by ada (01M75SBGY5HYKYTQTTRK3WDGXV). Jules, jules/bug2-org-toolbar @ 3f2a96a7, Tier B, Sol READY. The merge also carries the BUG-87 test-flake fix.

**Verdict: fixed.** R1 to R5 are met on all three tabs (Teams, People, Reporting lines):
- at 320, 640, 720, 960 and 1280, plus 639 for the search edge;
- at 100% and 200% text;
- in WebKit and in Chromium.

The build before the fix fails in 27 of the 60 WebKit cases. The fix adds no new problem beyond two cosmetic ones (F5). Several pre-existing layout and accessibility problems remain; they are below, and most are already in my BUG-70 list.

**Method:**
- 200% text means the root font-size set to 200%, the same method the e2e spec uses.
- "Sideways scroll" means `<main>` scrollWidth greater than clientWidth. The page scrolls inside `<main>`; the document itself never scrolls sideways in either build.

## Requirements (evidence I opened)

| Req | Before (b595515b) | After (034628d6) |
| --- | --- | --- |
| R1: every action reachable at 960×640 (Teams) | 100%: main 1058/960, "Collapse all" off-screen. 200%: main 2103/960, 9 controls off-screen (WebKit and Chromium) | 960/960, 0 controls clipped or off-screen. The toolbar wraps to 2 rows at 100% and 7–9 rows at 200% |
| R2: wraps at 720, 640 and 320, tidy at 1280 | Teams main 834, 834 and 770 at 100%; 1655, 1655 and 1527 at 200%. 1280 at 200%: 2103 | Equal to the viewport everywhere. 1280 is one row at 100% |
| R3: search full width below 640 | Fixed at 256 px (100%) or 512 px (200%). At 320/200% it is 512 px in a 192 px column | 320: 256/256 and 192/192. 639: 575/575 and 511/511. At 640 it is 256 px (100%), which is correct because the rule applies below 640 |
| R4: no sideways scroll at 720 and below | Fails on Teams at every width. People and Reporting lines fail at 320/100% (342/320) and at every width at 200% | main scrollWidth equals clientWidth in all 60 WebKit and 36 Chromium after-probes |
| R5: 100% and 200% text | Fails at both sizes | Met at both |

**BUG-87 (the flake):** `org-chart-responsive.spec.ts --repeat-each=5 --workers=2` gave "270 passed (8.5m)":
- Chromium 135 and WebKit 135, with 0 flaky and no retries.
- The focus-order tests at 320/200% and 960/200% each passed 5 of 5 in both engines.

**Walkthrough (after):** every control still works once the toolbar wraps. I tried:
- a live resize 1100 → 320 → 960 with a search and an Office filter set; both are kept;
- a 235-character search and a very long office name; both stay inside the viewport;
- switching tabs while wrapped; the search and filter are kept.

Tab order matches the visual order, and every control has an accessible name.

## Findings (none caused by BUG-2)

1. **S3, layout, pre-existing (new bug):** at 960 px with 200% text, the desktop sidebar is 576 px wide because its width scales with text. That leaves a 256 px content column:
   - the tabs stack and "Reporting lines" wraps;
   - the search placeholder is cut to "Search membe";
   - the Teams chart starts at y=1745.

   At 959 px the content gets 703 px. Expected (design-system §20): content stays usable at 200% text. Suggested fix: size the sidebar in px, or switch to the rail when the content column would drop below about 480 px. Evidence: chromium `shots/after-teams-960-200.png`.
2. **S3, a11y, pre-existing (new bug):** touch targets below 640 px are smaller than 44 px:
   - buttons are 32 px;
   - tabs are 36 px;
   - the WebKit native Office select is 20 px.

   Expected: 44 px (design-system §10 and §20). Evidence: blitz `probes/after-order-and-targets.txt`.
3. **S3, UX, pre-existing (new bug):** on a phone the Org chart is hard to use:
   - at 320×640 the chart starts at y=804, below the first screen, after the header blocks and a 5-row toolbar;
   - People in Fit mode shrinks the chart until its text can't be read;
   - the Teams chart in Fit mode is still 843 px wide in a 256–608 px area.

   OC-1 and OC-4 in my BUG-70 list reduce the header and the toolbar; the Fit behaviour on phones needs its own fix. Evidence: catalog `org-chart-people-phone`.
4. **Already in the BUG-70 list (not refiled):**
   - The focus ring is shadcn `ring-3` at 50%, which is 1.75:1 (SH-20).
   - The Search and Office borders are 1.35:1, and the placeholder is 3.03:1 (F-8).
   - Phone gutters are 32 px; at 200% they are 64 px (SH-18).
5. **S4, visual:** at 320 the wrapped toolbar is right-aligned and ragged, and at 200% "Collapse all" wraps inside its button. OC-4 (a "View ▾" menu) removes this.

**In the spec but not built** (offices-and-teams-ux.md, about line 965; not counted against BUG-2):
- There are no zoom controls (− % +), only an Actual size / Fit to width toggle.
- Reporting lines has no Office filter.
- List view exists only on Teams.
- Search sits above the tabs rather than in the toolbar.
- Teams also shows Add person and Add agent.

## Coverage and evidence

Evidence is in my session scratch, `bug2/{webkit,chromium,blitz}`:
- WebKit: 60 before and 60 after probes, plus toolbar crops.
- Chromium: 72 probes, plus `flake-run.txt`.
- Walkthrough: the matrix, stress, keyboard and contrast probes.

The catalog now has Org chart · Teams, · People and · Reporting lines at 034628d, marked validated.
