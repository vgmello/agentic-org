# Validation · chat follow-up (v1 6848ba1 vs 2b1f39b)

**Verdict:** all four items are fixed, and all 25 slash examples follow the rule. One new S4 (F1) and two pre-existing S4s. All 100 non-chat pairs are identical; every chat difference is explained by the fixes or by timing. Local checks, through heavy: check `COMPLETED 1160 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`, unit `Tests  1346 passed (1346)`, e2e `297 passed (9.1m)`.

**Each item, checked by me in the code and the probe output:**
- **BUG-56, fixed.** The window Escape handler returns when focus is in a text field with text (`+page.svelte:380-382`), so the reply, ask Reply and Reject texts survive.
- **BUG-45d, fixed.** `parseSlash` uses `/^\/([a-z0-9-]+)(?:\s+…)?$/i`, with no `trimStart` (`model.ts:311-320`), and the composer shows "Send as message" (`Composer.svelte:221`). In `fu-B.json`:
  - "/Task", "/TASK" and "/BuG" open their dialogs.
  - "/frobnicate", "/Frobnicate", "/deploy2", "/tmp", "/tmp is full", "/ship-now", "/2 …" and "/- …" get "No command /<word>. Send as message", and near misses add "Did you mean /bug?".
  - "/etc/hosts …", "/tmp,", "/tmp.", "/", " /task …" and "\t/bug …" post as text.
- **BUG-57, fixed:**
  - `beforeNavigate` cancels a popstate and dismisses an open dialog first (`+page.svelte:344-350`).
  - The title is `{viewTitle} · Chat · Agentic Org` (`:96`, `:388`).
  - The avatar wrapper is now `span.flex`, so the row is 36 px (`ChatSidebar.svelte:39`).
  - The article no longer has `tabindex` (`MessageRow.svelte:68`).
- **BUG-58, fixed.** React, pin and mute are `disabled={!officeRunning}` (`MessageRow.svelte:129`, `MembersPanel.svelte:65`). S4 note: the reason shows only in the banner and the composer note, not on the controls. That's acceptable, since the banner says why.

**New and pre-existing, checked by me:**
- **F1 · S4 · new.** History entries carry the next place's title. `select()` sets `view` (`+page.svelte:153`) before `goto` pushes the entry (`:175`), so `fu-B.json` shows `/chat | Mentions…` and `/chat?v=mentions | Threads…`. Fix: update the title after the push, or with `replaceState` on the new entry.
- **F2 · S4 · pre-existing.** Escape with focus on Send or "Also send" closes the thread and loses the reply (`:380-382`, a non-text target). The draft rule I gave Avery (keep the draft per thread while the page is loaded) covers it.
- **P1 · S4 · pre-existing.** With the office stopped, channel management stays enabled and fails: Remove, Add people…, Rename… and Archive have no `disabled` (`MembersPanel.svelte:70,77-79`), nor does Add channel (`ChatSidebar.svelte:82,90`). §7 says read-only.

The full working report follows.

---

# Validation: the chat follow-up (BUG-56, BUG-45d, BUG-57, BUG-58), merged as 6848ba1

**Verdict: all four items are fixed. Every example of the slash rule behaves as specified in 6848ba1. 6848ba1 brings 1 new problem (S4): history entries carry the title of the *next* place, so the Back menu is off by one. Two older problems, not catalogued, remain: Escape from the thread's Send button or "Also send" box still discards the reply, and a stopped office still offers channel management that then fails.**

