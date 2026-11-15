# Second recheck: requests-3 (app-frontend-dev/requests-3 @ 1ff1a473 vs 292e5960)

**Verdict: CHANGES NEEDED.** F1 is still S2, and at 390 and 320 with 200% text it's **worse than on 292e5960** (a regression in this build).

At 390×844 and 320×640 with 200% text, the capture panel can't be used:
- The panel's body shows 8 px of height in both. At 390 it showed 152 px on 292e5960; at 320 it was already 8 px.
- The content is 133 px (390) and 203 px (320) too wide. That's the same amount as on 292e5960, but now the body overflows inside the panel, where before the panel itself overflowed.
- The footer's stacked buttons are taller now (inference from the diff: their labels wrap). Send is 122 px tall at 320. Send and Just save stay inside the panel, but Cancel runs past its bottom (Cancel wasn't measured before).
- At 320 the heading and the close X are pushed above the top, out of reach (heading at y −432). On 292e5960 the heading's top was at y 80, though cut off on the right.

The other 8 of 12 cases per engine pass (3 sizes × 2 text sizes × 2 states). All of 1280, and 390 and 320 at 100%, fit.

**Fixed:** F3, N1, N2, and the completion caret case. F9 is fixed apart from one class.

**Partly fixed:**
- **F7:** the queue no longer scrolls sideways at any size, but two buttons are 24 px below 640 with 100% text.
- **F5:** every ring is now indigo-400 at 3:1 or more, but Search has no outline, four offsets are off, and in WebKit the radios lose their ring.

**New:**
- the request's page scrolls sideways at 200% text on narrow screens (S3; not measured before, probably already there);
- two S4s.

The requests e2e suite passes. Apart from F1, the flows show no regressions.

- **Requested by:** app-frontend-dev (01M78WTC), after fixing my first recheck (`ux-pass-requests-3-recheck.md`).
- **Checked:** 12 Nov 2026 (late), by Eden.
- **Scope:** as before. The office-stopped state, the project's Requests tile, the RQS-6 hint, RQS-17 and RQS-18 are in requests-4. Message instead is judged as hidden.
- **Against:**
  - the spec, `knowledge/specs/feature-requests-ux.md` rev 4 (§1.1, §2, §3; RQS-4, 13, 15, 16);
  - the design system v2.5 (§6, §7 and D23, §11, §17, §18, §20);
  - Emery's vision lens (§3, §4). The branch has no Inbox or Home, so those rules are not applicable.
- **Method:**
  - Three checks, each in its own worktree at 1ff1a473 with the mock data (`--mode e2e`):
    - A: the capture panel;
    - B: the queue and the request's page;
    - C: the flows, the lens and regressions.
  - Each check ran WebKit first, closed it, then ran Chromium. At most two checks ran at once, so at most two browsers.
  - The 200% runs set the root on every page, and the 32 px root was confirmed before each measurement.
  - Counts are per engine, and results match in both engines unless noted.
  - The screenshots and `results.json` are in the session scratchpad under `rq3c/{A,B,C}/`. They aren't committed.
  - The requests e2e suite at 1ff1a473 gives `46 passed (2.6m)` (Chromium, 1 worker, after `bun run build`).

## Findings from the earlier passes

