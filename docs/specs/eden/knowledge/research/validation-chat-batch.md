# Validation · chat bug batch (v1 be62fa2 vs its parent)

**Verdict:** BUG-40, 41, 42, 43 and 44 part 1 are fixed, and BUG-45 is 7 of 8 fixed (45d partly). Nothing outside Chat changed: all 100 non-chat main-screen pairs are identical, and the 281 chat pairs that differ trace to the fixes or to the clock. Six new issues (one S3), plus one pre-existing S4. Local checks, through heavy: check `COMPLETED 1158 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`, unit `Tests  1325 passed (1325)`, e2e `258 passed (6.9m)`.

**Each fix, checked by me in the code at be62fa2:**
- **BUG-40:** `ThreadPanel.svelte:54` is now `absolute … left-64 z-30 … xl:static`, an overlay below xl.
- **BUG-41:** `AskCard.svelte:55,94` guards on `!officeRunning`.
- **BUG-42:** `+page.svelte:412`, the message list is `role="log" aria-live="polite"`.
- **BUG-43:** focus moves on answer, Reject and Retry (`AskCard.svelte:55-70`, `+page.svelte:263-305`).
- **BUG-44 part 1:** the URL carries `c`, `thread` and `v` (`+page.svelte:137-170`).
- **BUG-45:** 7 of 8 fixed (ChannelDialog, Composer, AskCard chip, ChatSidebar name and contrast, Escape).
- **45d, partly fixed:** `parseSlash` matches only `/[a-z]+`, so "/Frobnicate" and "/deploy2" still post as text (`messaging/model.ts:314`).

**Second pass: the code and screenshots I opened for BUG-43 and BUG-45:**
- **43:** `+page.svelte:266-291` calls `restoreFocus(origin, place, target)` only when focus was lost by this update; `AskCard.svelte:68-69` restores focus to its origin after a delayed refusal.
- **45a, fixed:** the name input has `oninput={() => (error = undefined)}` plus `aria-invalid` and `aria-describedby` (`ChannelDialog.svelte:80`). The probe in `bugs-B.json` shows the refusal ("There is already a channel named #general.", invalid true), then after editing no alerts and invalid null. In `bugs-A.json` the alert stays after editing. Screenshot: `bugs-B/bug45a-edited.png`.
- **45b:** `Composer.svelte:107-110` checks `TextEncoder` byte length against `bodyMax` before sending, keeps the text and says "That message is too long. Shorten it or attach it as a file."
- **45c:** the thread renders `undeliveredRows(pending.filter(thread_root_id === root.id))` (`+page.svelte:516`), the same Not delivered and Retry rows as the main list.
- **45e:** the sample hand-off's `linked_ref` now carries the project key, rendered through `Rich` as a chip (`AskCard.svelte:122`, `sample.ts:62`).
- **45f:** `dm-status` is `text-gray-300` (`ChatSidebar.svelte:41`): 7.34:1 on the selected row by my calculation.
- **45g:** the row's `aria-label` is `${title}, ${PRESENCE_WORD}` (`ChatSidebar.svelte:34`), and the avatar is `aria-hidden` (`:39`).
- **45h:** Escape closes the thread (`+page.svelte:357-361`); N1 is the side effect.

**New in be62fa2, each checked by me:**
- **N1 · S3.** Escape anywhere closes the thread, even with text in its reply box or an ask's Reply box, and the text is lost. The window `keydown` handler calls `closeThread()` with no draft check (`+page.svelte:357-361`).
- **N2 · S4.** Back with Add channel open moves the conversation behind the open dialog. The URL effect calls `openLocation` without checking for an open dialog (`+page.svelte:123-131`).
- **N3 · S4 a11y.** The title is always "Chat · Agentic Org" (`+page.svelte:364`), so every move announces the same words and every history entry has the same name.
- **N4 · S4.** The DM row is 37.5 px, not 36: an inline `<span aria-hidden>` wraps the Avatar and adds line height (`ChatSidebar.svelte:39`).
- **N5 · S4 (WebKit).** Clicking in a message focuses the `<article tabindex="-1">`, so `focus-within` keeps the highlight and toolbar on (`MessageRow.svelte:68`).
- **N6 · S4.** Any text starting with a lowercase `/word` is refused ("/tmp is full" gives "No command /tmp.", even with a leading space, because of `trimStart()`). There's no way to send it as text (`model.ts:314-320`).
- **P1 · S4, pre-existing.** With the office stopped, react, pin and mute stay enabled, then fail (`+page.svelte:283-286`). §7 says read-only.

