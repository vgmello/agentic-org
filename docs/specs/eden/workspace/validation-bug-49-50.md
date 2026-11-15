# Validation: BUG-49 (narrow Chat panels and the Conversations overlay) and BUG-50 (live-log scope), v1 8ef34f8 vs b88d1bd

- **Build under test:** v1 `8ef34f8` (Avery, merge of `avery/chat-accessibility`). **Parent:** `b88d1bd`.
- **Requirements:** `docs/specs/chat-ux.md` rev 9 §1.1–1.2 and CHAT-29..33 (CHAT-24..28, 34, 35 are other rev-9 work, not touched by this merge); `docs/specs/app-shell-ux.md` §2 (320 px rule), §6 and SHELL-28; the brief for BUG-49 and BUG-50.
- **How:** throwaway worktrees A = `b88d1bd`, B = `8ef34f8`, both on `bun run dev --mode e2e` (A 5261, B 5262). WebKit is the main engine, Chromium the check. Widths 1440, 1279, 1024, 960, 900 (rail, content 836), 820, 720, 639, 600, 320 (all ×900), plus 720×450 (200% zoom of 1440×900) and 200% text (`html{font-size:200%}`) at 1440, 1279 and 1024. I tested as a reviewer only and changed nothing in the product repo. Worktrees and servers are removed.
- **Date:** 2026-11-11.

## Verdict

BUG-49 and BUG-50 are **met in their core**. The Conversations overlay shows exactly when the chat content is under 768 px (1024, 960, 820, 720, 639, 600, 320, and 1279 and 1024 at 200% text), is 256 px wide, modal, focus-trapped, opens on the selected item, returns focus to the button after Esc, scrim and Close (both engines, mouse and keyboard; BUG-59's WebKit trap doesn't recur here), focuses the heading after a choice, and Back closes only it (52 of 52 repeat trials). Thread and Members overlay below 1280, make the conversation inert, take focus on open, and fill the width below 640. Channel switches, search and history loads are now silent.

