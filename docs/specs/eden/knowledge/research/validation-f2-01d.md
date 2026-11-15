# Validation · F2-01d messaging move (v1 4216b73 vs fb567d5)

**Verdict:** a pure move with no behaviour change and nothing new. D-F14a is the documented temporary exception: the chat route imports the module-private `legacy-api.ts` until F1-02's factory cutover (ada's STATUS.md).
- **Pixel compare:** 493 of 500 pairs are byte-identical (125 states × 4 runs). The other 7 are scroll, clock and antialiasing noise.
- **Avery's 30 shots:** 12 match byte for byte, 15 differ only in wall-clock times, and 3 differ by 1 to 2 px.
- **Local checks:** check `COMPLETED 1170 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`, unit `Tests  1087 passed (1087)`, e2e `229 passed (7.3m)`.

**Pre-existing issues, all checked by me, sent to ada 10 Nov:**
1. S2 layout. At 960×640, opening a thread or Members squeezes the conversation to about 32 px; search paints over the panel and the Send buttons overlap. Seen in `B/960x640-dark/c06-thread-open.png`.
2. S3 requirement. When the office isn't running, the banner says "Read-only from the last copy", but Approve, Reject and the ask answers stay enabled, then fail with "The office didn't answer." Spec §7: "Office not running: read-only from the last copy". Seen in `A/960x640-light/c81b-offline-answer-ask.png`.
3. S3 a11y. New messages aren't announced. The only `aria-live` in chat is the typing line (`chat/+page.svelte:443`); there's no `role="log"`.
4. S3 a11y. Focus drops after answering an ask, Reject or Retry: the buttons are `disabled={busy}` (`AskCard.svelte:92-105`), and the card is replaced.
5. S3 history. The chat place isn't in the URL; the route reads only `?surface=` (`+page.svelte:73`). Back, Forward and reload lose the channel, the draft and "Not delivered" rows.
6. S4, all confirmed:
   - **Add channel keeps a stale refusal:** the error is reset only on open or submit (`ChannelDialog.svelte:36-63`).
   - **An over-limit message:** `maxlength` counts characters, but `CHAT_LIMITS.bodyMax` is 256 × 1024 bytes. An oversized message is cleared and offered a Retry that can't work.
   - **A failed thread reply** gets only a toast; `ThreadPanel` has no Not delivered row.
   - **An unknown /command** is posted as text; there's no unknown-command handling.
   - **"Linked to T-12"** shows a plain `linked_ref`, with no project key and no chip (CHAT-14).
   - **The selected DM row's status:** `text-gray-400` on `bg-white/10` is about 4.16:1 by my calculation (the run measured 4.41:1); both are below 4.5:1.
   - **The DM row's name repeats the status:** the Avatar's initials and its `sr-only` presence word are both part of the name.
   - **Escape doesn't close the thread:** `ThreadPanel` has no key handler.

**Requirements:** met: CHAT-1, 2, 3, 8, 9, 10, 12, 13, 14, 17, 18. Partly met: CHAT-4, 5, 6, 7, 11, 15. CHAT-16 can't be tested in the UI. CHAT-19 to 23 aren't in this build.

Screenshots: `workspace/validation-f2-01d-shots/` (not committed). The full working report follows.

---

# Validation: F2-01d, the messaging (chat) module move and D-F14a

**Verdict: no behaviour change found. 4216b73 introduces no problems. Every finding below already exists in fb567d5.**

- Change: merge `4216b73` ("Merge avery/f2-01-messaging: domain module and state coverage") on `v1`.
- Compared with its first parent, `fb567d5`. `git log -1 --format=%P 4216b73` gives `fb567d5… 216e62b…`. The apps/org-ui trees: fb567d5 = ad305cd = `20a412c`; 4216b73 = 216e62b = Avery's 5646b2c = `16532a5`.
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. The throwaway worktrees are removed and the dev servers are stopped.

## What D-F14a is

D-F14a isn't in any commit message of `fb567d5..216e62b`. It's in the code and in ada's tracking:

