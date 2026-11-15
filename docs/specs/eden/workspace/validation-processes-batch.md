# Validation: the processes bug batch (e27153d), compared with its first parent 07b9b9d

**Verdict: six of the seven bugs are fixed and nothing else changed. BUG-18 is only partly fixed: the sheet is now 440 px, but the Close button's 2 px outline with its 2 px offset is never painted. The only focus indicator is a 1 px border at the button's edge. Every other pixel difference comes from a listed fix. The Report a bug dialog and the rest of the main screens are unchanged, apart from Chat's "Waiting N min", which depends on run timing.**

- Change: merge `e27153d` ("Merge avery/processes-bugs: process presentation and draft spacing") on `v1`, compared with its first parent `07b9b9d` (F1-03).
- Diff: 8 files, +259 −30.
  - `ProcessSheet.svelte`, `present.ts`, `view.ts` and `processes/+page.svelte`.
  - `DraftableText.svelte`, with one line.
  - `e2e/processes-bugs.spec.ts` is new, with 197 lines.
  - Two e2e expectations changed to match the new copy.
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. The worktrees and dev servers are gone (see Method).

## Per bug

Before = B (`07b9b9d`), after = C (`e27153d`). WebKit unless noted. The screenshots are in `validation-processes-batch-shots/bugs-B/` and `bugs-C/`, with the same file names in each.

| Bug | Result | Before | After | Code (e27153d) |
| --- | --- | --- | --- | --- |
| BUG-14 · 960×640, Show › Web, drift badges squeeze the name | **Fixed** | "Code review (Web)" is 23 px wide and 192 px tall: 8 lines of 1 or 2 letters | 576×24 px, on one line. The badges wrap under the name. The narrowest name column in any of the 14 places is now 261 px (it was 23) | `routes/(shell)/processes/+page.svelte:121` `min-w-48 flex-1` |
| BUG-15 · missing space after the colon, and the capital letter | **Fixed** | "Design handoff:Details not recognized (overridable).", "Standup:Two processes use the key \`standup\` in Beacon.", "Starting a task:Check that…", "Waking up:If…", "Finishing a task:Ask…" | "Design handoff: details not recognized (overridable).", "Standup: Two processes use the key `standup` in Beacon." (key in a `code` element, no backticks), "Starting a task: Check that…". No run-together colons in any sheet of the problem processes | `+page.svelte:157` (`{': '}`, backtick split into `<code>`); `present.ts:14` `problemWords`; `ProcessSheet.svelte:78-80` `{' '}` |
| BUG-10 · "Draft used.Shortened to fit…" | **Fixed** (WebKit and Chromium) | "Draft used.Shortened to fit the limit of 120 characters." | "Draft used. Shortened to fit the limit of 120 characters." An unclipped draft still reads "Draft used." | `DraftableText.svelte:219` `{' '}Shortened…` |
| BUG-16 · "(added here)" on inherited additions, and "Agent noor" | **Fixed** | Incident response (added at Engineering) reads "Addition (added here)" on Web, Platform, Atlas, Beacon, Noor, Kai and Juno. Noor's badge reads "Agent noor v1" in the row and the sheet | "(added here)" only at the level that added it: Engineering for Incident response, Atlas for Release checklist, Noor for My notes. Everywhere else it reads "Addition". Badges read "Agent Noor Haddad v1" in the row, the sheet header and the "Why" chain | `+page.svelte:127` (level and scope test); `view.ts:45-71` `nameAgentSources` |
| BUG-17 · long names: the select grows and the page scrolls sideways; the title runs under × | **Fixed** (1440 and 960) | With stress names, the select is 2345 px wide (right edge 2710). The content scrolls 1750 px sideways at 960. The title's text ends at 944 px, under × at 912. The turned-off sheet scrolls 1634 px sideways | The select is 256 px (right edge 928 at 960). The content, page and sheets don't scroll sideways. The heading and subtitle wrap. The title ends at 901 px, 11 px clear of × at 912. The turned-off reason wraps. Also clean in the F2-01b stress set (66 rows, long unbroken process name, long reason) | `+page.svelte:62-72` (`min-w-0`, `[overflow-wrap:anywhere]`, select `min-w-0 max-w-64 truncate`); `ProcessSheet.svelte:32` `pr-10`, `:36`, `:50` |
| BUG-18 · sheet 384 px, Close ring 3 px at 50% | **Partly fixed** | 384 px. Close shows a 3 px box-shadow ring at 50% alpha | **Width: fixed**, 440 px at 1440 and 960 (left edge 1000 and 520). **Ring: not fixed**, see P1 | `ProcessSheet.svelte:29` (width); `:99` (ring) |
| BUG-20 · "last copy from 09:12" without the date | **Fixed** | "Showing the last copy from 09:12." for a copy from 8 Nov, seen on 10 Nov | 10 Nov, UTC, en-GB: "…from 8 Nov 2026 at 09:12." Same day: "…from 09:12." Los Angeles, en-US: "…from Nov 8, 2026 at 01:12." Tokyo, where the copy is the previous local day though the same UTC day: "…from 8 Nov 2026 at 18:12." The date follows the local calendar day | `present.ts:17-21` `copyTime`; `+page.svelte:88` |

