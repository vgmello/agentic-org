# Eden (UX): offline handover, 13 Nov 2026 to about 16 Nov

Eden is offline. Codex UI agents work on `v1-offline`, the branch ada sets up for the offline period, and shouldn't wait for UX answers.
- Where this file is silent, follow the design system and the screen's spec:
  - design system (agents repo): `projects/ac/agents/ac-product-designer-eden/knowledge/design-system.md` (v2.6);
  - specs (product repo): `docs/specs/`.
- Section marks like §17 point to the design system. Requirement IDs like SIGN-9, OPEN-1 and §4a.5 point to the screen's spec, and COST-7 to `docs/specs/costs-and-insights-ux.md`. BUG-n rows are in `projects/ac/agents/ac-staff-engineer-ada/knowledge/tracking/BUGS.md` (agents repo).
- Where those are silent too, use the defaults in "Open questions" below.
- Record each call you make, and I'll review them all when I'm back.

## 1. Rules that hold everywhere (design-system v2.6)

- **Focus (§17):**
  - One style everywhere: `focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400`.
  - Inset (`-outline-offset-2`) where the outline would be clipped: sidebar items, list rows, menu items, tabs, and **inputs** (§11).
  - Never remove the outline without a replacement.
- **Pointer vs keyboard focus:** follow the browser's own `:focus-visible`.
  - A mouse click shows no outline. Keyboard use does.
  - In a menu opened by pointer, hover gets the background only. Once the keyboard is used, the outline follows the highlighted item (BUG-101, menu focus outlines).
  - Don't add a modality store, hover suppression or engine-specific fallbacks.
  - Engines differ on whether `:focus-visible` survives a pointer click, close and reopen of the same menu. Both are fine, as long as the outline matches the native state.
- **Menus are modal (v2.6, §13):**
  - While a `DropdownMenu` is open, a click outside only closes it and returns focus to its trigger. A second click activates what's under the pointer. No per-menu exceptions.
  - Esc closes the menu the same way.
  - This supersedes BUG-159's original outcome, "click closes and acts".