- **D-F14** (ada, 2026-11-10 16:00, `STATUS.md` line 99): a module move keeps `index.ts` pure (named exports, nothing constructed at import). Existing import-time construction (here `chatApi = createMockChat(orgApi)` and `sender`) moves unchanged into a module-private `legacy-api.ts`. It isn't exported from the index, and its header points to Jules's F1-02 factory cutover.
- **D-F14a** (ada, 16:15, Rowan confirmed, `STATUS.md` line 96): the exception for a route that is the composition root. `routes/(shell)/chat/+page.svelte` may import `$lib/modules/messaging/legacy-api` directly, as a commented temporary exception until the F1-02 cutover. Only route files (`+page`, `+layout`) may do so, they're listed in the legacy-api header, and a grep proves it. Every other consumer uses `index.ts`.
- In 4216b73: the chat route carries `// D-F14a: temporary route composition root; Jules's F1-02 factory cutover removes this exception.` `legacy-api.ts` is the old `src/lib/chat/index.ts` plus a 7-line header naming the route as its only outside caller. `git grep legacy-api` finds that route and nothing else outside the module.

## Summary

- **Source check** (`git diff -M fb567d5 4216b73`, 30 files):
  - 14 helper and test files are 100% renames from `src/lib/chat/` to `src/lib/modules/messaging/` (api, commands, engine, markdown, model, sample, testkit, types and their tests).
  - 11 components move from `src/lib/components/chat/` into `modules/messaging/components/` (Avatar) and `features/<slice>/`: open-dm, send-message, inspect-thread, answer-ask, manage-channel and run-command. Only their import lines change.
  - `chat/index.ts` becomes `legacy-api.ts` (+7 header lines, same body). The new `index.ts` re-exports the 9 components, `commandForm` and `turnForm`, 15 model functions and 6 types.
  - The chat route changes only imports (and the D-F14a comment). No markup or logic changed.
  - The tests add 15 named captures to `chat.spec.ts`, two held-snapshot tests (loading then loaded, loading then error), two strip assertions to `org-chart-agents.spec.ts`, and a 3 s animation with a 15 s timeout in `shot.spec.ts`.
  - No import of `$lib/chat` or `components/chat` remains.
- **Pixel comparison:** I captured 125 states in 4 runs (1440×900 and 960×640, OS dark and light, WebKit). That gives **500 pairs, and 493 are byte-identical PNGs.**
  - 373 of the 376 chat pairs (94 chat states) are byte-identical, and all 376 are identical once scroll is normalised.
  - The 7 pairs that differ are scroll, clock and antialiasing noise (see "Pairs that differ").
- **Behaviour:** every scripted step behaves the same in both builds. The URL, title, headings, alerts, focused element, focus-visible style, control counts and full visible text match in every state apart from the clock text below.
  - Two steps fail at 960×640 **in both builds**, because the thread and Members panels crush the conversation (finding 1).
  - The console is the same in both builds. The only messages come from the deliberate 404 (`31-not-found`). The network calls are the same: documents and `package.json` only.
- **Blitz and a11y probes:** I ran them in both builds, and they give the same results.
  - `blitz-B.json` matches `blitz-A.json` key for key, apart from timing.
  - `a11y-B.json` matches `a11y-A.json` in all 24 states, and the ARIA trees match apart from wall-clock message times.
- **Avery's 30 named screenshots:** Avery's worktree has moved. The archives are now in `app-avery-processes/apps/org-ui/playwright-report/previous-avery-worktree/messaging-5646b2c` (the merge, with the same UI tree as 4216b73) and `messaging-216e62b` (the branch head).
  - Against 5646b2c, my e2e run of 4216b73 reproduces **12 of the 30 byte for byte**:
    - the snapshot loading (×2), error and search-empty shots,
    - Needs-you empty and the empty organization,
    - the three dialog `-context` crops,
    - how-it-runs-error and its crop,
    - finite-transition.
  - **15 differ only in wall-clock text.** These are chat-channel, chat-thread, snapshot-loaded, as-owner, search, safe-markdown, bridge-preview, add-channel, new-group-limit, ask-answered, needs-you, command-form, delivery-failed, delivery-retried and office-offline.
    - The e2e suite installs no clock, so the message times ("15:27" against "16:46") and "Waiting N min" move. In the add-channel, new-group-limit and command-form shots, the moved text is the blurred Needs-you text behind the dialog.
    - Each diff is a few narrow bands, 11 to 30 px tall, over those labels. I checked them side by side (`avery-compare/messaging-5646b2c/`).
  - **3 differ by antialiasing:** chat-channel-members (2 px), edit-agent-error (1 px) and its `-context` (2 px), as in F2-01c.
  - Against 216e62b: 14 byte-identical, because edit-agent-error and its crop match too. The other 16 differ in the same way.
  - Every one of Avery's states is what I saw in my own captures.