Screenshots, before and after:

- BUG-14: `bug14-960-dark-team-web.png`.
- BUG-15: `bug15-team-design-invalid.png`, `bug15-project-beacon-clash.png`, `bug15-clash-line.png` and `bug15-sheet-moments.png`.
- BUG-10: `bug10-webkit-clipped.png`, `bug10-webkit-line.png` and `bug10-chromium-line.png`.
- BUG-16: `bug16-team-platform.png`, `bug16-agent-noor.png` and `bug16-agent-noor-sheet.png`.
- BUG-17: `bug17-960x640-{organization,team-web,project-atl,agent-noor}.png`, `bug17-960x640-sheet-long-title.png`, `bug17-960x640-turned-off-sheet.png` and the 1440 set.
- BUG-18: `bug18-{webkit,chromium}-{1440,960}-close-focus.png` and `-close-zoom.png`.
- BUG-20: `bug20-{other-day-utc,same-day-utc,other-day-us,same-utc-day-but-next-local-day-tokyo}.png`.

## Problems

### P1 · S4 · BUG-18 is partly fixed: the Close outline is never painted (in e27153d, not new)

- **Repro:** open Processes, focus a row and press Enter. Focus lands on Close in the sheet. Look at Close.
- **Expected:**
  - `--ring` as a 2 px outline with a 2 px offset. The design system's token table says: `--ring` … "Focus ring, 2 px plus a 2 px offset". BUG-18 asks for the same.
- **Actual:**
  - The computed style is `outline: none 2px oklch(0.585 0.233 277.117) offset 2px`, in WebKit and Chromium.
  - The width, offset and colour are right, but `outline-style` is `none`, so the outline isn't drawn.
  - The box-shadow ring is turned off (`focus-visible:ring-0`).
  - What you see is the shared button's `focus-visible:border-ring`: a 1 px border at the button's edge, with no offset.
- **Pixel proof:**
  - With transitions off and that border made transparent, the focused button shows no indicator at all (`bugs-C/bug18-close-focus-4x-border-hidden.png`).
  - `bug18-close-focus-4x.png` is the real state.
  - `bug18-close-focus-4x-if-outline-solid.png` shows what the class list intended.
  - Avery's own capture shows the same 1 px edge (`process-bugs-e27153d/…BUG-795fe…/processes-bug18-close-focus-context.png`).
- **Cause:**
  - `components/ui/button/button.svelte:7` has `outline-none`, which sets `--tw-outline-style: none`.
  - `ProcessSheet.svelte:99` adds `focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring`. Tailwind v4's `outline-2` paints with `outline-style: var(--tw-outline-style)`, which is still `none`.
  - The new e2e test (`processes-bugs.spec.ts:177-181`) checks the width, offset and colour, not the style, so it passes.