**Product rule (mine) for 45d and N6, sent with the bugs:**
- Command names match regardless of case.
- Any leading `/word` (letters, digits and hyphens) that isn't a command stays in the composer, with "No command /<word>." and a **Send as message** action that posts it as text. "Did you mean /<cmd>?" applies when the word is close to a command.
- A leading space never makes text a command.

The full working report follows.

---

# Validation: the chat bug batch (BUG-40 to BUG-45), merged as be62fa2

**Verdict: all six bugs are fixed, except BUG-45's unknown-command item, which is only partly fixed. Nothing outside Chat changed. be62fa2 brings 6 new problems, none above S3. The worst is that Escape in a thread's text box closes the thread and throws away the text.**

- Change: merge `be62fa2` ("Merge avery/chat-bugs: Chat layout, accessibility and navigation") on `v1`. I compared it with its first parent, `d9cda30` (`git log -1 --format=%P be62fa2` gives `d9cda30… c876ee9…`).
- Builds: A = `d9cda30` (before) and B = `be62fa2` (after), each in a throwaway worktree under the session scratchpad `chat-batch/`, running `bun run dev --mode e2e` (the test-owned fake composition; ordinary `bun run dev` has shown "unavailable" since F1-03) on ports 5241 and 5242.
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. The worktrees are removed and the dev servers are stopped.
- Requirements: `docs/specs/chat-ux.md` at be62fa2 (§1, §3, §7, CHAT-1/2/3/5/8/9/14/18) and Eden's `knowledge/design-system.md` (Layout, Sizes, Colour).
- I don't re-report anything already catalogued. That covers BUG-2, 6..12, 14..20, 23..27, 28..37, 46, O1 and N1, plus Tomas's be62fa2 spot-check, which ada has catalogued as **BUG-49** (focus hidden under the overlays) and **BUG-50** (the live region spans the whole message area). Where my probes touch those, I only say so.

## Per bug

