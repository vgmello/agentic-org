# Validation · Agents URL intents ?q= and ?add=1 (v1 aaa82b8f vs 2efce5d5)

Requested by ada (01M762S4CWYAFM46X76SN29GA3). Avery, avery/agents-url-intents @ 0abb457e, Tier A, Tomas READY.

**Verdict: met, with three new S3 gaps in the empty search state and the add flow.**
- What now works:
  - `?q=` filters the roster case-insensitively on name and title.
  - `?add=1` opens Add agent.
  - Back, Forward and refresh behave sensibly, and focus returns to the "Add agent" button.
  - Nothing flashes the full list while the roster loads.
  - All of this works at every width and text size, in WebKit and Chromium.
- The before build ignored both parameters.
- What falls short:
  - The no-results state has no way out and isn't announced (design-system §21).
  - An agent added while a search is active is hidden with no notice.

**Method:**
- Requirements:
  - design-system line 588 (search goes to `/agents?q=`);
  - Rowan's bug70-slice-order A-QADD (`?add=1`, only `/agents` opens the sheet);
  - ada's checklist I1 to I5.
- The top-bar controls (J-TOP) aren't built yet, so I tested by URL entry and in-app links.
- 200% text means the root font-size set to 200%.

## Requirements (evidence I opened)

| Req | Before (2efce5d5) | After (aaa82b8f) |
| --- | --- | --- |
| I1 URL entry | `?q=noor` shows all 8 rows, with no search box. `?add=1` does nothing | `q=noor`, `NoOr` → Noor. `DeSiGnEr` → Juno and Pax. `WEB%20ENGINEER` → Noor. The box shows q. `add=1` opens "Add an agent" (aria-modal) with focus in Name, and `add` is removed from the URL. `add=0` and `add=yes` don't open. `q` and `add` together filter and open (WebKit `after/summary.txt`, Chromium `after-clean/I1-*`) |
| I2 Back and refresh | `add=1` stays in the URL, ignored | Refresh after closing: the sheet stays closed. Typing replaces the history entry, so the length stays the same and refresh keeps q. Back leaves to the previous page, and Forward returns with q and no sheet. Going from `/agents?q=noor` to `?add=1`, closing, then Back restores q (WebKit `after/summary2.txt`) |
| I3 focus | Opened by button: back to Add agent | Esc, Close, the backdrop and a successful add all return focus to "Add agent", whether opened by button or by URL. Exception: in WebKit, a mouse-click opener leaves focus on body, which is pre-existing (F4) |
| I4 states | Loading with a slow org: 0 rows, then all 8 | 0 rows, then 1 match, never the full list (Chromium `loading/q-x-slow-org`). Empty q, invalid add values, `<script>…`, emoji, a broken percent-escape and 2000 to 5000 characters all give 0 console errors; the box truncates to 200. The empty state is only partly met (F1, F2) |
| I5 widths and text | — | 320, 640, 960 and 1280 px × 100% and 200% × WebKit and Chromium × the search, empty and add states: 48 combinations, none with sideways scroll on the document. The sheet fits and scrolls. At 200% text, `main` scrolls sideways (F5) |

**Tests:** agents-roster.spec `--repeat-each=3` gave "105 passed (3.3m)". It runs in Chromium only, because the spec isn't in the WebKit project.

## Findings

1. **S3, UX and a11y, new: the empty search has no way out and isn't announced.** On `/agents?q=zzzz` the page shows only "No agents match your search."
   - There is no clear-search action.
   - The message leaves out the query.
   - It is in no live region.
   - Esc clears the field in Chromium but not in WebKit.
   - Expected (design-system §21): `No agents match "{query}".` plus a polite live region with the count, and a "Clear search" button (I4).
   - Evidence: `layout/catalog/agents-empty-search-desktop.png`, `layout/a11y-after.json` (`noMatchLive: not in live region`).
2. **S3, UX, new: an agent added while a search is active is hidden.** On `/agents?q=noor`, adding "Cameron Díaz" shows the "Added" toast, but the roster still shows only Noor.
   - Expected: clear the search after an add, or say "Cameron Díaz is hidden by your search · Clear".
   - Evidence: `webkit/after/shots/I2-add-while-filtered--added-hidden-by-filter_.png`.
3. **S4, IA, new: the tab counts ignore the search.** The tabs read "All 8 · Active 8" over 0 or 1 visible row. Expected: counts within the search, or a result count next to the search.
4. **S3, focus, pre-existing: in WebKit (the Tauri engine), a mouse-click opener leaves focus on body after the sheet closes.** A WebKit mouse click doesn't focus the button, so there's nothing to return to; the next Tab starts at Chat. Expected (I3): focus on "Add agent". Fix: focus the trigger explicitly on close.
5. **S3, layout, pre-existing: at 200% text and widths 320 to 960, the page's `main` scrolls sideways.** The intro, the Sample data note and the Paused/Former tabs are cut off on the right. Already covered by BUG-70 AG-1 and SH-18.
6. **S4, new: matching is accent- and normalization-sensitive.** `q=ines` and NFD `Ine%CC%82s` don't find "Inês Duarte". Expected: normalise (NFC) and match without accents.
7. **S4, observation:** a 2000-character q is cut to 200 in the box but kept in full in the URL.

## Catalog

Added at aaa82b8, validated: Agents · search, Agents · no search results (with F1 noted) and Add agent.