- Change: merge `6848ba1` ("Merge avery/chat-validation-followup: Chat input, history and read-only actions") on `v1`. I compared it with its first parent, `2b1f39b` (`git log -1 --format=%P 6848ba1` gives `2b1f39b… a68b105…`).
- Builds: A = `2b1f39b` (before) and B = `6848ba1` (after), each in a throwaway worktree under the session scratchpad `chat-followup/`, running `bun run dev --mode e2e` (the test-owned fake composition, as the README at 6848ba1 describes) on ports 5251 and 5252. A third worktree at 6848ba1 (`C`) ran check, unit and e2e, so their `.svelte-kit` writes couldn't reload the dev server under my probes.
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. The worktrees are removed and the dev servers are stopped.
- Requirements: ada's brief for this follow-up (the slash rule as stated there), `docs/specs/chat-ux.md` at 6848ba1 (§3, §7) and `validation-chat-batch.md` (N1 to N6, P1), which BUG-56/57/58 catalogued.
- I don't re-report anything already catalogued (BUG-2, 6..12, 14..20, 23..27, 28..37, 40..46, 49, 50, 52..55).

## Per item

| Item | Result | Evidence (A → B) | Code (6848ba1) |
| --- | --- | --- | --- |
| **BUG-56** Escape with text | **Fixed** | WebKit and Chromium, `fu-*.json` `esc-*`. Thread reply "half-written thread reply" + Escape: A closed the thread and lost the text; B keeps both (`threadClosed:false, textKept:"half-written thread reply"`). The same for an ask's Reply… box and an approval's Reject reason inside a thread (A closed, B kept). Whitespace-only text also stays. An empty box still closes the thread, and 10 Escapes in a row keep the text (blitz). The @-popup, the search box and Escape in Needs you behave as before. Shots: `fu-A|B/esc-draft-before.png`, `esc-draft-after.png`, `esc-ask-reply-after.png`. Residual: F2 below. | `+page.svelte:378-386` (`textField && field.value !== ''` returns before `closeThread`) |
| **BUG-45d** slash rule | **Fixed**: all 25 examples as specified | See the slash table below. `fu-*.json` `slash-*`; shots `fu-A|B/slash-task-upper.png`, `slash-Frobnicate.png`, `slash-bgu-near.png`, `slash-tmp-words.png`, `slash-etc-hosts.png`, `slash-bare-slash.png`, `fu-B/slash-sendas-before|after.png`, `slash-thread-refused.png` | `model.ts:311-320` (`/^\/([a-z0-9-]+)(?:\s+…)?$/i`, `unknown: true`), `Composer.svelte:114`, `:221` (Send as message), `+page.svelte:251` (`asMessage` skips parsing) |
| **BUG-57a** Back with a dialog | **Fixed** | Back with Add channel, New message, a command form (/bug) and Rename open: A moved the conversation (#release-v1 → #atlas, → #general, → Needs you) and kept the dialog; B closes the dialog and stays on `?c=c-release-v1`. The same over an open thread (`&thread=m-10` kept). Afterwards Back and Forward still walk atlas → general → atlas → release-v1, and Chromium's history list is unchanged (5 entries, index 4, before and after). Forward with a dialog open also just closes it. Two Backs 150 ms apart close the dialog and step back once (WebKit and Chromium). A busy submit still completes. Shots: `fu-A|B/back-dialog-before.png`, `back-dialog-after.png`. | `+page.svelte:344-350` (`beforeNavigate` cancels a `popstate` and dismisses) |
| **BUG-57b** title per place | **Fixed**, with F1 | `document.title`, WebKit and Chromium: "Needs you · Chat · Agentic Org", "Mentions · …", "Threads · …", "#general · …", "#release-v1 · …", "Sam Okafor · …", "Sam Okafor, Noor Haddad · …", "#design-crit · …" (as owner), "Search · …" while searching, and a renamed channel follows ("#release-v9 · …"). Deep links and reload set it at once, and `?c=c-nope` gives "Needs you · …". A: always "Chat · Agentic Org". Each move is announced: `#svelte-announcer` reads the new title (A: "Chat · Agentic Org" every time). Search isn't announced (it isn't a navigation), and opening or closing a thread leaves the announcer on the channel's title. Back restores each title and announces it. **But the history entries are off by one: F1.** | `+page.svelte:96` (`viewTitle`), `:388` (`<title>`) |
| **BUG-57c** DM row 36 px | **Fixed** | DM rows: A 37.5 px (1:1 DMs; the group row was 36), B 36 px for all, so the DM list moves up 1.5 px per row (`fu-*.json` `dmrow`; `fu-A|B/dmrow-sidebar.png`). | `ChatSidebar.svelte:39` (`<span class="flex">`) |
| **BUG-57d** WebKit click focus | **Fixed** | WebKit, mouse click on ↳ replies, Show more, message text, then × and Reply… → Cancel: A focused the `<article>` or the ask card (`tabindex=-1`), and the row kept its highlight and toolbar (`opacity 1`) after the pointer left. B: focus goes to the message list, and the row has no highlight and opacity 0. Keyboard focus return still lands on the message (a temporary `tabindex` that is removed on blur: `rowTabindex:null` afterwards). Shots: `fu-A|B/click-after-close-x.png`, `click-text-away.png`. Side effect in the Notes. | `MessageRow.svelte:68`, `AskCard.svelte:74` (no `tabindex`), `+page.svelte:291-305` (temporary `tabindex` in `restoreFocus`) |
| **BUG-58** stopped office | **Fixed** (react, pin, mute) | `?chat=offline`, #general: React Seen and React Looking in the toolbar, the reaction chips, Pin, Mute and Daily digest in the menu, and every Members mute select are disabled in B (all enabled in A). A forced click on a disabled reaction does nothing (A: "The office didn't answer."). The same in the thread. A menu that was open when the office stopped turns its items disabled in place. When the office comes back, they re-enable. The reason is the banner "Read-only from the last copy. The office isn't running." and the composer's "The office isn't running. Restart it from Offices."; the controls themselves carry no reason (see Notes). Shots: `fu-A|B/offline-toolbar.png`, `offline-menu.png`, `offline-members.png`. Out of BUG-58's scope but still enabled: P1. | `MessageRow.svelte:105`, `:129`, `:145-150`; `MembersPanel.svelte:65`; `+page.svelte:284-286` |

