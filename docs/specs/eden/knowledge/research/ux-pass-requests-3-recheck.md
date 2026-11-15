# Recheck: requests-3 (app-frontend-dev/requests-3 @ 292e5960 vs 83ca9b13)

**Verdict: CHANGES NEEDED.**

Fixed: 7 of the 12 findings (F2, F4, F6, F8, F10, F11, F12).

Partly fixed: 5 of the 12 (F1, F3, F5, F7, F9).

One problem still blocks: **F1 at narrow widths with 200% text (S2).**
- At 390×844 and 320×640 with 200% text, the capture panel's content is 133 px (at 390) and 203 px (at 320) wider than the panel, so its right side is cut off (at 390, "Send to Juno Pa…"; at 320, "Send to Jun…" and "Just sa…").
- At 320, the close X sits on top of the heading, and the text field can't be seen.

At 1280 at both text sizes, and at 390 and 320 at 100%, the panel now fits.

Besides F1, three S3 problems remain:
- At 200% text on narrow screens, Work's tab bar makes the main area scroll sideways, and at 320 the State and People selects still run past the viewport (to x=362 in WebKit, x=373 in Chromium).
- The Search box's focus ring (1.74:1 WebKit, 1.77:1 Chromium) and the radios' browser ring in Chromium (1.76:1) are below 3:1.
- Below 640 px at 100% text, targets are under 44 px: the WebKit selects are 20 px, and in both engines New request is 32, the group headers 36 and the tabs 42. At 200% only the WebKit selects stay short (37 px).

There are no regressions (the tab-bar overflow at 200% was already in 83ca9b13, where it was hidden behind the filter overflow; the filters still overflow at 320), and the requests e2e suite passes.

- **Requested by:** app-frontend-dev (01M78QM3), with the fixes for F1–F12 from `ux-pass-requests-3.md`.
- **Checked:** 12 Nov 2026, by Eden.
- **Scope:** ada's split (01M78NF5). These items moved to requests-4 and aren't counted here:
  - the office-stopped state;
  - the project's Requests tile;
  - the RQS-6 bug hint;
  - RQS-17 and RQS-18;
  - Message instead, judged as hidden.
- **Against:**
  - the spec, `knowledge/specs/feature-requests-ux.md` rev 4 (§1.1, §2, §3; RQS-4, 13, 15, 16);
  - the design system, §6 and §7 (D23), §17, §18 and §20;
  - Emery's vision lens (§3, §4). Only the rules for features this branch has were applied; the others are marked not applicable.
- **Method:** three checks, each in its own worktree with the mock data (`--mode e2e`), in WebKit and then Chromium.
  - There was one browser at a time per check, and at most two checks ran at once.
  - The 200% runs set the root on every page through an init script, and the 32 px root was confirmed before each measurement.
  - Counts are per engine. Results match in both engines unless noted.
  - The screenshots and `results.json` files are in the session scratchpad, under `rq3b/{A,B,C}/shots/`. They aren't committed.
  - The requests e2e suite on 292e5960 gives `43 passed (2.5m)` (Chromium, 1 worker).

## Findings from the first pass