| # | 292e5960 | Now (1ff1a473) | Measured | Shot (opened) |
|---|---|---|---|---|
| F1 · S2 · capture panel at 200% on narrow screens | Partly fixed | **Not fixed, still S2, and worse at 200%** | **Compared with 292e5960:** at 390 with 200% the body went from 152 px to 8 px, and at 320 the heading's top went from y 80 to y −432. Inference from the diff of `NewRequestDialog.svelte`, not measured: the footer buttons now wrap their labels (`h-auto whitespace-normal`) and grow taller, and the heading has extra right padding, so they take the space.<br>**Pass:** 8 of 12 per engine. All of 1280, and 390 and 320 at 100%, both empty and with long text. Inset is 16 px or more, the body scrolls, the footer sits inside the panel, nothing is clipped, and the X is clear of the heading. At 1280 with 200% text the body shows 364 px, and the text field (224 px) is fully visible and can be typed in.<br>**Fail:** 4 of 12, the same in both engines: 390 and 320 at 200%, empty and with long text.<br>- The body's visible height is **8 px**, because the description and the three stacked footer buttons fill the panel. Typing worked once the field was scrolled into view (inference: Playwright's click scrolls it into view on its own, which a person can't do through an 8 px window).<br>- The body's content is 363 px wide in a 230 px (390) or 160 px (320) body. The Kind select and the "Who scopes it" fieldset are cut off on the right (fieldset to x 439, panel to 358).<br>- Send and Just save are inside the panel (390: 600–682 and 698–770; 320: 356–478 and 494–576), but Cancel runs past it: 786–858 against a panel bottom of 812 (390), and 592–664 against 608 (320). Cancel wasn't measured on 292e5960.<br>- At 320 the title is at y −432 and the X at y −448, so they can't be reached.<br>- 13 (WebKit) and 14 (Chromium) elements lie outside the panel at 390, and 30 in both at 320.<br>Inference, not checked in the code: the panel's spacing grows with the text size while the window doesn't. The developer's e2e checks the panel against the viewport, but this failure is inside the panel | A/shots/chromium/fit-390x844-200-empty.png; A/shots/webkit/fit-320x640-200-long.png |
| F3 · S4 · text after the handle | Partly fixed | Fixed | "@sam,, x", "@sam — x" and "@sam - x" save as "x". "@Sam Okafor rest" saves as "rest" and reads "Sent to Sam Okafor", on the row and on the request's page | A/shots/webkit/strip-queue.png |
| F5 · S3 · focus rings (§17, §11) | Partly fixed | **Partly fixed, S3** | **Every ring measured is 2 px solid indigo-400**, 4.03–4.69:1 against what's behind it, in both engines. Correct offsets: −2 on tabs, selects and row titles; +2 on New request, Review and Route, Clear filters, Show all, the request page's buttons, and the capture panel's Cancel, Just save, Send, Close and radios. That includes a keyboard-opened panel in WebKit (reached with Option+Tab).<br>**Still wrong:**<br>- **Search has no outline at all.** Its outline style computes to `none` because the input's own `outline-none` wins, so focus shows only as a 1 px indigo-500 border (3.21:1), not §17's ring.<br>- **Offsets:** the group headers 0 and Retry 0 (should be −2 and +2); the From link 0 in WebKit and 1 in Chromium (should be +2); the panel's inset controls (text field, Kind, Project, Someone else) −1 (should be −2).<br>- **WebKit only:** moving through the "Who scopes it" radios with the arrow keys leaves the newly checked radio with no ring (`:focus-visible` false, outline none). Chromium shows it. This wasn't measurable before, because WebKit's plain Tab skipped the radios | B/shots/chromium/focus-search.png; A/shots/webkit/focus-radio-2.png |
| F7 · S3 · narrow layout (§20) | Partly fixed | **Partly fixed, S3** | **At every size** (390, 320 and 800 at 100%; 390 and 320 at 200%; 1280), html, body and main don't scroll sideways and nothing sticks out of main.<br>- The tab bar wraps, and the selects stay inside the viewport (right edges 370–372 at 390, 189–190 at 320, 352–358 and 288 at 200%).<br>- No title breaks inside a word, and gutters are 16 px below 640 at 100%.<br>- Below 640 at 100%, the tabs, selects, Search, New request, Review and Route, group headers and row titles are 44 px or more.<br>**Still short:** **Clear filters** and **Show all requests** are 24 px tall at 390 and 320 with 100% text (48 and 96 px at 200%) (44 required) | B/shots/chromium/narrow-320x640@100-nomatch.png |
| F9 · S4 · gray Scoped pill (D23) | Partly fixed | Fixed apart from one class | Gray-400/10 background, gray-300 text (7.25–7.28:1), a file-check icon, and no amber on the row. The 1 px gray-400/20 ring isn't inset (`ring-inset` missing), a 1-class difference | B/shots/chromium/scoped-pill-row.png |
| N1 · S4 · amber validation text | New | Fixed | In the capture panel, the unknown-handle, partial-handle and too-long messages are red-300 (7.64:1). The empty message and "Write what you need after @sam." are red-400 (about 5.1:1). The route warning is red-200 on red-500/10 (9.48–9.52:1). Amber is left only on the Needs you header and the tab badge (computed on 5 screens) | A/shots/webkit/n1-route-warning.png |
| N2 · S4 · the owner named two ways | New | Fixed | A request the owner raised reads "Raised 12 Nov by you" and "You raised this". One an agent raised reads "by Juno Park" | B/shots/chromium/n2-owner-request-sheet.png |
| Caret (item 3, RQS-4) | Inconclusive in Chromium | Fixed | In both engines, clicking at the end of the text (caret at 9) or pressing ArrowRight past the handle (caret at 4) closes the list. In WebKit, Home and End don't move the caret in a text field. That's macOS behaviour, and Cmd+→ closes the list | A/shots/webkit/caret-click-end.png |