### The slash rule, every example (channel composer, Ctrl+Enter, WebKit)

| Text | A (2b1f39b) | B (6848ba1) | Rule |
| --- | --- | --- | --- |
| `/task @kai write the docs` | Task form | Task form ("Assign a task to Kai Lindqvist") | ✓ |
| `/Task @kai …`, `/TASK @kai …` | posted as text | Task form | ✓ any case |
| `/BuG the export button`, `/Wake @ines` | posted | Report a bug, Run Inês Duarte now | ✓ |
| `/frobnicate now` | kept, toast only | kept, "No command /frobnicate. **Send as message**", `aria-invalid` | ✓ |
| `/Frobnicate now`, `/deploy2 now`, `/ship-now please` | posted | kept, "No command /<word>." + Send as message | ✓ letters, digits, hyphens |
| `/tmp is full again`, `/tmp`, `/tmp⏎second line` | kept, toast only | kept, "No command /tmp." + Send as message | ✓ |
| `/bgu the list`, `/BGU the list` | kept / posted | kept, "No command /bgu. Did you mean /bug?" + Send as message | ✓ near miss |
| `/etc/hosts has the wrong entry` | posted | posted | ✓ path |
| `/tmp,`, `/tmp.`, `/tmp, again` | posted | posted | ✓ punctuation |
| `/` | posted | posted (the command popup shows while typing) | ✓ bare slash |
| ` /tmp is full again` (leading space) | kept, toast | posted as "/tmp is full again" | ✓ |
| ` /task @kai …`, `⇥/bug tabbed` | Task form, Bug form | posted as text | ✓ leading whitespace |
| `/état cassé` | posted | posted | see Notes |
| `/- dash`, `/2 people agreed` | posted | kept, "No command /-." / "/2." + Send as message | per the rule's letter |