- **Checks in B:**
  - `bun run check`: `COMPLETED 1170 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
  - `bun run test`: `Tests  1087 passed (1087)` (51 files)
  - **E2E, final line: `229 passed (7.3m)`**, with none flaky or failed (`heavy`, 2 workers). The 2 new snapshot tests are among the 229.

The app is dark only, as in F1-01, so the light runs render the same as the dark runs.

## Findings introduced by 4216b73

None.

## Pre-existing findings (present in fb567d5, unchanged by this merge, not yet catalogued)

Each one renders byte-identically, or gives the same blitz value, in both builds.

1. **S2 · Layout · At 960×640, opening a thread or Members crushes the conversation to 32 px.**
   - Repro: at 960×640, open #release-v1 and choose **↳ 3 replies** (or **Members**).
   - Actual: the 384 px panel sits beside a 256 px chat sidebar, so the conversation column is 32 px wide (544 to 576). Its composer is 24 px wide, and its Send button sits under the thread's.
   - The search field (224 px) paints over the panel's first row, Sam Okafor's.
   - The panel covers the **Members** toggle, so it can't be clicked to close the panel. Only × works.
   - A sent thread reply and a rename can't be seen in the main column. My scripted steps `c08-thread-sent` and `c39-renamed` time out at 960 in both builds.
   - Expected: no clipping at the minimum window, 960×640 (design system, Sizes). The side panel opens over the content at narrow widths (design system, Layout).
   - Fix: below about 1200 px, overlay the thread or Members panel over the conversation, or collapse the chat sidebar.
   - Screenshots: `B/960x640-dark/c06-thread-open.png`, `c35-members-panel.png`, `c39-renamed.png`, `blitz-B/resize-960-thread.png` and `resize-960-members.png`.

2. **S3 · Requirements · While the office isn't running, asks and approvals can still be answered, and fail with "The office didn't answer."**
   - Repro: `/chat?chat=offline`, then Needs you, then **Yes, block**.
   - Actual: the banner says "Read-only from the last copy" and the composer is gone, but Approve, Reject…, the options and Reply… stay active. Pressing one shows a red "The office didn't answer." in the card.
   - Expected (§7, Office not running): read-only from the last copy. Hide or disable the answer buttons, with "The office isn't running. Restart it from Offices."
   - Screenshot: `B/default-dark/c81b-offline-answer-ask.png`.

3. **S3 · A11y · New messages aren't announced.**
   - Actual: the message list has no `role="log"` and no `aria-live`. When Inês's reply arrives, it isn't in any live region. Only the typing line (`aria-live="polite"`) and the toasts are live, and the typing line is empty by the time the reply lands.
   - Expected: a status message reaches assistive tech without moving focus (WCAG 4.1.3). Make the list `role="log"`, or announce "New message from <name>" politely.
   - Evidence: `blitz-B.json` keys `kbd-*-live-after-reply` and `kbd-*-message-list-role`.

4. **S3 · A11y · Focus drops to the page after answering an ask, sending a Reply…, rejecting an approval, or pressing Retry.**
   - Actual: `document.activeElement` is `body` in WebKit and Chromium, because the card or row that held focus is removed.
   - Expected: focus stays somewhere useful (WCAG 2.4.3). For example, the next card in Needs you, the "Nothing needs you" line, or the delivered message.
   - Evidence: `blitz-B.json` keys `kbd-*-after-answer`, `job-focus-after-answer` and `fail-focus-after-retry`. This is the same pattern as F2-01c finding 6, here on the chat screen.

5. **S3 · History · Where you are in Chat isn't in the URL, so Back, Forward and Reload lose it.**
   - Actual: the URL stays `/chat` for every conversation, thread and search. Back from a DM leaves Chat for Agents. Forward and Reload open Needs you.
   - A reload also drops a typed draft (the draft does survive switching conversations), and a "Not delivered" message disappears without a word.
   - Expected: the app already has a link model (`/chat?c=…&m=…`, CHAT-15), so selection can update it. §7 keeps a failed message with Retry until it's sent or dismissed.
   - Evidence: `blitz-B.json` keys `hist-*` and `fail-pending-after-reload`. This is the same history model as F2-01c finding 19, here on chat.

6. **S4 · Validation · Add a channel keeps a stale refusal.**
   - Repro: Add channel, type "General", choose **Add channel** ("There is already a channel named #general."), then type "Release V2".
   - Actual: the refusal stays under the new, free name until the next submit, and the field has no `aria-invalid`.
   - Expected: clear the refusal when the name changes (design system, Errors).
   - Screenshots: `B/default-dark/c33-add-channel-filled.png` and `blitz-B/inv-channel-stale-error.png`.

7. **S4 · Errors · An over-limit message is cleared from the box, then offers a Retry that can never work.**
   - Repro: paste 160,000 "é" (320 KB) and send.
   - Actual: the box accepts it, because `maxlength` counts characters while the limit is 256 KB in bytes. The composer clears, and the row says "Not delivered: That message is too long. Retry · Dismiss". Retry gives the same result.
   - Expected: refuse before sending, keep the text, and say what to do ("Shorten it or attach it as a file"). No Retry for a permanent refusal (design system, Errors).
   - Screenshot: `blitz-B/long-over-limit-960.png`.

8. **S4 · States · A failed thread reply shows only a toast.**
   - Actual: "The office didn't answer." appears as a toast. The text stays in the thread box, with no "Not delivered" row and no Retry. The main conversation does show an inline "Not delivered … Retry".
   - Expected (§7, Delivery failed): the message shows "Not delivered: <reason>" with **Retry**.
   - Screenshot: `blitz-B/fail-thread-reply.png`.

9. **S4 · Input · An unknown slash command is posted as a message.**
   - Actual: `/frobnicate now` goes to #general as text, with no hint.
   - Expected: the six commands in §3. An unknown `/word` should say "No command /frobnicate" and keep the text, as `/task` with no agent already does.
   - Evidence: `blitz-B.json` key `inv-slash-unknown`.

10. **S4 · Copy · The hand-off card says "Linked to T-12".**
    - Actual: the item has no project key and isn't a chip.
    - Expected: rev 3 item numbers carry the project key (`ac-T-17`) and render as chips (CHAT-14).
    - Screenshot: `B/default-dark/c29-handoff-card.png`.

11. **S4 · Contrast · On the selected DM row, the status word is 4.41:1.**
    - Actual: "Asleep" and "Working" (12 px, `text-gray-400` on the active fill) are below 4.5:1.
    - Evidence: `a11y-B.json`, `dm-ines`.

12. **S4 · A11y · Each DM row's name repeats its status.**
    - Actual: the row is named "SO Working Sam Okafor Working", because the avatar's initials and status are part of the name.
    - Expected: "Sam Okafor, Working". Hide the avatar from the name.
    - Evidence: `a11y-B.json`, `aria.needs-you`.

13. **S4 · Keyboard · Escape doesn't close the thread panel.** Focus stays on **↳ replies**, and only × closes it. Evidence: `blitz-B.json` key `kbd-*-escape-thread`.

**Already known, so I didn't re-report them:**
- F2-01c finding 17: `/chat?org=slow` is a blank page until the organization loads (`c93-org-slow`). `/chat?org=fails` is the full-page "500 · Something went wrong" with only **Back to the start** (`c92-org-fails`).
- BUG-18: the 3 px, 50% shadcn focus ring on **↳ replies**.
- BUG-19: there's no `main` landmark. The landmarks are `nav "Main"` and `nav "Chat"`.

## Pairs that differ (7 of 500)

| Pair | Pixels | Why it isn't the merge |
|---|---|---|
| `default-dark`, `default-light` and `960x640-dark` `/c36-add-people-dialog` | 33,270 to 33,400 (**0 after scroll normalising**) | The Add people list's own scroller stopped at 0, 84 or 168 px after checking Kai Lindqvist, differently in each run. The normalised shots are byte-identical, and the text, alerts and focus are the same. |
| `default-dark`, `960x640-dark` and `960x640-light` `/20-chat` | 751 each | "Waiting 8 min" against "9 min", and "11 min" against "12 min", in both directions: a wall-clock minute ticked over, as in F2-01a to F2-01c. |
| `default-dark/16-project-new-task-dialog` | 12 | A few antialiased pixels on the Title field's focus ring and under Cancel. Projects isn't touched by the merge, and the text and focus are the same. |

The diff images are in `validation-f2-01d-shots/diff/`.

## Requirements in B (docs/specs/chat-ux.md at 4216b73, rev 8)

"Met" means met in the browser UI on the TypeScript mock. Every row is the same in fb567d5.

| ID | Status | Evidence |
|---|---|---|
| CHAT-1 | Met | The sidebar has Needs you (2), Mentions, Threads, Channels, Direct messages and "Reading as the owner". A conversation opens with a thread panel (c01 to c09). |
| CHAT-2 | Met (mock) | #general, the project channels (#atlas, #beacon…) and the team channels exist. Add channel and Add people work (c30 to c37), and an automatic channel says "follows the Atlas project" with no Add people (c40). |
| CHAT-3 | Met | DMs show Working, Online, Asleep and Paused as words (c10 to c13). A group stops at 8 with "A group has up to 8 members, you included." (c42). |
| CHAT-4 | Partly | Asks, approvals, hand-offs, announcements (with a reason) and replies appear. There's no channel **Question** kind (rev 6). The enforcement is office-side and can't be seen in the mock. |
| CHAT-5 | Partly | The card has its options and Reply…, says "Answered by Alex Kim: Yes, block" and leaves Needs you (c20 to c28). There's no Home › Needs you (the nav starts at Chat), and "unblocks the agent" can't be seen in the mock. |
| CHAT-6 | Partly | The deadline shows ("If no answer by 14:00, the default applies… (2 h left)"). The applied-default state is in the code but isn't reachable in the sample, so I didn't see it. |
| CHAT-7 | Partly | "Inês Duarte is asleep. Your message will wake it.", then "is typing…", a reply, and Asleep becomes Working (c13, c15). Rev 8's HR lead state is missing: there's no HR lead DM, and `@hr` shows no note. |
| CHAT-8 | Met | Thread replies, and "Also send to #release-v1" puts the reply in the channel (c07, c08). |
| CHAT-9 | Met (drafts) | /bug, /request, /task, /status, /wake and /remind open their forms, and Turn into… works. A missing field says "What happened is needed." (c50 to c61). They save nothing ("Sample data: nothing was saved"). |
| CHAT-10 | Met | "↳ 3 replies · Sam Okafor posted 3 more updates", and the folded posts stay out of the channel. |
| CHAT-11 | Partly | Mute and Daily digest work, with "1 muted message · Show" (c75 to c78). Activity doesn't list chat messages, so "still appear in Activity" can't be seen. |
| CHAT-12 | Met | "You're reading as the owner. You can reply here.", and the reply posts (c17, c18). |
| CHAT-13 | Met | Text, `from:`, `in:`, `has:`, `kind:`, `before:` and `after:`, with a filter help row and "Nothing matches." (c62 to c68). `from:@Kai Lindqvist` (a display name) finds nothing. Only handles work. |
| CHAT-14 | Met | Bold, code, links with `rel="noopener noreferrer"`, `ac-T-17` as a chip, Show more and Show less, and HTML and `javascript:` links kept as text (c70 to c74). A 300-line message collapses ("Show more (285 more lines)"). |
| CHAT-15 | Partly | `/chat?c=…&m=…` opens and highlights the message (c89), and `?c=c-dm-sam` opens the DM (c90). Activity rows have no **Open in Chat** (0 found). An unknown `?c=` opens Needs you without a word (c91). |
| CHAT-16 | Not testable | Agent read rights are office-side. |
| CHAT-17 | Met (preview) | The Slack badge is on Leo's message. `?surface=bridge` shows the asks read-only with **Open in Agentic Org**, and the thread has no composer (c87, c88). |
| CHAT-18 | Met | The hand-off to Sam shows Reply… only to the owner (c29). The accepted one reads "Accepted by Noor Haddad · hh:mm" with no buttons (c29b). Person DMs show "Online". |
| CHAT-19 to CHAT-23 | Not in this build | There's no Browse channels, Join or Request invitation, and no channel Question. The agent-side tools and wake rules are office work (W19). |
| Design system, Empty states | Met | "Welcome to #general · Agents and people talk here…" (c86). |
| Design system, Errors | Partly | "Not delivered: The office didn't answer. Retry · Dismiss" says what to do. The offline answer error and the over-limit Retry don't (findings 2 and 7). |
| Design system, Loading | Not met | "Loading chat…" is plain text, with no skeleton (Avery's `chat-snapshot-loading`). |

## Blitz: owner jobs and attempts to break it (the same in both builds)

- **Owner job, end to end, in 2.7 s, with no console errors:** answer Kai's ask from Needs you, reply in #release-v1's thread, wake Inês by DM (she replies and turns Working), add #launch-plan with Sam and Kai, post a mention with a chip, then find Kai's asks by search and open one in #atlas.
- **Long messages** (960×640): 300 lines collapse. An unbroken 5,000-character word and a 400-character URL wrap with no sideways scroll. An over-limit message fails late (finding 7).
- **Many messages:** 120 sends in a row arrive in order, all unique, with the list kept at the bottom (about 40 s, so about 330 ms each through the queue).
- **Rapid sends:**
  - 10 ⌘/Ctrl+Enter presses on one text post 1 message.
  - A double-click on Send posts 1.
  - The same text sent twice on purpose posts 2.
- **Offline, then Retry** (`?chat=fail`):
  - "Not delivered: The office didn't answer. Retry · Dismiss" is `role="alert"`, and Retry delivers exactly once.
  - The failed row survives switching conversations but not a reload (finding 5).
  - A thread reply only toasts (finding 8).
- **Back and forward:** Chat isn't in history (finding 5).
- **Reload mid-flow:** a draft, a half-filled Add channel dialog and a failed message are all lost. The mock keeps chat in memory, so sent messages, answers and new channels also reset on reload. That's sample data ("Sample data" badge), not a finding.
- **Resizing:** 1440 to 960 with a thread or Members open breaks the layout (finding 1). `blitz.mjs` stops there in both builds (`resize-FAILED`: the panel covers the Members toggle), and `resize-extra.mjs` covers the rest. With no panel, 960×640 fits, search shows its results in the 416 px column, and the chat sidebar scrolls on its own (576 of 960 px tall).
- **Invalid and duplicate input:**
  - A blank name and "!!!" are refused ("Give the channel a name with letters or numbers.").
  - "GENERAL" and "release v1" are refused as taken (case and spaces fold).
  - "🚀 launch" becomes #launch, and "Équipe café" becomes #equipe-cafe.
  - 70 characters is cut to 60 by the field, and the preview shows #aaaa… (40). The preview tells the truth, so that's fine.
  - A second "twice" is refused.
  - A group with the same people reopens the existing group, a DM with Sam reopens his DM, and Open is disabled with nobody picked.
  - Odd search input (`(.*`, `*`, `<b>`, `before:notadate`) gives "Nothing matches." with no error.
  - Whitespace-only messages can't be sent.
  - An unknown `/command` posts (finding 9).

## UX, IA, visual and a11y

- **Keyboard** (WebKit Option+Tab, Chromium Tab):
  - The sidebar is in a sensible order, with a 2 px indigo outline on every item.
  - The message actions toolbar shows its ring on focus.
  - Add channel opens with focus on Name, and Escape returns focus to **Add channel**.
  - Focus after Send stays in the composer.
  - The gaps are findings 3, 4 and 13.
- **Accessible names:** no unnamed controls in the 24 probed states.
  - Messages are `article`s named "<name>, <time>".
  - Counts read "atlas 2 unread" and "general 1 mentions".
  - The composer suggestions are a `listbox` with `option`s.
  - "Not delivered" is `role="alert"`.
- **Contrast:** finding 11. Everything else probed is 4.58:1 or better. The lowest are the Approve and Send labels and the "1" mention badge.
- **At 960×640:** every dialog fits (448 to 512 px wide, no scroll) and nothing scrolls sideways without a panel. Finding 1 is the exception.
- **IA:** Chat is the first nav item (the spec says second, after Home; there's no Home in this build). "Sample data" is shown on every conversation.
- **Tokens:** raw Tailwind gray, indigo, amber and red, dark only, as in F1-01 (app-wide).

## What I covered

- **94 chat states, in both builds and 4 runs each:**
  - Navigation: Needs you, Mentions, Threads, #general, #release-v1, a thread (open, also send, sent, closed), DMs (Working, person Online, Paused, Asleep, woken and replied), reading as the owner (list, conversation, reply), and a group DM.
  - Asks: Reply… (open, typed, sent), approval Reject… (open, reason, rejected), Needs you empty, answered in #atlas, an option answered with its toast, and a hand-off (pending and accepted).
  - The channel dialog: open, an invalid name, a taken name, filled, created, Members, Add people, saved, Rename, renamed, and an automatic channel's members.
  - The new message dialog: open, the group limit, a group started, and its members.
  - The command dialog: the slash popup, /bug (form, missing field, drafted), /request, /task, /status, /wake, /remind, /task with no agent, the message menu, and Turn into a bug.
  - Search: text, from, has:ask, in, empty, combined, and opening a result.
  - Markdown: #general, Show more, unsafe HTML, the mention popup and the completed mention.
  - Mute: the menu, muted, shown, and the digest.
  - Offline: the channel, a thread, and answering an ask.
  - Not delivered, Retry, a second failure, and Dismiss.
  - The empty organization.
  - The bridge preview and its thread, and a bridged message.
  - Links: a message, a DM, and an unknown link.
  - `?org=fails`, `?org=slow`, `?mock=turned_off` and `?mock=unreachable` (once connected, the office scenarios don't change Chat).
  - A keyboard path of 4 steps.
- **The F1-01 main set:** 31 states.
- **Blitz** (`blitz.mjs`, `resize-extra.mjs`) and **a11y** (12 states × 2 sizes) in A and B.
- **Checks in B:** check, unit and the full e2e suite. **Comparison with Avery's 30 shots.**

## Method

- The scripts are in `validation-f2-01d-shots/`, copied from `validation-f2-01c-shots/` and adapted. I didn't edit the originals.
  - `validate.mjs` takes `PAIR=A=5201,B=5202`.
  - `blitz.mjs`, `a11y.mjs` and `resize-extra.mjs` run with `node <script> <A|B>`.
  - `compare-avery.mjs` runs with `node compare-avery.mjs <archive>`.
- The scripts import Playwright from the B worktree, which is now removed. Point the import at a current `apps/org-ui/node_modules` before re-running them.
- Each build ran `bun run dev --port 520x` from its own detached worktree (A fb567d5 on 5201, B 4216b73 on 5202, C 4b64032 on 5203).
- Each flow ran in WebKit in a fresh context, with the clock installed at 2026-11-10T12:00Z. It waited for finite animations only, then took screenshots with `animations: 'disabled'`. Each step also has a `.norm.png` with every scroller at the top.
- E2E ran with `PW_PORT=4301 ~/.agency/bin/heavy bunx playwright test --workers=2` in B. My first, unwrapped run was stopped when the heavy-slot rule arrived.

## Files

- Report: `workspace/validation-f2-01d.md`
- Screenshots and data: `workspace/validation-f2-01d-shots/`
  - `{A,B}/<run>/<state>.png` and `.norm.png`, where `<run>` is `default-dark`, `default-light`, `960x640-dark` or `960x640-light`
  - `diff/<run>/` (the 7 differing pairs)
  - `blitz-{A,B}/` and `blitz-{A,B}.json`
  - `a11y-{A,B}.json`
  - `avery-compare/<archive>/` (`avery-*.png`, `mine-*.png` and `avery-compare.json`)
  - `summary.json` and `results.json`
  - Logs: `validate-run1.log`, `blitz-*.log`, `a11y-*.log`, `e2e-B.log`, `check-B.log`, `unit-B.log` and `avery-compare-*.log`
  - Scripts: `validate.mjs`, `blitz.mjs`, `a11y.mjs`, `resize-extra.mjs` and `compare-avery.mjs`