| Bug | Result | Evidence (A → B) | Code (be62fa2) |
| --- | --- | --- | --- |
| **BUG-40** panels at 960×640 | **Fixed** | Thread at 960: the conversation was 32 px (544–576) with a 24 px composer, search painted over the thread, and both Send buttons were unclickable. Now the panel lies over the conversation (544–960, 416 px), and the page doesn't scroll sideways (`sectionX` 212 → 0). The conversation is 416 px once it closes. Members is the same. The steps that used to fail now pass at 960 in both schemes: `c08-thread-sent` (the reply shows in the thread and in the channel) and `c39-renamed` (the title reads #release-v3). From 1280 px up the panel is static (1280: conversation 352 px, panel 384 px; 1440: 512 + 384). | `ThreadPanel.svelte:54`, `MembersPanel.svelte:46` (`absolute … left-64 z-30 xl:static xl:w-96`) |
| **BUG-41** stopped office | **Fixed** (asks, approvals and hand-offs) | `?chat=offline`, Needs you: the cards had Approve, Reject…, Yes, block, No, allow and Reply…, and an answer failed with "The office didn't answer.". Now each card has 0 buttons and says "The office isn't running. Restart it from Offices." This holds in Needs you and in #atlas; the thread composer is gone, and `ThreadPanel` passes `officeRunning` to its rows too. Reactions are still live (P1 below, which predates the merge). | `AskCard.svelte:55` (guard) and `:94` (read-only branch), `+page.svelte:296`, `:308` (`answer`/`accept` refuse when stopped) |
| **BUG-42** new messages announced | **Fixed** | Before, there was no `role=log` and no live region apart from typing. Now the list is `role="log" aria-live="polite" aria-label="Messages"` and the thread is a "Thread messages" log. Inês's reply arrives inside the log (WebKit and Chromium). The log is too broad: it also holds search results and Needs you (`bug42-search-inside-log: true`). That is BUG-50, already filed. | `+page.svelte:412`, `ThreadPanel.svelte:59` |
| **BUG-43** focus after answer, Reject, Retry | **Fixed** | Focus used to land on `body` every time. Now, in WebKit and Chromium: after Reject by keyboard it moves to the next card's **Yes, block**; after the last answer to **Nothing needs you.** (`tabindex=-1`); after Send reply in #atlas to the answered card; after Retry, by keyboard or mouse, in the conversation or a thread, to the delivered message. A delayed refusal keeps focus on the button (`online-stopped-mid-answer`). Tomas's LOW 3 (a mouse Retry focuses the list) didn't reproduce: focus went to the delivered row. | `AskCard.svelte:55-70`, `+page.svelte:263-305` (`deliver`, `restoreFocus`, `answer`) |
| **BUG-44** place in the URL (part 1) | **Fixed**; part 2 **no worse** | Before, the URL stayed `/chat`, reload went to Needs you, and Back left Chat (`/agents`, then `about:blank`). Now the URL follows the place: `?c=c-general`, `?c=c-release-v1&thread=m-10`, `?v=mentions`, `?v=threads`, and `&m=` from search. Reload keeps the channel and thread. Back steps thread → channel → previous channel, and Forward returns. A deep link to a thread or a reply (`?m=m-11`) opens it, and mock flags (`chat=fail`) are kept. Part 2 (F2-08) is no worse: the DM draft and the Not delivered row are lost on reload in both builds. | `+page.svelte:165-175` (`writePlace` → `goto`), `:123-131` (address `$effect`), `:132-148` (`urlLocation`, `openLocation`) |
| **BUG-45a** stale "already exists" | **Fixed** | After "General" is refused, typing "Release V2" now clears the alert and `aria-invalid`. Before, the alert stayed, and a later "!!!" error showed both alerts. The refusal now sets `aria-invalid` and `aria-describedby`. | `ChannelDialog.svelte:80` |
| **BUG-45b** over-limit message | **Fixed** | 160,000 "é" (320,000 bytes) is now kept in the box (160,000 chars) with the alert "That message is too long. Shorten it or attach it as a file." and `aria-invalid`. Nothing is posted and there's no Retry. Before, the box was cleared and a Not delivered row offered a Retry that couldn't work. The same holds in the thread, where the old build kept the text but only toasted. Exactly 262,144 bytes sends. | `Composer.svelte:108` |
| **BUG-45c** failed thread reply | **Fixed** | Before, there was only a toast and the text stayed in the box. Now there's an inline "Not delivered: The office didn't answer. Retry · Dismiss" in the thread, and in the channel too when "Also send" is on. The row survives closing the thread and switching channels. Retry from either copy delivers once (thread 1, channel 1, rows 0). | `+page.svelte:258`, `:370`, `:516` |
| **BUG-45d** unknown /command | **Partly fixed** | `/frobnicate now` is refused with the toast "No command /frobnicate." and the text stays in the box. Before, it was posted. But `/Frobnicate now` and `/deploy2 now` are still posted as text. New side effect: N6. | `model.ts:312` (regex `^\/([a-z]+)…`), `:319` |
| **BUG-45e** Linked to T-12 | **Fixed** | "Linked to ac-T-12" now shows with an item chip. Before: "Linked to T-12", plain mono text, no chip. The key comes from changing the sample data (`sample.ts` `ac-T-12`). The chip regex also accepts a bare `T-12`. | `AskCard.svelte:122` |
| **BUG-45f** selected DM status contrast | **Fixed** | Selected row: 4.41:1 → **7.80:1**. Other rows: 5.95 → 10.51 (gray-400 → gray-300). | `ChatSidebar.svelte:41` |
| **BUG-45g** DM name repeats status | **Fixed** | Before: "SO Working Sam Okafor Working". Now: "Sam Okafor, Working" (with ", N unread" or ", N mentions" when there are any). | `ChatSidebar.svelte:34`, `:39` |
| **BUG-45h** Escape closes the thread | **Fixed** | Escape closes the thread and focus returns to the "↳ 3 replies" opener (WebKit and Chromium). The URL drops `thread=`. Escape with the @-popup open only closes the popup, and Escape in a message menu only closes the menu. New side effect: N1. | `+page.svelte:357-361` |