- **Fix:**
  - Add `focus-visible:outline-solid`, or `focus-visible:border-transparent` with an explicit style.
  - Assert `outlineStyle === 'solid'` in the test.
- It's still a visible 1 px indicator in the right colour, so S4: a token mismatch, not a missing focus indicator.

### P2 · S4 · pre-existing (in B and C) · Report a bug: the "Shortened to fit" line stays true-looking after the title is edited past the limit

- **Repro:**
  1. Report a bug › Draft or improve Title, with a 400-character instruction.
  2. Use this draft. The Title is 120 characters, and the line reads "Draft used. Shortened to fit the limit of 120 characters."
  3. Type " edited" at the end.
- **Actual:**
  - The Title is 127 characters (`maxlength` is 240), and the line still says it was shortened to fit 120.
  - The limit is enforced only on submit: "The title can be at most 120 characters."
- **Expected:**
  - The field's limit is visible while typing (for example a counter, or a 120 `maxlength`), and the draft note doesn't claim a fit that no longer holds (design system: errors say what happened and what to do).
- This is the same "refused only on submit" class as BUG-36 (agent names), but for the bug title. BUG-10's fix doesn't cause it, and it renders the same in B.
- **Evidence:**
  - `blitz-B.json` and `blitz-C.json`, `report-*-after-edit`.
  - `bugs-C/report-title-130-submit.png`.
- **Code:** `DraftableText.svelte:217-223`.

Nothing else is new. I checked and dropped one apparent issue. The "Draft or improve" button seemed to have no focus ring, but my probe had read the style during the 150 ms `transition-all`. After 600 ms the 3 px ring is there, in both builds.

**Note on the clash line:** the spec's clash copy is a sentence on its own ("Two processes use the key `<key>` in <scope>. …", PROC-1). The screen prefixes the process name ("Standup: Two processes…"), so after the colon the capital T stays. "Design handoff: details…" is lowercase, because PROC-16 writes it that way. Both now follow their spec lines, so this isn't a finding.

## Pixel comparison (B against C)

**76 states × 4 runs** (1440×900 and 960×640, OS dark and light, WebKit) gives **304 pairs. 169 are byte-identical and 135 differ.** Each differing pair comes from a fix:

| States | Pairs | Why they differ |
| --- | --- | --- |
| Every open sheet: p03, p04, p06, p07, p08, p12, p13, p18, p19, p24, p44, p45, p47 | 52 | The sheet is 440 px, not 384 (BUG-18), with its own Close button. p03 and p04 also have the guidance-moment spacing (BUG-15), and p24 has "Agent Noor Haddad" (BUG-16). The 960 bottom-scrolled sheet is shorter (scrollTop 406 → 334), because the wider sheet wraps less |
| 960×640 only: p02, p05, p09, p10, p16, p26, p28, p33, p36, p37, p40–p43, p46, `28-processes` | 32 | Show now sits beside the heading instead of wrapping under it, and the subtitle wraps to two lines. This is from BUG-17's `min-w-0 flex-1` on the heading block. The same pairs are identical at 1440. The list moves up 12 px |
| p11, p14, p17, p25 | 16 | "Addition (added here)" → "Addition" (BUG-16). At 960, p11 is the BUG-14 fix, and the rows rewrap with `min-w-48` |
| p15 | 4 | "Design handoff: details…" (BUG-15) |
| p20, p35 | 8 | The clash lines (BUG-15) |
| p23, p27 | 8 | Agent badges and "(added here)" (BUG-16) |
| p31, p32, p38 | 12 | "…from Nov 8, 2026 at 09:12." (BUG-20) |
| `20-chat` | 3 | "Waiting 8 min" against "Waiting 9 min": the run timing, as in earlier validations |

