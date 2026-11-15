# UX pass: requests-3 (app-frontend-dev/requests-3 @ 83ca9b13)

**Verdict: CHANGES NEEDED.** Routing by @name, completion, the People filter, the From link and similar requests work. Message instead is hidden, as agreed. One thing blocks it: at 200% text the capture panel can't be used, because it's taller than the window, cut off at the top and the bottom (Send is off-screen) and doesn't scroll (F1, S2). At 390 px and 100% text it overflows by about 63 px at the top and at the bottom in Chromium, but Send stays visible.

There are also six S3 findings, five S4 findings, and two RQS-15 parts not built (the office-stopped state and the project's Requests tile). The biggest S3 gap is that the queue offers only 2 of the 7 State options (F4).

- **Requested by:** app-frontend-dev (01M78KQ1). ada gave the go in 01M78M40: the v1.0 queue, one browser at a time.
- **Checked:** 12 Nov 2026, by Eden.
- **Branch:** unmerged, and far behind v1 (it still has the old sidebar shell). I judged the requests feature only, not the shell.
- **Against:**
  - `knowledge/specs/feature-requests-ux.md` rev 4: §1.1, §2, §3 and RQS-4, RQS-13, RQS-15 and RQS-16;
  - design-system §6 (D23), §17, §18 and §20.
- **Spec age:** the requests spec predates IA v2. Where I hold the branch to IA v2 rules (D23, design-system §6), I say so; those rows are lower severity.
- **Method:** one worktree with the mock data (`--mode e2e`), Playwright, WebKit first and then Chromium, one browser at a time with the 1-minute load at 40 or below. Results match in both engines unless noted.
  - Screenshots and measurements are in the session scratchpad, `rq3/shots/{webkit,chromium}/`, each with its own `results.json`. They aren't committed.
  - The developer's `e2e/requests.spec.ts` gives `29 passed (1.8m)` after `bun run build`.

## Items

| # | Item | Result | Measured | Shot |
|---|---|---|---|---|
| 1 | State filter, RQS-15 | Partly met | The default is Open. The control offers only **Open** and **All**, 2 of the 7 options in §2. "Nothing is open. Show all requests" switches to All and shows the 1 row. While you narrow by people, project or search, the collapsed Approved group opens (3 of 3). No match reads "Clear the filters"; the spec says "Clear filters". Loading is the text "Loading work…" with no skeleton rows. Can't load is page-wide, "{error} Try again", not "Can't load requests." with **Retry** (both from the code; can't load couldn't be triggered in the mock) | webkit/13, 09, 10 |
| 2 | @name routing, RQS-4 | Partly met | "@sam, fix the build time" is saved as "fix the build time", on the row and on the request's page. An unknown handle shows the exact sentence, and Send is off. Picking someone clears it. "@sam" alone asks "Write what you need after @sam." But a partial handle routes without a warning, and only one punctuation mark is stripped (see F2 and F3) | webkit/03–06, 08 |
| 3 | Completion, RQS-4 | Met | ↑↓ move and wrap, Enter and Tab pick, and Esc closes only the list. The list shows only while the caret is in the handle. Nothing is picked during IME composition. The field has aria-activedescendant, aria-controls and aria-autocomplete="list", and the list is `role="listbox"`. The options are named "Sam Okafor @sam". See F8 for accessibility beyond the spec | chromium/02 |
| 4 | People filter, RQS-15 | Partly met | It offers Anyone · Not routed · Raised by me · Scoped by Juno Park · Scoped by Sam Okafor. No "Raised by <member>" options appear, inference: the mock has no other raisers. "Not routed" belongs under State in §2, not People. Project lists only projects that have requests, where §2 says each project (RequestsPanel.svelte, the People and Project selects). Filters are kept after a reload, and each organization has its own (2 storage keys; Acme's filters survive a visit to Beta) | webkit/14 |
| 5 | From link, RQS-16 | Met | A task shows "From REQ-9 · Costs per project" as a link, which opens the request's page (Approved) | webkit/11, 12 |
| 6 | Similar requests, RQS-13 | Met | Shows 2 of the 3 unscoped look-alikes; the scoped one is left out. Each has **Add to REQ-n** | webkit/07 |
| 7 | Message instead | Met (hidden). RQS-4 requires it, but ada ruled on 12 Nov (01M78M40) that it's judged as hidden until a messaging capability exists | No button, no "Choose who to message." text and no hint element | webkit/05 |
| 8 | IA v2 placement, D23 (beyond the spec, which predates IA v2) | Partly met | Requests is a tab of Work (Tasks, Bugs, Debt, Requests). A Scoped row shows the text "Scoped by Juno Park" and a **Review** button, but no gray **Scoped** pill. Its only pill is the project's ("Atlas"). There's no amber Needs you on the row | webkit/08 (opened) |
| 9 | Office-stopped state, RQS-15 | Not met (not built) | The queue has no code for an office that's stopped: no readable last list with actions off and the reason. Capture while the office is unreachable (RQS-17) is declared not built yet | code |
| 10 | Project's Requests tile, RQS-15 | Not built (declared) | Nothing under the project routes. The developer listed it as not built yet | code |
| 11 | Merge into… and Decline… only in open states, RQS-16 | Met (from code) | Both buttons sit inside `open_` (OPEN_STATUSES, RequestSheet.svelte:116, 248). Merge is disabled with "No other open requests to merge into." when there's none | code |

## Findings

- **F1 · S2 · the capture panel is cut off and can't scroll.**
  - At 1280 px with 200% text, the panel is 1350 px tall in a 900 px window. Its top is at −225 px (the heading is clipped) and Send ends at 1077 px. Neither scrolling nor focusing Send brings it into view.
  - At 390 px with 200% text it's 2382 px tall with its top at −769, and it overflows sideways by 77 px. This is the worst case.
  - At 390 px and 100% text it's 970 px tall in 844 in Chromium (938 px in WebKit). In Chromium it runs from −63 to 907, so about 63 px are off-screen at each end, but Send stays visible (bottom 795). WebKit's position wasn't measured.
  - Shot: chromium/18-capture-1280-200pct.png (opened).
  - Fix outcome: the panel fits the window, with at least a 16 px inset. Its body scrolls, its footer (Send, Just save) stays visible, and nothing overflows sideways at 390 px or at 200%.
- **F2 · S3 · a partial handle routes without a warning.**
  - "@sa partial then words" was sent to the rule's suggestion (Juno Park), with "@sa" left in the saved text (REQ-4).
  - Fix outcome: a leading handle that matches nobody exactly gets the unknown-handle message, and Send stays off until someone is picked or the handle is removed.
- **F3 · S4 · only one punctuation mark is stripped (beyond the spec: §1.1 and RQS-4 only require the handle to be removed; stripping punctuation is the developer's own rule, and it should be complete).**
  - "@sam,, x" is saved as ", x" and "@sam — x" as "— x".
  - Fix outcome: all leading punctuation and dashes after the handle are stripped, along with the space.
  - Inference, not measured: "@Sam Okafor …" would probably be saved as "Okafor …". Please test that case.
- **F4 · S3 · the queue is missing State options and two states (RQS-15, §2).** This is the largest S3 gap.
  - The spec's State options are Open (the default) · Needs you · With agents · Not routed · Approved · Declined and merged · All.
  - Loading should be skeleton rows. Can't load should be "Can't load requests." with **Retry**, inside the list.
  - No match should read "Clear filters".
- **F5 · S3 · focus rings don't follow §17's outline style, and some fail 3:1 (measured in Chromium; in WebKit the measurement landed on a toast and a select, so it isn't counted).**
  - Below 3:1: "Show all requests" and "Clear the filters" use the browser's ring (2.74:1). Review has a 3 px ring at 50% opacity (2.74:1).
  - 3.19:1, passing contrast but not §17's outline: the From link, Search, and the capture panel's Cancel, Just save, Send and Close.
  - Fix outcome: the standard indigo-400 outline (§17), at 3:1 or more against what's around it.
- **F6 · S3 · contrast.** The "@sam" handle inside the selected option is gray-400 on white/15, 3.51:1. Use gray-300 or lighter (§18, 4.5:1 for text).
- **F7 · S3 · 390 px layout (§20).**
  - Titles break mid-word ("everywh/ere").
  - Controls are 20–36 px tall, not 44 px: row title buttons are 24 px, selects 31 px in Chromium and 20 px in WebKit, and Search 36 px.
  - The side gutters are about 32 px, not 16 (from the screenshot).
  - At 200% text the filters overflow sideways.
  - Shot: chromium/17-queue-390.png (opened).
  - Fix outcome: the row stacks below 640 px with the title at full width and breaking between words only; targets are 44 px below 640; gutters are 16 px.
- **F8 · S3 · completion semantics (accessibility beyond RQS-4).** The text field already has aria-autocomplete, aria-controls and aria-activedescendant, and the list is a listbox. It still needs `role="combobox"` and `aria-expanded`. The options also contain nested buttons, which a listbox option shouldn't have.
- **F9 · S4 · no gray Scoped pill (D23).** A Scoped row should keep its gray **Scoped** pill (§6), as on Work. Its Needs you belongs to the Inbox Approval.
- **F10 · S4 · unknown-handle message not linked to the field.** Add aria-describedby on the text field.
- **F11 · S4 · clicking the already-selected radio doesn't clear the unknown-handle message.**
- **F12 · S4 · Work forgets the Requests tab (beyond the spec).** After a reload, Work reopens on Tasks.

No emoji found.

**What's next:** app-frontend-dev fixes F1, F2 and F4 to F8, builds the office-stopped state and the Requests tile (or ada defers them), and fixes the Item 4 points and, ideally, F9 to F12. Then I recheck, one browser at a time.