Screenshots: `validation-chat-batch-shots/bugs-A/` and `bugs-B/` (`bug40-960-thread.png`, `bug40-960-members.png`, `bug41-needs.png`, `bug42-thread.png`, `bug43-*.png`, `bug44-*.png`, `bug45*.png`), with numbers in `bugs-A.json`/`bugs-B.json` and `bugs2-*.json`. Validate pairs: `A|B/<run>/c06-thread-open.png`, `c35-members-panel.png`, `c81b-offline-answer-ask.png`, `c32/c33-add-channel-*.png`. Avery's archive `chat-bugs-be62fa2/` shows the same states (see Checks).

## New in be62fa2

### N1 · S3 · Escape in a thread's text box closes the thread and throws the text away
- Repro: #release-v1 › ↳ 3 replies, type "half-written thread reply" in Reply in thread, press Escape. Or: #atlas › Reply in thread on Kai's ask › Reply…, type an answer, press Escape.
- Actual: the thread closes. Reopened, the box is empty, and the typed ask answer is gone. Escape in the search box also closes the thread.
- Expected: Escape mustn't silently discard typed text. In the established pattern (work-items-ux.md §4, the BUG-6 fix), closing with text asks "Discard…? (Discard · Keep editing)". Simpler: ignore Escape when focus is in a non-empty text field, or keep the draft.
- In A, Escape did nothing and the text stayed. Closing with × also drops the draft in both builds (that predates the merge).
- Evidence: `bugs2-B.json` `esc-draft` `{closedByEsc:true, draftAfterReopen:""}`, `esc-ask-reply` `{threadClosed:true}`, against A `{closedByEsc:false, draftAfterReopen:"half-written thread reply"}`. Screenshot before Escape: `bugs-B/esc-draft-before.png`.
- Code: `+page.svelte:357-361` (the window handler checks only dialogs and `defaultPrevented`, not the focused field).

### N2 · S4 · Back while a dialog is open changes the conversation behind it
- Repro: in #release-v1 (reached by switching channels), open Add channel, type "half", press Back.
- Actual: the conversation behind the dialog switches to #atlas, and the dialog stays open with "half".
- Expected: Back closes the dialog first, or leaves the place alone while a modal is open.
- Evidence: `blitz-B/urls-back-with-dialog.png` and `blitz-B.json` `urls-back-with-dialog`.
- Code: `+page.svelte:123-131` (the `$effect` on `page.url.search` runs `openLocation` whatever dialog is open).

### N3 · S4 · A11y: every move in Chat announces "Chat · Agentic Org", and every history entry has that title
- Actual: each channel, thread or view change goes through `goto`, so SvelteKit's route announcer (`aria-live="assertive"`) reads the unchanged title "Chat · Agentic Org". It appears in the page text of 130 B states and 0 A states, and in blitz `kbd-*-live-after-reply` as `div[assertive]: "Chat · Agentic Org"`. The window title and the Back menu show the same name for all 10+ entries.
- Expected: the title names the place ("#release-v1 · Chat · Agentic Org"), which makes the announcement, the window title and history useful.
- Code: `+page.svelte:364` (static `<title>`), `:174` (`goto`).

### N4 · S4 · Visual: DM rows grew from 36 to 37.5 px
- The avatar is now wrapped in an inline `<span aria-hidden="true">`, so every DM row is 1.5 px taller and the DM list shifts. This causes most of the pixel differences: 6,008 px at the bottom of the sidebar in every 1440 chat state.
- Expected: 36 px rows on the 4 px grid (design system, Sizes).
- Fix: make the wrapper `contents`, `flex` or `block`.
- Code: `ChatSidebar.svelte:39`. Evidence: `A|B/default-dark/c01-needs-you.png` (Avery Stone at y 796 → 798, Inês Duarte at 872 → 876).

### N5 · S4 · WebKit: a mouse click inside a message focuses the whole message
- Messages and ask cards are now `tabindex="-1"`. In WebKit, which doesn't focus clicked buttons, a click on ↳ replies, Show more or Reply… moves focus to the `<article>` or the card.
- The row's highlight and hover toolbar then stay on after the pointer leaves (`B/default-dark/c09-thread-closed.png`).
- Validate records `focus: body → article`/`div` in c06, c09, c20, c23, c71 and c81.
- Code: `MessageRow.svelte:68`, `AskCard.svelte:74`.