- **Behaviour per step** (URL, headings, alerts, focus, overflow, controls, landmarks): the same in all 304 steps. The exceptions are:
  - the text changes above;
  - the sheet Close focus style;
  - the 960 scroll offsets that follow the shorter header.
- **Main screen set:** 31 states, 124 pairs. 119 are identical. The 3 Chat timing pairs and the 2 Processes pairs at 960 are explained above.
- **Diff images:** `validation-processes-batch-shots/diff/<run>/`.

## Blitz: Processes and Report a bug (both builds)

- **Stress data** (F2-01b set: 66 rows, a long unbroken name, a long spaced name, a long reason, and "Web" plus 204 characters):
  - B scrolls sideways in every view: `main` is 1747 px wide at 960 and 1440, and the select is 1382 px wide.
  - C doesn't scroll sideways anywhere, and the select is 256 px.
  - No errors in either build.
- **Rapid clicks:** 9 fast row clicks and 8 fast place changes: no stacked dialogs, and the URL, heading and select agree (`project:onb`). Same in both builds.
- **History and reload:**
  - Back leaves Processes. Forward restores Atlas.
  - Reload with the sheet open keeps the place and closes the sheet.
  - Reload shows the skeleton. Leaving during the load gives no error.
  - Same in both builds.
- **Resize:** with the sheet open, going from 1440 to 960 and back gives no overflow. The sheet is 440 × 640 at 960 (it was 384).
- **Keyboard** (Chromium Tab, WebKit Option+Tab):
  - The order is unchanged: the nav, then Show, then the rows.
  - Enter opens the sheet with focus on Close, and focus stays in the sheet.
  - Escape returns focus to the row.
  - Close's indicator: P1.
- **Accessibility:**
  - The sheet is `dialog "Code review (Web)"`, `aria-modal`.
  - The clash key is now a `code` element in the accessibility tree.
  - The lowest text contrast is 4.86:1 (gray-400 at 12 px), the same as before.
- **Report a bug (BUG-10), 1440 and 960:**
  - 5 rapid Draft clicks give 1 proposal.
  - The draft line is 528 px wide, with no overflow.
  - Title → Tab reaches Undo, with a 2 px outline.
  - Undo clears the title and the line.
  - P2 is the only issue.

## Local checks in C (e27153d)

- `bun run check` (heavy): `COMPLETED 1156 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
- `bun run test` (heavy): `Test Files  49 passed (49)` / `Tests  1093 passed (1093)`
- `bun run test:e2e --workers=2` (heavy, `PW_PORT=4283`): `238 passed (7.4m)` (exit 0)

## Method

- **Worktrees:** `git worktree add --detach` at `5068a6a` (A), `07b9b9d` (B) and `e27153d` (C) under the session scratchpad `f1-03/`. All are removed now.
- **Dev servers:** B and C ran `bun run dev --mode e2e` on 5234 and 5235, the test-owned fake composition F1-03 introduced. Ordinary `bun run dev` shows "unavailable" since F1-03.
- **Scripts** (in `validation-processes-batch-shots/`):
  - `validate.mjs`: adapted from the office-order and F2-01b scripts, with builds and flows from the environment. Here: `FLOWS=process,main`, `BUILDS=B=…,C=…`.
  - `bugs.mjs`: one probe per bug, with stress names substituted in the served `sample.ts`/`world.ts` through `context.route`, the same technique Avery's spec uses.
  - `blitz.mjs`: the F2-01b blitz plus a Report a bug section.
- **Clock and capture:** the clock was installed at 2026-11-10T12:00Z, and each flow ran in a fresh WebKit context. Every step also has a `.norm.png` with all scrollers at the top.
- **Harness note:** in the C default-dark run, `p01-org-loading` logged one "Importing a module script failed" Vite reload. That was my own `check` run touching `.svelte-kit` in the same worktree. The step passed, and its pair is byte-identical.