- **Send as message** posts the literal text once, clears the box and puts focus back in the composer, by mouse and by keyboard. A double-click posts one copy. In the thread it posts there, too.
- Editing the text clears the refusal and removes the action. Ctrl+Enter 20 times on `/nope repeated` gives one alert and posts nothing. The same refusal appears with Enter sends on.
- With `?chat=fail`, Send as message gives "Not delivered", and Retry delivers it once as text without re-parsing it (no form, no refusal).
- A 100,000-letter command word is refused in 0.7 s.
- A refusal doesn't survive a channel switch: the draft comes back, but the alert doesn't. That's harmless, because Ctrl+Enter refuses again.

## New in 6848ba1

### F1 · S4 · History entries carry the next place's title, so the Back menu is off by one
- Repro (Chromium; read with CDP `Page.getNavigationHistory`, which is what the long-press Back menu lists): open Chat, then Mentions, Threads, #general, #release-v1, Sam Okafor, the group DM, #design-crit, and Needs you.
- Actual: every entry has the title of the place opened *after* it: `/chat | Mentions · Chat`, `/chat?v=mentions | Threads · Chat`, `/chat?v=threads | #general · Chat`, `/chat?c=c-general | #release-v1 · Chat`, … `/chat?c=c-design-crit | Needs you · Chat`. Only the current entry is right. In the Back test: `/chat | #general`, `/chat?c=c-general | #atlas`.
- Expected: "history entries carry it", so each entry names its own place.
- `document.title` is right on every place and after every Back, and the announcer reads the right title. The wrong part is the history list, until you visit an entry again, which rewrites it.
- Cause: `select()` sets `view` first, so `<title>` updates at once. Then `writePlace()` calls `goto`, and SvelteKit only pushes the new entry after that. The title change lands on the entry being left. The fix is to change the title after the navigation is pushed, for example by deriving it from the place in the URL, or by setting `view` from the address `$effect`.
- Evidence: `fu-B.json` `titles-chromium-history`, `back-chromium-history`. A is `Chat · Agentic Org` everywhere, so it isn't comparable.
- Code: `+page.svelte:151-162` (`select`: `view = next` at :153, `writePlace()` at :162), `:175` (`goto`), `:388` (`<title>`).

## Not new, not catalogued