### N6 · S4 · Ordinary text that starts with a /word can no longer be sent
- Repro: post "/tmp is full again", or " /tmp is full again" with a leading space.
- Actual: the toast "No command /tmp." appears and the text stays in the box. A sent it as text. The refusal doesn't say how to post it as a message. Meanwhile `/Frobnicate` and `/deploy2` (BUG-45d) still post.
- Expected: spec §3 lists the commands; an unknown one needs a way out, such as "No command /tmp. To send it as text, start with //" or a **Send as message** action.
- Code: `model.ts:312` (`text.trimStart()`, `[a-z]+`) and `:319`; the unit test changed in `model.test.ts:184`.

## Not new, not catalogued

### P1 · S4 · Office stopped: reactions (and the other message actions) still act, then fail
- Repro: `?chat=offline`, #general, then ✅ on "Ready for review".
- Actual: the toast "The office didn't answer." The toolbar's React Seen, React Looking, Reply in thread and More actions are all enabled, in both builds.
- Expected: §7, read-only from the last copy.
- Evidence: `bugs-B/bug41-react.png`, `bugs-*.json` `bug41-react`.
- Code: `+page.svelte:283-284` (`react` and `pin` call `act` whatever `officeRunning` is), and the `MessageRow` toolbar.

### Notes (observations, not filed)
- Below 1280 px the overlay covers the whole conversation: 416 px at 960 and 735 px at 1279. The design system's side panel is 440 px.
  - The Members toggle sits under its own panel, and Escape doesn't close Members. Only × closes it.
- An unknown `?c=`, `?thread=` or `?v=` falls back silently, and the bad key stays in the URL: `?c=c-nope` shows Needs you. These are Tomas's LOW 2.
  - Attacker-shaped values are inert: `?c="]<img onerror>` showed Needs you, with no error and no injection.
- The decorative "#" of the selected channel is 4.41:1 in both builds. It's `aria-hidden`.

## Pixel comparison (nothing else broke)

There are 125 states (94 chat states plus the 31 main screens) × 4 runs (1440×900 and 960×640, dark and light, WebKit), so **500 pairs: 219 byte-identical, and 259 identical after scroll normalisation.** All 25 non-chat main screens (01-19, 26-31) are byte-identical in all 4 runs (100 pairs). Every one of the 281 differing pairs is explained:

| Cause | Pairs | Where |
| --- | ---: | --- |
| DM rows: status gray-400 → gray-300 (BUG-45f), plus the 1.5 px taller rows (N4) | 159 | sidebar only, y 746-899 at 1440 (6,008 px; 4,438 px once a channel is added) |
| The same shift moving the sidebar's scroll at 960 (normalised pair identical) | 36 | sidebar only |
| The same, with dialog or sidebar scroll offsets that differ (c36 Add people dark at 1440; c12, c13, c42 and 25 at 960) | 7 | sidebar or dialog list only |
| "Linked to ac-T-12" chip (BUG-45e) | 30 | an 18 px band at x 708-835 in #release-v1 states and in search |
| Thread or Members overlay at 960 (BUG-40) | 26 | c06-c08, c35-c40, c44, c78, c81, c88, 22 |
| Focus returns to the message after the thread closes (BUG-45h, BUG-43), so its highlight and toolbar show | 8 | c09 and 23, both sizes |
| Add channel: refusal cleared, `aria-invalid` border (BUG-45a) | 8 | c32, c33 |
| Offline cards read-only (BUG-41) | 4 | c81b |
| Run timing ("Waiting 8 min" against "Waiting 9 min") | 3 | 20-chat |

- Behaviour per step, beyond the bug fixes: URLs gain `c=`, `thread=` and `v=`; focus moves from `body` to the article or card (N5); and the page text gains "Chat · Agentic Org" (N3).
- The console is the same in both builds: only the deliberate 404 on `31-not-found`. Network: documents and `package.json` only.
- At 960 the old build fails `c08-thread-sent` and `c39-renamed` (BUG-40). The new build passes every step.