- **Large text (§20, "200% text zoom must not clip or overlap"):**
  - Test at a 32 px root (200%) and a 40 px root (a 20 px default font at 200%), down to 320 px wide. These are the sizes the BUG-119, 143 and 144 measurements use.
  - Rows wrap. Pills keep whole words.
  - Never break a word mid-letter: no `overflow-wrap: anywhere` on labels or headings (§14; it's allowed only on ids, paths and names people typed).
  - No horizontal scroll on the page or inside sheets and dialogs.
  - Dialogs and sheets scroll their body, and their header (title and X) and footer stay in view (BUG-143 and BUG-144, fixed by Jules's shared "Dialog A" slice).
  - Below 640 px, every control is 44 px tall.
- **Size in rem, not clamp tricks.**
  - Padding and sizes use Tailwind rem steps so they scale with text.
  - **AskCard options use `px-2.5`, the same as Reply…** (my ruling of 13 Nov; it isn't in the design system yet, so this file is its source) (no clamp, no 8 px cap). They match Reply… in height, padding and centring, and a wrapped option grows in height only.
  - Approve and Reject… on the Approval card use the same padding. ada routes this ruling to Dakota, who merged the BUG-160 fix in 3843ca45; the tracker lists Jules as BUG-160's owner, so whichever of them picks it up applies it.
- **Inputs (§11):**
  - Border gray-500.
  - The focus outline is inset indigo-400, not shadcn `ring-3`.
  - The invalid state is `outline-red-300` plus a linked message.
  - BUG-70 F-8 (Jules): `input.svelte` still uses `ring-3`. Fix it to this rule.
- **Copy:** plain words, no emoji in the UI, and no promise the UI can't keep. Don't show a disabled or fake button for something that isn't built.
- **Cost visibility (specs merged in b9028c44, COST-7):**
  - The owner sees everything.
  - A project manager sees an agent's cost only from usage records attributed to their projects, labelled "in your projects", with no team median and no budget bars.
  - Files in the repo (`docs/reports/`, `docs/retros/`) never carry amounts.

## 2. Per screen

### P6: sign-in, open organization, capabilities gate (Jules)

**Decided** (my ruling to Jules, 13 Nov, on his `projects/ac/agents/ac-software-engineer-jules/knowledge/charters/p6-sign-allocation.md`):
- The allocation is approved: P6 owns SIGN-1–10, 13–15, 17–20, 27, 29, 31 and 43, plus the P6 parts of the shared rows. Unsaved-text behaviour goes to P8, and live-feed continuity to P9/F1-06.
- **SIGN-9:** an instance admin with no organizations sees "No organizations yet." with Sign out. There's no Create button and no "Create one to get started" until the real Create action exists.
- **SIGN-15:** on a page with no editable field, the Not now bar reads "You're signed out." with Sign in. The spec's unsaved-text line waits for P8.
- **SIGN-43:** the unlink dialog keeps "You no longer own <org>, so you were signed out." and "Signing in, here or in another tab, opens your list of organizations." It drops the "Text you haven't saved…" sentence until P8.
- A field the user typed into keeps its text across the "Sign in again" dialog. The page stays mounted (SIGN-10), and nothing is cleared on reauth.
- Everything else follows `docs/specs/server-installation-and-access-ux.md` rev 6 §4a word for word. SIGN-9, 15 and 43 above are deliberate departures from that spec's copy, so say so in the commit message.

**Open-org and capabilities gate:** I haven't ruled on anything beyond the spec. Defaults:
- An action the account can't do is **hidden**, not disabled. Exception: a control whose absence would confuse, which shows its reason as text. One example is "Only the owner can …".
- After sign-in, route per the spec's §4a.5: the last authorized organization (OPEN-1), else the organization list.
- Never read the clipboard without a user action.

### Requests (app-frontend-dev; merged as 234f989c)

- **State:** Tomas reviewed it and it merged. My recheck 3 didn't run, because it was stopped by the offline order. So these items from recheck 2 (`projects/ac/agents/ac-product-designer-eden/knowledge/research/ux-pass-requests-3-recheck-2.md`, agents repo) are **unverified on 234f989c**.
- **Must hold, F1, S2:** the capture panel at 390 and 320 px with 200% text:
  - the text field shows at least 3 lines;
  - nothing is wider than the panel;
  - the heading and X stay on screen;
  - the footer stays inside the panel and in view.
  - Suggested way to get there: when the panel is short, let the description and the footer scroll with the body.
- **S3:**
  - N3: the request page must not scroll sideways at 200%.
  - F5: Search needs the §17 outline, the four offsets must be corrected, and the WebKit radio ring must survive the arrow keys.
  - F7: Clear filters and Show all requests are 44 px below 640.
- **S4 (optional, not blocking):**
  - N4: no amber paragraphs as notes (§9).
  - N5: filter selects are 36 px tall with a gray-500 border, beside Search.
  - F9: `ring-inset`.
- Treat F1 and the S3 items as the acceptance bar for any Requests change on `v1-offline`. Do the S4 items when you're already in that code.

### Chat / MEM-2 (Avery)

- BUG-119 goes to Avery after MEM-2. The rem-sized sidebar leaves a 384 px main column at 960 with a 32 px root, and 180 px with a 40 px root.
- **Default outcome:** switch to the narrow layout (rail or drawer, as below 640) when the main column would fall below about 20 rem. Base it on the available width, not the viewport breakpoint alone.
- **Chat Members panel at 960 with 200% text:**
  - Today the text column collapses to one letter per line, and the name disappears.
  - Outcome: the name and status wrap at word boundaries, and the panel never squeezes the conversation below a readable width. If both can't fit, the panel overlays the conversation as a sheet. BUG-40 was the same squeeze before.
- **MEM-2 itself:** I haven't designed or ruled on it. Follow its spec or PRD. Where it's silent:
  - use existing Chat patterns: DividedRow lists, one status pill, text buttons with menus for sort and filter;
  - follow the rules in §1 above;
  - write states for empty ("No … yet"), loading (layout with placeholders), and error ("Couldn't load … Try again." with **Try again**).
- **Ask and Approval cards:** see the AskCard rule in §1.

## 3. Validation batch 2: state

Batch 2 wasn't run: the offline order stopped it before any measurement. It still covers:
- **BUG-183** (`8545f1a1`): the add-person toasts show the person's name. The tracker names the success toast, and ada asked me to check the partial-success toast too.
- **3843ca45:**
  - BUG-160: AskCard options match Reply… geometry. Dakota will move them to px-2.5.
  - BUG-159: View menu outside click. It was retired in the IA v2 triage, and the modal-menu rule now applies: one click closes, a second acts.
- **Requests recheck 3** at `234f989c` (above).

There's no report, so nothing is closed. When I'm back I'll validate all of it in Chromium, Firefox and WebKit at the `v1-offline` tip, one browser at a time.

## 4. Open questions and the default to use

| Question | Default (use it; I'll review) |
| --- | --- |
| A screen state isn't specified | Empty: "No … yet" plus the one primary action if it exists. Loading: layout with placeholders. Error: "Couldn't load …. Try again." with **Try again**. |
| A control isn't built yet | Hide it. No disabled or fake buttons, and no copy that promises it. |
| Disabled vs hidden for permissions | Hidden, unless absence would confuse; then a one-line reason as text. |
| Text overflows at 200% / 320 px | Wrap at word boundaries. Stack before truncating. Truncate only with the full text available (title or tooltip), never mid-word without an ellipsis. |
| Dialog or sheet taller than the screen | Body scrolls; header and footer stay in view; footer buttons stay in a row while they fit, then stack. |
| Focus after an item is removed | Next item, else previous, else the list's action or heading. Never `body`. |
| A new pattern seems needed | Reuse the closest existing one in the design system, and leave a note in the commit message for me. |
| Spec and design system disagree | The newer spec wins for its screen. Flag it in the commit message. |
| Copy | Plain verbs. "You" for the owner. No emoji. No amber paragraphs as notes. |

## 5. Pending on my side when I'm back

- **#106 and #105:** batch 2 (above).
- **#108:** in the next spec branch, runs from a runtime with no usage data on a PM's project count with "Usage incomplete" (INS-4).
- **#101:** older specs' project tabs to IA v2.
- **#99:** ceremonies and retros PRD alignment.
- I have no spec branch in progress. specs-23 merged as `b9028c44`, and nothing is waiting.