## New in this pass

- **N3 · S3 · the request's page scrolls sideways at 200% text on narrow screens (§20).** Not measured in earlier passes. Probably already there: `RequestSheet.svelte` changed only one line (l. 189) between 292e5960 and 1ff1a473.
  - At 390 and 320 with 200% text, the page's content is 466 px wide in a 292 px (390) or 239 px (320) page.
  - It's pushed wide by the state pill "Scoped by Juno Park" (no wrapping) and the action buttons ("Approve and create 2 tasks…" reaches x 547). The no-wrap and the x 547 come from a probe's console output, which wasn't saved, and the shot is Chromium only. The 466 px width was measured in both engines.
  - Fix outcome: pills and buttons wrap to the page's width, with no sideways scroll.
  - Shot: B/shots/chromium/sheet-320x640@200-scrolled-sideways.png
- **N4 · S4 · amber paragraphs on the request's page (lens §3.7, design system §9: no amber text paragraphs as notes).** This was already there in 83ca9b13. I read it from the code; the mock data can't trigger it. `RequestSheet.svelte` l. 254, 272 and 280 use amber-200 for three messages:
  - "No tasks were proposed, so there is nothing to approve."
  - "{name} is paused and won't pick this up."
  - "There is no project to put the tasks in."
  - Fix: use the neutral or error tone, as N1 did in the capture panel.
- **N5 · S4 · filter selects at 640 px and wider (§11).** They're 20 px tall in WebKit and 31 px in Chromium, beside a 36 px Search. Their border is white/10 (1.37:1), where §11 asks for a gray-500 border. Not measured before. Fix outcome: selects at least 36 px tall with a gray-500 border, as §11 says. Shot: B/shots/webkit/narrow-800x600@100-queue.png
- Outside this branch's scope (it still has the old shell): the sidebar links' focus ring is indigo-500, 2.91:1 on the current Work item.

**Measured but not counted as a finding:** at 200% text the side gutters are 32 px, because the padding is set in rem. Doubling with the text size is expected.

## Flows and regressions (both engines, 1280 and 390 at 100%)

All of these work:
- an exact @handle gives "Sent to Sam Okafor · not accepted yet";
- Just save lands in Not routed;
- focus returns to the row after the request's page closes, and to New request after each of the panel's 5 ways of closing;
- Similar requests' **Add to REQ-2** adds the words to REQ-2 with no new row;
- Merge into… and Decline… appear only on open requests (present on 1 of 1 for each open state; absent on the declined one);
- the State, People and Project filters narrow and are kept after a reload;
- Work reopens on its last tab;
- Tasks and Bugs render with no sideways scroll, and Debt stays disabled with its reason.

Lens checks:
- the counts agree (header "1 item needs you" = badge 1 = "Needs you · 1");
- there's no emoji;
- there are no machinery words in the Requests or Work copy. The old shell's Sessions, Processes and Offices nav isn't this branch's.

Shot: C/shots/webkit/1280-vision-queue.png (opened).

## What's next

app-frontend-dev:
- **Must fix:** F1 at 390 and 320 with 200% text. Required outcome:
  - the text field shows at least 3 lines;
  - nothing in the panel is wider than the panel;
  - the heading and the X stay on screen;
  - the footer stays inside the panel and in view.

  One way to get there, your call: when the panel is short, let the description and the footer scroll with the body, and keep the footer's buttons in a row while they fit.
- **Should fix (S3):**
  - N3, the request's page at 200%;
  - F5: Search's outline, the four offsets, and the WebKit radio ring after arrow keys;
  - F7: Clear filters and Show all requests at 44 px below 640 (24 px now at 100% text).
- **Ideally (S4):** N4, N5 and F9's `ring-inset`.

Then I recheck those, one browser at a time.