## Blitz (both builds)

- **Owner job:** answer, thread reply, wake Inês, add #launch-plan with 2 people, post, search and open. 3.0 s, no errors. Focus after the answer: body (A) → Approve (B). Open from search gives `?c=c-project-atlas&thread=m-7&m=m-7`.
- **Rapid sends:**
  - 120 sends arrive in order and unique.
  - Ten Ctrl+Enter presses give 1 message, a double-click on Send gives 1, and the same text twice gives 2.
  - My first B run showed 0 of 120, because my own `bun run check` touched `.svelte-kit` and Vite reloaded the page (dev-B.log 20:07:10). On rerun it was 120 of 120 (`blitz-B-run1.json` kept).
- **Offline, then online:** with `?chat=offline` the cards have no buttons. Clearing the flag and waiting for the 30 s refresh brings the banner down and Approve and Reject… back. Stopping between the read and the answer gives "The office didn't answer." in the card, and focus stays on Yes, block.
- **Back, Forward and reload:**
  - 10 rapid channel switches leave 13 history entries, and Back walks them one by one (A: Back leaves Chat).
  - Reload keeps Mentions and Threads (`?v=`).
  - A reply link `?m=m-11` opens the thread with the reply highlighted. Close, then Back, reopens it.
  - Back with a dialog open: N2.
- **Deep and invalid links:**
  - `?c=c-release-v1&thread=m-10` opens the thread. A thread from another channel opens the channel only. `?thread=` alone, `?c=`, `?c=c-nope`, `?v=nope` and `?thread=m-nope` fall back.
  - A duplicate `c` takes the first. The injection-shaped value is inert. No console errors.
- **Resize:**
  - With the thread open, 1440 → 960 → 1440 → 1024: the panel switches between static and overlay, and there's no sideways scroll.
  - Escape at 1024 closes it, and focus goes to the opener's message.
  - The old `resize` section still stops in both builds, because the Members toggle is under the overlay (see Notes).
- **Invalid input:** the same in both builds, apart from the BUG-45 fixes.
- **a11y probe:** the same in both builds, apart from 960 thread and Members hscroll 212 → 0 and the DM status contrast (4.41 → 7.80).
- **resize-extra at 960 with Members open:** A: the conversation composer is 24 px wide, and search isn't covered. B: both are under the panel, as intended.

## Checks in be62fa2 (B worktree)

- `bun run check` (heavy): `COMPLETED 1158 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
- `bun run test` (heavy): `Test Files  50 passed (50)` / `Tests  1325 passed (1325)`
- `bun run test:e2e --workers=2` (heavy, `PW_PORT=4291`): **`258 passed (6.9m)`** (exit 0, none flaky or failed). The 21 `chat-bugs.spec.ts` tests are among them.
- **Avery's archive** `app-avery-processes/apps/org-ui/playwright-report/chat-bugs-be62fa2/` (49 shots) against my e2e run:
  - 10 are byte-identical and all 49 are the same size.
  - 37 differ in narrow bands (546 to 7,069 px, at most 126 rows), which fits the wall-clock message times. The e2e suite installs no clock, as in F2-01d.
  - 2 differ by 2 px of antialiasing: `chat-channel-members` and `chat-focus-option`.

## Method

- **Scripts:** in `validation-chat-batch-shots/`, copied from `validation-f2-01d-shots/` (the originals are untouched).
  - `validate.mjs`: the pixel and behaviour pairs. Ports changed.
  - `blitz.mjs`: adds section 10, "urls", for the new addresses, Back with a dialog open, reload per view, reply links and resizing.
  - `a11y.mjs` and `resize-extra.mjs`: ports changed.
  - New: `bugs.mjs` and `bugs2.mjs` (one probe per bug in both builds, Escape edge cases, offline then online) and `analyze.mjs` (which region of each differing pair changed, written to `diff-regions.json`).
- **Clock:** `validate.mjs` installs the clock at 2026-11-10T12:00Z, and each flow runs in a fresh WebKit context. Every step also has a `.norm.png` with its scrollers at the top. Some light `.norm` images (c74, c83 to c85) differ by about 101k px while their raw pairs differ only by the DM rows. That's a normalisation artifact.