| # | Before (83ca9b13) | Now (292e5960) | Measured | Shot (opened) |
|---|---|---|---|---|
| F1 · S2 · capture panel cut off at 200% | Not met | **Partly fixed, still S2** | 8 of 12 cases fit: 1280 at 100% and 200%, and 390 and 320 at 100%. In those, the panel is inset 16 px or more, its body scrolls when the content is taller than the panel, and the heading, Send and Just save are visible. 4 of 12 fail: 390 and 320 at 200%, both empty and with long text, in both engines. The content is 133 px (390) or 203 px (320) wider than the panel and runs to x=443 in the empty state (420 and 350 px with long text in Chromium, where at 320 the content also starts at x=−13, so it's cut off on the left too). The body's visible height is 152 px at 390 and 8 px at 320. At 320 the X overlaps the heading; at 390 it stays clear. The document itself doesn't scroll sideways (the panel clips) | A/shots/chromium/f1-320x640-200-empty.png |
| F2 · S3 · partial handle routes silently | Not met | Fixed | "@sa partial then words" now reads "@sa could be Sam Okafor. Pick one from the list or remove it.", and Send is off. That wording differs from the unknown-handle sentence, but the rule holds | A/…/route-_sa_partial_then_words.png |
| F3 · S4 · punctuation after the handle | Not met | Partly fixed | "@sam,, x", "@sam — x" and "@sam - x" are all saved as "x". But "@Sam Okafor rest" is routed to Sam and saved as **"Okafor rest"**, on the row and on the request's page | A/shots/chromium/route-_Sam_Okafor_rest-page.png |
| F4 · S3 · State options and states | Not met | Fixed | All 7 State options, in the spec's order, with Open as the default; each shows only its own groups. Skeleton rows while loading (`aria-busy`). "Can't load requests." with **Retry** (the wording the spec requires; its placement is unchanged from 83ca9b13, a page-level message that also hides the filters and New request, which the spec doesn't rule on). "No requests match. Clear filters". Not routed appears under State only. Every project is listed. Filters are kept per organization | B/shots/*/01-state-*.png, 04-no-match, 15-cant-load |
| F5 · S3 · focus rings (§17) | Not met | **Partly fixed, S3** | Most queue controls (both engines) and the capture panel's buttons (Chromium only, from A's focus-results.json; A's first run caught some rings mid-fade and isn't counted; in WebKit that second run's scripted focus showed no ring on them) now use a 2 px indigo-400 ring, 4.03–4.69:1. Still failing: **Search** keeps the 50% ring at 1.74:1 (WebKit) and 1.77:1 (Chromium). The first pass recorded it at 3.19:1, but that was a mismeasurement: Search's markup only gained a min-height, and §17 itself puts the 50% ring at about 2:1. So this isn't a regression. The 4 "Who scopes it" radios use the browser's 1 px ring at **1.76:1** in Chromium (not measured on 83ca9b13, so new or old is unknown). Both are below 3:1. Offsets don't match §17 and §11: tabs, group headers and row titles (0 px) and the selects and text field (−1 px) should be inset at −2 px; the free-standing Clear filters (0 px), Show all, Retry and the From link (0 px in WebKit, 1 px in Chromium) should be +2 px. In WebKit, plain Tab skips Cancel, Just save, Send, Add to and the radios (it reaches Close; the engine's default Tab setting). With Option+Tab, Send and Just save are reached, so their ring there is unmeasured | B/shots/webkit/06-focus-search.png; A/shots/chromium/ring-4.png |
| F6 · S3 · handle contrast | Not met | Fixed | The selected option's handle is gray-300, 6.18:1 | B/…/11 |
| F7 · S3 · 390 px layout (§20) | Not met | **Partly fixed, S3** | At 100%: 16 px gutters, no sideways scroll, and 0 of 9 titles break mid-word. But targets below 640 px are under 44 px: the State, People and Project selects are **20 px in WebKit** (44 or more in Chromium), and in both engines New request is 32 px, the group headers 36 and the tabs 42. At 200% (390 and 320, both engines), **the tab bar runs to x=627, so the main area scrolls sideways by 237 px (390) and 307 px (320)**; the document's own width stays at the viewport. At 320 two selects overflow too; at 390 they reach into the right gutter by 15 px (Chromium) and 4 px (WebKit), inside the viewport. The WebKit selects are 37 px. The tab bar's markup is unchanged since 83ca9b13, so this overflow was already there, behind the filters that overflowed first | B/shots/webkit/12-top-390-200.png |
| F8 · S3 · completion semantics | Not met | Fixed | The field is role=combobox with aria-expanded, aria-controls and aria-activedescendant. The list is a listbox. 5 of 5 options have no nested interactive elements | A/…/f8-list-open.png |
| F9 · S4 · gray Scoped pill (D23) | Not met | Partly fixed | There's now a **Scoped** pill with "by Juno Park", and no amber on the row. But it's a white/10 chip with white text and no ring and no `document-check` icon, where §7 asks for `bg-gray-400/10 text-gray-300 ring-gray-400/20` with the icon | B/shots/webkit/07-scoped-row.png |
| F10 · S4 · message not tied to the field | Not met | Fixed | `aria-describedby="handle-problem"` on the field and on Send | A/…/f10-unknown.png |
| F11 · S4 · re-clicking the chosen radio | Not met | Fixed | One click clears the message and turns Send on | A/…/f11.png |
| F12 · S4 · Work forgets its tab | Not met | Fixed | Work reopens on its last tab, per organization. An organization with no requests shows the honest empty state, and Tasks is one click away, so nobody gets stuck | C/…/r-reload-empty-requests.png |