Gaps, all new:
- **N1 (S3):** in WebKit, a thread opened with the mouse returns focus to the Messages region on close, not to "↳ N replies" (CHAT-31's "closing returns focus to its opener", WebKit only; Members is fine).
- **N2 (S3):** the message log still announces non-message changes: "Waiting N min" and the deadline line every minute while an ask is open, the "↳ N replies" count, and your own message twice (Sending… then delivered).
- **N3 (S4):** at 200% text, the fixed 256 px list truncates names and the overlay's "Conversations" title runs under its Close button.
- **N4 (S4):** Chat's search field stays 224 px below 640 px (SHELL §2: full width).

## 1. Requirement by requirement (WebKit unless noted)

Probe JSON: `validation-bug-49-50-shots/narrow-webkit/B/narrow.json`, condensed in `narrow-webkit/B/summary.json` (A: `narrow-webkit/A/…`; Chromium: `narrow-chromium/{A,B}/…`). Shots per width in `narrow-<engine>/<build>/<w>x<h>/` (`01-general`, `02-open-mouse`, `03-chose-release-v1`, `04-before-back`, `05-after-back`, `06-release-v1`, `07-thread-open`, `07-members-open`). Code lines are at `8ef34f8`, `apps/org-ui/src/…`.

| # | Requirement | Result | Probe value (B; A in brackets) | Code |
|---|---|---|---|---|
| 1 | Overlay only when chat content < 768 px | Met | content 1152/991/836 → inline 256; 736 (1024), 672 (960), 756 (820), 656 (720, 720×450), 639, 600, 320 → overlay. 200% text: 864 inline, 703 and 448 overlay. Chromium identical. [A: inline 256 everywhere; at 320 the message column is 0 px] | `modules/messaging/features/open-dm/ChatSidebar.svelte:57` (`inline = rect.width >= 768`, measured on the chat frame by ResizeObserver) |
| 2 | Overlay 256 px, capped at available width | Met | 256 at every width; x = 288 (full nav), 64 (rail), 0 (top bar); scrim strip 480/416/500/400/383/344/64 px | `ChatSidebar.svelte:160` (`Math.min(256, bounds.width)`) |
| 3 | "Conversations" button left of the title | Met | button at x20/308/84 left of `h2`, `aria-haspopup=dialog`, `aria-expanded` false→true, `aria-controls` = the dialog id | `routes/(shell)/chat/+page.svelte:524` |
| 4 | Accessible name adds the count; amber badge | Met | "Conversations, 2 need you" → after one answer "Conversations, 1 needs you" (badge 1) → after both "Conversations" (no badge). Badge `aria-hidden`, bg `oklch(0.828 0.189 84.4)` = amber-400, same as the list's badge (`extra-webkit/B/extra.json` `names`) | `+page.svelte:524-526` |
| 5 | Opening is modal; content behind inert | Met | `role=dialog` "Conversations", `aria-modal=true`; conversation `inert=true`; 25 Tab presses never leave the dialog (`trapEscaped 0`) at every width | `+page.svelte:519`, `ChatSidebar.svelte:155` |
| 6 | Focus on open → selected conversation's item | Met | `button[data-conversation=c-general] aria-current=true` (and `c-release-v1` when that is open), mouse and keyboard, both engines | `ChatSidebar.svelte:161-165` |
| 7 | Choosing closes it, focus → that conversation's heading | Met | mouse → `h2 #release-v1`, Enter → `h2 #general`, Needs you → `h2 Needs you`; overlay closed; `h2 tabindex=-1` with a ring | `+page.svelte:123-125` |
| 8 | Esc, Close, scrim → focus to the button | Met | all three × mouse/keyboard open → `button "Conversations, 2 need you"`, `aria-expanded=false`, every width, both engines | `ChatSidebar.svelte:167`, `+page.svelte:129` |
| 9 | Back with the overlay open closes only it (SHELL-28, CHAT-33) | Met | URL stays `/chat?c=c-release-v1`, overlay closed, focus on the button; `back-repeat.json` 36/36 and `seq-repro.json` 16/16 across 1024/720/320, both engines. (One Chromium pass at 1024 navigated and lost focus, and one at 720 timed out, both while the full e2e ran alongside; they didn't recur in 52 trials or a rerun, so I don't report them.) | `+page.svelte:453-457` |
| 10 | A dialog opened from the overlay returns sensibly | Met | Add channel from the overlay: full width at <640 (639/600/320 px), 448 px above; Esc → focus on the Conversations button | `+page.svelte:134-150` |
| 11 | Thread and Members overlay below 1280 px | Met | 1279: `absolute`, x544 w735 over a 991 frame (list stays inline); 1440: `static` w384 beside. 1279 at 200% text: overlay. [A: also overlay below xl, but nothing inert] | `ThreadPanel.svelte:62`, `MembersPanel.svelte:56`, `+page.svelte:199` (`matchMedia('(width < 1280px)')`) |
| 12 | Content behind is inert while one is open | Met | conversation `inert=true` 1279→320; 12 Tabs reach the conversation 0 times (1440 inline: 12, as intended) [A: `inert=false`, 12/12] | `+page.svelte:519` |
| 13 | Focus moves into the panel | Met | `button "Close thread"` / `"Close members"` inside the panel at every overlay width, both engines [A: focus stays on the opener behind] | `+page.svelte:162-171`, `ThreadPanel.svelte:56` |
| 14 | Closing returns focus to its opener | **Partly met** | Keyboard open, then Esc → opener (`↳ 3 replies`, `Members`), both engines. Members by mouse → `Members` ✓. **Thread opened by mouse, then Close → `div "Messages"` (the list region) in WebKit at every width**; Chromium → `↳ 3 replies` (N1). Same in A. | `+page.svelte:433` |
| 15 | Panels full width below 640 px | Met | 639 → 639/639, 600 → 600/600, 320 → 320/320 (`fullFrame`); also full when the list is an overlay (1024: 736/736) [A: 320 → w64] | `ThreadPanel.svelte:62` (`left-0` when the list isn't inline) |
| 16 | 320 rule: no sideways scroll; no text column < 200 px | Met | `pageX 0`, `mainX 0` at every width and state; message body 240 px at 320; no wrapped text column < 200 px in base, thread, members or overlay at 600 and 320 (`extra.json` `rule-*`) [A at 320: body 0 px] | `+page.svelte:555`, `MessageRow.svelte:68` |
| 17 | 320 rule: search fields full width below 640 | **Not met** | search 224 px in a 560 px row (600) and 280 px row (320) (N4) | `+page.svelte:543` (`w-56`) |
| 18 | 200% text | **Partly met** | layout holds (no sideways scroll, overlay below 768), but names truncate and the overlay title overlaps Close (N3) | `ChatSidebar.svelte:153,169` |
| 19 | BUG-50: switches, search and history loads not announced | Met | see §2 | `+page.svelte:555` (`role=region`), `:610-611` (`{#key conversation.id}` + `role=log`) |
| 20 | BUG-50: new messages in the open conversation announced | Met | incoming reply in the open DM announced once | as above |
| 21 | BUG-50: nothing else from the log | **Partly met** | minute ticks, reply counts and the sending row are announced (N2) | `AskCard.svelte:86,88`; `MessageRow.svelte:112,120`; `+page.svelte:649` |

## 2. The live log (BUG-50)

Method: `live.mjs` records what each live region offers a screen reader, from DOM mutations, using the usual AT rules: a region announces only changes made after it was already in the page, `aria-relevant` (default "additions text") selects added nodes and text changes, and nothing inside inert or hidden content counts. It's a model, not a real screen reader, so it shows *what* is offered, not VoiceOver's wording. 1440×900, both engines, A and B. Output: `live-<engine>/<build>/live.json` and `live-*.log`.

Regions in B: `role=log "Messages"` (polite, additions text) wraps **only** an open conversation's rows and is recreated per conversation; the outer `message-list` is now `role=region "Messages"`. Needs you, Mentions, Threads and Search have **no** live region. The thread panel keeps its own `role=log "Thread messages"`. Shell title announcer, typing line and toasts are separate.

| Action | A (b88d1bd) hears | B (8ef34f8) hears |
|---|---|---|
| Initial load / deep link `?c=c-release-v1` | the whole conversation (every message) | nothing |
| Channel switch (#release-v1 → #general) | the whole of #general, plus the title | only the title "#general · Chat · Agentic Org" (SHELL-25) |
| Switch to a DM, to Needs you | the whole DM / both ask cards | only the title |
| Search "create-org", then "review" | "4 results Filter by from:@name …" + every result, and again | nothing |
| Search cleared | the whole conversation again | nothing |
| Browser Back / Forward (history load) | the whole place each time | only the title |
| Reply arrives in the open DM | the reply | "Inês Duarte is typing…" (typing region), then "Inês Duarte 00:34 Sample reply: got it. …" |
| Reply arrives in **another** conversation | nothing | nothing |
| 30 s refresh with nothing new | nothing | nothing |
| Own message sent | "… Live log probe one Sending…" then "… Live log probe one ✅ 👀" | same: twice (N2) |
| Reply in a thread | thread log: the reply + "4 replies"; main log "↳ 4 replies" | same (N2) |
| Minutes pass with an ask open (#atlas, fake clock) | each minute: "If no answer by 02:35 … (1 h 58 min left) \| Waiting 13 min \| Waiting 10 min" | same (N2) |

Not covered: switching threads while the panel stays open, because the sample has one thread per channel and an empty Threads view.

## 3. New findings

### N1 (S3): WebKit, a thread opened with the mouse returns focus to the Messages region, not its opener

- **Repro:** Safari/WebKit, any width (e.g. 1279 or 320). In #release-v1 click "↳ 3 replies", then click Close thread (or press Esc).
- **Expected:** focus returns to "↳ 3 replies" (BUG-49; CHAT-31; WCAG 2.4.3).
- **Actual:** focus lands on `div[role=region] "Messages"`, and at <1280 the place you were reading is lost. Opening with the keyboard, Chromium, and Members (any way) are all fine.
- **Cause:** `openThread` takes the opener from `document.activeElement` (`routes/(shell)/chat/+page.svelte:433`). WebKit doesn't focus a button on click, so the active element is the focusable list region the click landed in. Members passes `event.currentTarget` (`:287`, `toggleMembers`), which is why it works. Fix: pass the clicked element from `MessageRow`'s `replies-link` (`MessageRow.svelte:112`) the way Members does. Same in A, so it's older than this merge, but BUG-49 states this requirement and the e2e runs on Chromium only.
- **Evidence:** `narrow-webkit/B/summary.json` `threadClose: div "Messages"` at all 13 cases; `narrow-chromium/B/summary.json` `threadClose: button "↳ 3 replies"`.

### N2 (S3): the message log still announces non-message changes

- **Repro:** VoiceOver on. Open #atlas (an ask with a deadline) and wait a minute; or send a message; or reply in a thread.
- **Expected:** only new messages in the open conversation are announced (BUG-50).
- **Actual:** with `aria-relevant="additions text"`, every text change in the log is spoken:
  - each minute: "If no answer by 02:35, the default applies: No, allow (default). (1 h 58 min left) | Waiting 13 min | Waiting 10 min" (`AskCard.svelte:86` waiting, `:88` deadline; `now` refreshes every 30 s, `+page.svelte:190,212`),
  - your own message twice: the pending row ("… Sending…", `MessageRow.svelte:120`, rendered inside the log at `+page.svelte:649`) and then the delivered row,
  - "↳ 4 replies" in the main log when a thread gets a reply (`MessageRow.svelte:112`), and "4 replies" again in the thread log (`ThreadPanel.svelte:69`).
- **Fix direction:** `aria-relevant="additions"` on both logs, with the reply counts, clocks and pending row outside the log or `aria-hidden` for live purposes; announce delivery once.
- **Evidence:** `live-webkit/B/live.json` steps 11, 16, 18; the same in Chromium and in A.

### N3 (S4): at 200% text, the 256 px list truncates names and the overlay title runs under Close

- **Repro:** 200% text (browser text size), Chat at 1440 (inline) or 1279/1024 (overlay).
- **Expected:** names stay readable and the overlay's title and Close don't overlap (WCAG 1.4.4; the spec's 256 px reads as the default size).
- **Actual:** "engine…", "platf…", "relea…", "Needs you" on two lines; in the overlay "Conversations" runs under the × button. A used `w-64` (16 rem, 512 px at 200%, which then crushed the conversation); B pins `w-[256px]`.
- **Code:** `ChatSidebar.svelte:153` (`w-[256px]` inline), `:160` (overlay width), `:169` (`SheetHeader … pr-12` with an absolute Close). Product call: let the list grow with text (e.g. `min(16rem, …)`) and keep the overlay rule, or wrap names.
- **Evidence:** `narrow-webkit/B/1440x900f/01-general.png`, `1024x900f/02-open-mouse.png`.

### N4 (S4): Chat's search field isn't full width below 640 px

- **Repro:** Chat at 600 or 320.
- **Expected:** SHELL §2: "search fields are full width below 640 px".
- **Actual:** 224 px in a 560 px (600) or 280 px (320) row.
- **Code:** `routes/(shell)/chat/+page.svelte:543` (`w-56 max-w-full`); add `max-[640px]:w-full` on the input and its wrapper (`:540`).
- **Evidence:** `extra-webkit/B/extra.json` `rule-600`, `rule-320`; `narrow-webkit/B/320x900/01-general.png`.

### Notes (not findings)

- Resizing with the overlay open: 720 → 1024 keeps it open and moves it to x288; → 1440 closes it and focuses the inline item (`c-general`); back to 600 focuses the button (`extra.json` `resize`). Reasonable; the spec doesn't say.
- At ≥1280 the thread/Members open beside the conversation and focus stays on the opener (no move needed, nothing inert), as the spec's "overlay below 1280" implies.

## 4. Pixel comparisons

`validate.mjs` (the chat-followup set: every chat flow c01–c99 plus the F1-01 main screens 01–31), WebKit, 1440×900 and 960×640, dark and light, A vs B: **500 pairs, 299 byte-identical, 201 differ.**

- **1440×900: 5 differ (3 dark, 2 light), all explained:**
  - `c44-group-members` (1430 px, both schemes): the Members panel's title wraps "Pax Moreau · 8 members" at a different point, because B adds `min-w-0 break-words` to the panel `h2` (`MembersPanel.svelte:58`). Intended, harmless.
  - `25-chat-dialog-cancelled` (1066 px, both): after cancelling Add channel, focus returns to the Add channel button with its 2 px ring (A: focus on `body`). Intended (BUG-49 focus return).
  - `c36-add-people-dialog` (33 400 px, dark only): the dialog's member list starts scrolled 84 px in B. The scroll-normalised shot is **0 px** different, so it's scroll position only.
  - The only other behaviour diff at 1440 is a Bits UI id shift (`bits-c28` → `bits-c29`) from the new `$props.id()`. Same 3 console messages in A and B (the 404 step).
- **960×640: 98 differ per scheme, all from the intended layout change.** The chat content there is 672 px (< 768), so B shows the Conversations button and the list as an overlay instead of the inline column: every chat state differs, with 22 fewer visible controls (the hidden list), `h2` lists without CHANNELS/DIRECT MESSAGES, the heading or Close focused after moves (A: `body`), and a thread/Members panel covering the whole content area. The run opens the overlay when the list is hidden. Seven steps per scheme still couldn't run in B, all for expected reasons: `c40` and `c81b` try to reach the list while a Members or thread panel covers the conversation (the Conversations button is inside the inert conversation, so you close the panel first; by design), and the main set's `20`–`24` click the inline list directly (`mainFlow` isn't adapted), with `25` following from them. No unexpected visual change; spot-checked `c06` (thread over the full 672 px content) and `c62` (Search with the Conversations button).

## 5. Local checks on 8ef34f8 (B worktree, each through `~/.agency/bin/heavy`)

- `bun run check`: `COMPLETED 1160 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS` (exit 0)
- `bun run test`: `Test Files  50 passed (50)` / `Tests  1347 passed (1347)` (exit 0)
- `bun run test:e2e --workers=2` (`PW_PORT=4491`; builds first: `Wrote site to "build"`, `✔ done`; then `Running 345 tests using 2 workers`): `345 passed (10.2m)` (exit 0)

## Already catalogued, seen again, not re-reported

BUG-59's WebKit Menu focus loss (the Conversations overlay doesn't have it). The ask/deadline behaviour itself is BUG-41/58 territory; only its announcement is N2.

## Method and files

All under `workspace/validation-bug-49-50-shots/`:

- `orig-nav.mjs`, `orig-menu.mjs`, `orig-menu-focus-return.mjs`: copies of the BUG-52/53 scripts (originals untouched).
- `narrow.mjs` (adapted from menu.mjs/menu-focus-return.mjs): overlay, toggle, focus, Back, panels, overflow per width. Output `narrow-webkit/{A,B}/`, `narrow-chromium/{A,B}/` (+ `summary.json` from `summarize.mjs`), logs `narrow-*.log`. B WebKit's font cases are in `narrow-font.json`, merged into `narrow.json`; the Chromium 1024/720 rerun is in `narrow-rerun.json` (first pass `narrow-run1.json`).
- `back-repeat.mjs` / `back-repeat.json`, `seq-repro.mjs` / `seq-repro.json`: Back with the overlay open, repeated.
- `extra.mjs`, `extra-webkit/B/`: 320 rule columns, search width, toggle names 2/1/0, resize while open.
- `live.mjs`, `live-{webkit,chromium}/{A,B}/live.json`, `live-*.log`: §2.
- `validate.mjs` (copied from validation-chat-followup-shots, ports 5261/5262; list actions open the overlay when it's hidden), output `regress/` (`summary.json`, `results.json`, `diff/`, `validate-run.log`).