### F2 · S4 · Escape from the thread's Send button or "Also send" box still closes the thread and discards the typed reply
- Repro: in #release-v1 › ↳ 3 replies, type "typed then tabbed away", Tab to **Send** (Chromium; Option+Tab in WebKit) or to **Also send to #release-v1**, then press Escape.
- Actual: the thread closes. Reopened, the box is empty. This happens in WebKit and Chromium, in A and B alike.
- Expected (BUG-56's intent): Escape doesn't throw away typed text. Either keep the draft when the thread closes, or don't close while the thread's composer has text.
- BUG-56's fix only checks the focused element.
- Evidence: `fu-*.json` `esc-*-focus-on-send` and `esc-*-focus-on-checkbox`: `{threadClosed:true, draftAfterReopen:""}`.
- Code: `+page.svelte:380-382` (checks `event.target` only); the thread `Composer` drafts live in the component, so closing unmounts them (`Composer.svelte`, `drafts`).

### P1 · S4 · A stopped office still offers channel management and Add channel, which then fail
- Repro: `?chat=offline` › #release-v1 › Members.
- Actual: Remove, Add people…, Rename… and Archive are enabled. Remove gives "The office didn't answer." and changes nothing. Add channel (sidebar) opens, and its submit fails inside the dialog with "The office didn't answer.". The same in A.
- Expected: §7, "read-only from the last copy", as BUG-58 now does for react, pin and mute.
- Turn into a bug…, request… and task… also stay enabled offline. They only draft sample data today ("Sample data: nothing was saved"), so they need a product call when they become real writes.
- Evidence: `fu-*.json` `offline-members-editable`, `offline-add-channel`, `offline-turn-into-bug`; `fu-B/offline-members-release.png`.
- Code: `MembersPanel.svelte:70`, `:77-79` (no `officeRunning` guard), `+page.svelte:543` (`onremove`/`onarchive` call `act`), `ChatSidebar.svelte:82`, `:90`.

## Notes (observations, not filed)
- **The reason for disabled controls:** the toolbar's React buttons keep their titles ("Seen") and have no `aria-describedby`. The reason is only in the banner above the messages and in the composer's note. Optional: `title="The office isn't running"` on the disabled controls.
- **WebKit, mouse-opened thread:**
  - WebKit doesn't focus a clicked button, so the thread's "opener" is now the message list. Closing the thread (× or Escape) after opening it with the mouse returns focus to the list. Before, it returned to the opener's message (the article).
  - Opened with the keyboard, focus returns to ↳ 3 replies in both browsers.
  - Mouse users don't depend on this. I don't count it as a regression.
- **The slash rule's letter:** `/état` is plain text because `[a-z]` doesn't cover accented letters. Meanwhile `/2 people agreed` and `/- dash` are refused, as the rule's "letters, digits and hyphens" says. Both seem fine; `/2 people agreed` has its way out through Send as message.
- **Synthetic double Back:** `history.back(); history.back()` in one task with a dialog open lands two places back in Chromium. In WebKit it stays put. Two Backs 150 ms apart behave correctly in both browsers.
- **A draft across a stop:** typed text disappears with the composer when the office stops, and it comes back when the office restarts. That's the same in A and B.

## Pixel comparison (nothing else broke)

There are 125 states (94 chat states plus the 31 main screens) × 4 runs (1440×900 and 960×640, dark and light, WebKit, clock at 2026-11-10T12:00Z), so **500 pairs: 241 byte-identical and 259 differing, every one of them explained.** No step failed in either build. All 25 non-chat main screens (01-19, 26-31) are byte-identical in all 4 runs (100 pairs).

| Cause | Pairs | Where |
| --- | ---: | --- |
| DM rows 37.5 → 36 px (BUG-57c) | 241 | Sidebar only. At 1440 it's 5,926 px at x 308-522, y 746-899 (the DM list moves up 1.5 px a row). In DM and as-owner states the sidebar's own scroll differs by 6-7 px (`nav:34 → 28`), so the pairs are bigger (up to 25k px; 6-8k after normalisation). At 960, 52 of the 56 pairs are identical once the scroll is normalised. |
| Thread closed with × (c09, 23): the row no longer keeps focus, highlight and toolbar (BUG-57d) | 8 | One message row: 79,859 px at 1440 (x 556-1427, y 238-341), 36,179 px at 960. In A, focus was `article`; in B it's the message list. |
| Office stopped (c80, c81): the reaction chip is disabled at 50 % opacity (BUG-58) | 8 | The "✅ 1" chip: 868 px; 1,457 px in the 960 thread. Disabled controls go from 0 to 9. |
| Add people dialog, dark 1440 (c36): the list's scroll offset after checking Kai (84 → 168 px) | 1 | The dialog list. Light is identical apart from the DM rows. Run variance, also seen in the be62fa2 run. |
| Run timing, 960 light 20-chat ("Waiting 9 min" against "8 min") | 1 | 751 px of text |

- **Behaviour per step:**
  - Every chat step's title changes from "Chat · Agentic Org" to its place title (392 differences). The page text differs only by the announcer's title, apart from the one timing pair.
  - Focus moves from `article`, an ask card or `body` to the message list in c06, c09, c20, c23, c68, c71, c81, c85, c88, 22 and 23 (44 differences, all WebKit pointer clicks). The list doesn't match `:focus-visible`, so no ring shows.
  - Offline, 9 controls are disabled.
- **Overflow:** the same in both builds (only the 960 org chart, which predates this and isn't in Chat).
- **Console:** the same in both builds, only the deliberate 404 on `31-not-found`.
- **a11y probe** (`a11y-A|B.json`, every chat state and dialog at 960 and 1440): identical apart from the announcer's text, which now names the place, and the disabled offline reactions (`button "React Seen" [disabled]`). Contrast, unnamed controls, landmarks and scroll are unchanged.

## Blitz (both builds, `blitzfu-A|B.json`)

- **Back:**
  - From Chat's first entry with a dialog open, Back closes the dialog, and the next Back returns to /agents. A leaves Chat with the dialog still open.
  - With a submit in flight, the channel is still created.
  - Escape and then Back navigate normally.
  - Back with a message menu open closes the menu and goes back.
  - Two Backs 150 ms apart close the dialog and step back once (`extra-B.json`).
- **Send as message:**
  - A double-click posts 1 copy (A: no action at all).
  - 20 Ctrl+Enters give 1 alert and 0 posts.
  - With `?chat=fail`, Retry delivers it once as text.
  - A 100,000-letter word is refused in 0.7 s.
  - No console errors.
- **Escape:**
  - A burst of 10 keeps the text and the thread in both browsers.
  - Clearing the text, then Escape, closes the thread.
  - At 1024 (overlay), the text is kept.
  - Opened by keyboard, focus returns to ↳ 3 replies.
- **Titles:**
  - After 15 rapid switches, the title, h2 and announcer agree ("#release-v1"), and reload keeps it.
  - Renaming to #release-v9 updates the title.
  - The group DM is "Sam Okafor, Noor Haddad · Chat · Agentic Org".
- **Office stops while a menu is open:** Pin, Mute and Daily digest turn disabled in place, and choosing Pin does nothing. A: "The office didn't answer.".

## Checks in 6848ba1 (C worktree, through `~/.agency/bin/heavy`)

- `bun run check`: `COMPLETED 1160 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
- `bun run test`: `Test Files  50 passed (50)` / `Tests  1346 passed (1346)`
- `PW_PORT=4301 bun run test:e2e --workers=2`: **`297 passed (9.1m)`** (exit 0, none flaky or failed). The 24 `chat-validation.spec.ts` tests are among them.
- **Avery's evidence:** `chat-validation-f00fa8c/` (60 result folders, 20 of them `chat-validation`) and `chat-validation-a68b105-fix/` (14: 7 Chromium and 7 WebKit), 161 screenshots in all. They cover the same items: Back with Add channel, titles, 36 px rows, pointer focus, offline react/pin/mute, ask reply and reject in a thread, and Retry focus. My probes reproduce each of those outcomes independently in WebKit and Chromium.

## Method

- **Scripts:** in `validation-chat-followup-shots/`, copied from `validation-chat-batch-shots/` (the originals are untouched).
  - `validate.mjs`, `a11y.mjs` and `analyze.mjs`: run with the ports and the Playwright path changed.
  - `blitz.mjs`, `bugs.mjs`, `bugs2.mjs` and `resize-extra.mjs` were copied and their ports changed, but not rerun; `fu.mjs` and `blitz-fu.mjs` cover their probes for this change.
  - New: `fu.mjs` (one probe per item and every slash example, A and B, WebKit plus Chromium where focus differs), `blitz-fu.mjs` (a blitz of what's new: Back bursts and dialogs, Send as message under failure, huge words, Escape bursts, rapid titles, offline races) and `extra.mjs` (double Back 150 ms apart, a draft across a stop).
- **Rerunning:** the scripts import Playwright from the removed `chat-followup/B` worktree, so recreate it (or change the import) before running them again.
- **Load:** the machine ran at a load of 30 to 36 throughout. My first `fu.mjs` run was stopped and rerun after a locator in my own script waited 30 s per step; nothing from it is used.