## Items from the first pass

| # | Item | Now |
|---|---|---|
| 1 | State filter, RQS-15 | Met (see F4) |
| 2 | @name routing, RQS-4 | Met. An exact handle gives "Sent to Sam Okafor · not accepted yet". An unknown or partial handle gives a sentence, and Send is off. "@sam" alone gives "Write what you need after @sam." The F3 case is still open |
| 3 | Completion, RQS-4 | Met. ↑↓ move and wrap, Enter and Tab pick, and Esc closes only the list. In WebKit, the list shows only while the caret is in the handle. In Chromium the caret case is inconclusive (the script's caret key may not move the caret on macOS); re-measure on the next pass |
| 4 | People and Project filters, RQS-15 | Met. People: Anyone · Raised by me · Raised by Sam Okafor · Scoped by Juno Park. Project: All · No project · all 4 projects |
| 5 | From link, RQS-16 | Met |
| 6 | Similar requests, RQS-13 | Met. 2 of 3 unscoped look-alikes are shown and the scoped one is left out. **Add to** adds the comment without "@sam" |
| 7 | Message instead | Hidden, as agreed |
| 8 | IA v2 placement, D23 | Partly met (see F9) |
| 11 | Merge into… and Decline… only on open requests | Met. They're present in 4 of 4 open states and absent in 3 of 3 closed ones |

Items 9 and 10 (the office-stopped state and the Requests tile) moved to requests-4.

## New in this pass

- **N1 · S4 · amber used for validation text (vision lens §3.7: amber means "waiting on you").** "Too long to add as a note…" is amber-200 (new in 292e5960). The unknown-handle sentence and the route warning were already amber in 83ca9b13. The unknown-handle sentence was measured on screen in both engines (amber-200, 11.79:1; A/…/f10-unknown.png). The other two are read from `NewRequestDialog.svelte` (l. 334, 381) and weren't triggered. Fix: give inline problems the design system's error or neutral tone. Keep amber for the owner's waits and the warning icon.
- **N2 · S4 · the owner is named two ways (lens §4, pre-existing).** The request's page says "Raised 12 Nov by Alex Kim" in its subtitle, and "You raised this" under What happened. Fix: use "You" for the owner in both places. Shot: C/shots/after/chromium/v-sheet-scoped.png (opened).

**Checked and fine:**
- **Keyboard round trips, both engines:** a keyboard-opened capture panel returns focus to New request on all 5 ways of closing it, and the request's page returns focus to its row.
- **Pointer opens in WebKit:** after a pointer-opened panel or sheet closes, WebKit returns focus to `body`, in both builds. That's how WebKit handles pointer focus, so it isn't counted.
- **Vision lens (Chromium only):** the human loop comes first, with the Needs you group first and Review. On the queue, the sheet and the capture panel at rest, amber is used only for the owner's waits (the header and the tab badge); validation text is the exception (N1). There's no emoji. The counts agree (header 1 = badge 1 = Needs you · 1).
- **Regressions:** none. Tasks, Bugs and Debt behave as before, and nothing scrolls sideways at 1280.
- **Not applicable:** there's no Inbox or Home in this branch, so the Inbox invariants and presence rules weren't applied.

## What's next

app-frontend-dev:
- **Must fix:** F1 at 390 and 320 with 200% text. The panel's content has to fit its width, with nothing cut off, and the X has to stay clear of the heading.
- **Should fix (S3):** the F7 overflow at 200% (the tab bar should wrap or scroll within itself, and the selects at 320 must fit the viewport); the Search ring and the radios' ring, plus the offsets (F5: −2 px inset on tabs, rows and fields; +2 px on free-standing buttons and links); and 44 px targets below 640 px (F7: the WebKit selects, New request, the group headers and the tabs).
- **Ideally:** F3 ("@Sam Okafor"), F9 (the pill's tone and icon), N1 and N2.

On my side, the next pass also confirms the F5 offsets after the fix, measures the WebKit capture-panel ring from a keyboard-opened panel (reachability with Option+Tab is already confirmed), and re-measures the Chromium caret case (item 3).

Then I recheck those, one browser at a time.
