# Validation: F2-01b, processes moves to lib/modules/processes

**Verdict: No behaviour change found. 06efbae introduces no problems, and nothing blocks the app from loading. Everything listed below already exists in 017a286.**

- Change: merge `06efbae` ("Merge avery/f2-01-processes into v1") on `v1`.
- Compared with its first parent `017a286` ("Merge eden/specs-19 into v1 …").
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. The two throwaway worktrees are removed.

## Summary

- **Source check** (`git diff -M 017a286 06efbae`, 21 files):
  - The 13 files under `src/lib/processes/` are 100% renames to `src/lib/modules/processes/`.
  - `ProcessBadges.svelte` moved to `modules/processes/components/`, and `ProcessSheet.svelte` to `modules/processes/features/inspect-effective/`. Only their import lines changed.
  - `routes/(shell)/processes/+page.svelte` changed its 7 import lines and nothing else.
  - The test-only changes are the shared `e2e/shot.ts` and the new `e2e/shot.spec.ts`. `copilot.spec.ts` now uses the shared helper. `processes.spec.ts` adds 5 captures and pauses the clock for the loading capture.
  - `TECH-STACK.md` gains a "Screenshot evidence" section.
  - No `$lib/processes` or `$lib/components/Process*` import remains anywhere.
- **Pixel comparison:** 76 states × 4 runs (1440×900 and 960×640, OS dark and light, WebKit) gives **304 pairs. 301 are byte-identical PNGs.**
  - **All 180 processes pairs are byte-identical.**
  - The 3 pairs that differ are on screens this merge didn't touch. They are timing noise (see "Pairs that differ").
- **Behaviour:** every scripted step succeeded in both builds. The URL, title, headings, alerts, focused element, focus-visible style, scroll offsets, control counts and full visible text match in every processes state. The only text difference in any state is Chat's waiting time.
- **Console:** the same in both builds. The only messages come from the deliberate 404 page (`31-not-found`).
- **Blitz and a11y probes:** I ran them on both builds and they gave identical results: `blitz-after.json` = `blitz-before.json` key for key, and `a11y-after.json` = `a11y-before.json` for 36 of 36 states.
- **Checks in the after build:**
  - `bun run check`: `COMPLETED 1168 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
  - `bun run test`: `Tests  1087 passed (1087)`
  - **E2E, final line: `224 passed (3.9m)`.** All 12 processes tests and both new `shot.spec.ts` tests passed. OFC-11 (BUG-5) passed this run.
- **The new screenshot helper works:** `processes-loading.png` captures the skeleton (the clock is paused), and `processes-why-context.png` captures the sheet region. Copies are in `validation-f2-01b-shots/e2e-after-named/`.

The app is dark only, as in F1-01, so the light runs render the same as the dark runs.

## Findings introduced by 06efbae

None.

## Pre-existing findings (present in 017a286, unchanged by this merge, not yet catalogued)

The source is identical apart from the import lines. Each of these renders byte-identically in both builds.

1. **S3 · Layout · Processes › Web (team) at 960×640: the Code review row's name is squeezed to 23 px wide, one or two letters per line.**
   - Repro: open Processes and choose Show › Web at a 960×640 window.
   - Expected: every row is readable at the minimum window, 960×640 (design system, Sizes; PROC-9: the inheritance view shows each process with its source, version, kind and drift).
   - Actual: the badge cluster ("Team Web v2 · Override of code-review@1 · Parent changed: Organization is now v2") takes its full width. The name column (`min-w-0 flex-1`) collapses to 23 px, so it reads "Co / de / re / vie / w". It is the only squeezed row in 36 probed states (`a11y-after.json`, `squeezed`).
   - Fix: give the name column a minimum width (for example `min-w-48` or `basis-60`), so the badges wrap under the name instead.
   - Screenshots: `after/960x640-dark/p11-team-web-drift.png` and `p12-team-web-sheet-code-review-why.png`. They are identical in `before/`.

2. **S4 · Copy · "Needs attention" and the sheet's prompts are missing the space after the colon.**
   - Actual: "Design handoff:Details not recognized (overridable).", "Standup:Two processes use the key…", and in the sheet "Starting a task:Check that…".
   - Expected (spec §2 and PROC-16): "Design handoff: details not recognized (overridable).", with a space and a lowercase "details".
   - Cause: Svelte drops the trailing space before `{/if}` in `+page.svelte` (the problems list) and inside `<dt>` in `ProcessSheet.svelte`. This is the same class as BUG-10.
   - The clash text also shows literal backticks (`` `standup` ``), where the spec means code formatting.
   - Screenshots: `after/default-dark/p15-team-design-invalid.png`, `p20-project-beacon-conflict-clash.png` and `p03-sheet-branch-and-merge.png`.

3. **S4 · Copy · "Addition (added here)" on a process that was added somewhere else.**
   - Actual: Team Engineering's Incident response shows "(added here)" on the Platform, Web, Atlas, Beacon and agent views.
   - Expected: the spec example shows "(added here)" only at the level that added it, and PROC-9 asks for the source level.
   - Also, an agent's own badge reads "Agent noor v1" (its id), where team badges use names ("Team Engineering"). Expected: "Agent Noor Haddad v1".
   - Screenshots: `after/default-dark/p14-team-platform-sealed-refusal.png` and `p23-agent-noor-reads.png`.

4. **S4 · Layout (stress) · A long team, project or agent name makes the content scroll sideways.**
   - Repro: data injected in flight, without touching the repo (see Method).
   - Actual: the Show select is as wide as its longest option, so one long name widens it on every view: content `scrollWidth` 1747 against 960, and against 1440. With an unbroken name, the heading and subtitle overflow too, and a long process title runs under the sheet's × button.
   - Expected: no horizontal scroll and no clipping at 960×640 (design system).
   - Fix: cap the select (`max-w-64` with truncating options), put `[overflow-wrap:anywhere]` on the heading and subtitle, and pad the sheet title clear of the close button.
   - Screenshots: `blitz-after/stress-960x640-team-web-long-name.png`, `stress-960x640-org-top.png` and `stress-960x640-sheet-long-unbroken.png`.

5. **S4 · Visual and token · The sheet doesn't match the design system.**
   - The sheet is 384 px wide: `max-w-xl` loses to the shadcn default `sm:max-w-sm`. The design system's side panel is 440 px.
   - Its Close button's focus ring is a 3 px ring at 50% alpha, where the design system specifies `--ring` at 2 px plus a 2 px offset. This is the shared shadcn Sheet.
   - Evidence: `blitz-after.json` `resize-960-sheet` and `kbd-*-sheet-sequence`, and `blitz-after/kbd-webkit-sheet-focus.png`.

6. **S4 · A11y · The shell has no `main` landmark.** The only landmark is `nav "Main"` (`a11y-after.json`, `landmarks`). This is shell-wide, not specific to Processes.

7. **S4 · UX · The unreachable-handbook state gives only a time.** "Showing the last copy from 09:12" is for a copy from 2026-11-08, two days earlier. Expected: include the date when the copy isn't from today (PROC-16 says "<time>"). Screenshot: `after/default-dark/p31-handbook-down.png`.

8. **Requirement gaps (pre-existing, scope for ada):** the spec's actions aren't built. These are New process, Use the review-and-merge template, Review (diff, Keep my override, Take the new version, Edit), Fix, Rename…, Ask for approval… and Have an agent draft it…, Allow once…, and "Waiting to merge". There are also no Processes tabs on a team, a project's Docs or an agent's profile; the agent's view is under Processes › Show. See the requirements table.

**Already known and not re-reported:** BUG-5, BUG-2, and BUG-6 to BUG-12.

## Pairs that differ (3 of 304)

| Pair | Pixels | Why it is not the merge |
|---|---|---|
| `default-dark/16-project-new-task-dialog` | 12 px (the same after scroll normalising) | Antialiasing at the corner of the Title input's focus ring and under Cancel, caught mid-transition. The text, focus and controls are identical. The New task dialog is untouched by the merge, and the other 3 runs are byte-identical. Diff image: `diff/default-dark/16-project-new-task-dialog.png`. |
| `960x640-dark/20-chat` and `960x640-light/20-chat` | 751 px each | "Waiting 8 min · blocks release" against "9 min", and "11 min" against "12 min". A wall-clock minute ticked over, as in F2-01a. |

## Requirements: docs/specs/processes-ux.md in the after build

"Met" means met in the browser UI on the TypeScript mock (Sample data). Every row is the same in 017a286.

| ID | Status | Evidence |
|---|---|---|
| PROC-1 | Partly | Both clash files show "Two processes use the key `standup` in Beacon. Neither applies until one is renamed." (p20, p35 copied). **Rename…** is missing, and the backticks show literally. |
| PROC-2 | Met (UI) | The effective sets per place follow the levels. Noor on Beacon gets Organization, Engineering, and her private process (p23). Sam, with no current work, gets the organization's and his own (p26). |
| PROC-3 | Partly | "Platform's version of Security basics isn't applied: the organization sealed it. You can add a process instead." (p14, p17). The team-seal conflict on a shared project isn't in the sample, so I couldn't exercise it. |
| PROC-4 | Partly | "Turned off for Atlas: we report daily" (p17, p18). Onboarding shows "On · turning it off needs approval by someone other than its author (hana)" (p21). **Ask for approval…** and **Have an agent draft it…** are missing. |
| PROC-5 | Partly | Noor's view says "Private processes only add: My code review isn't applied because it changes an inherited process." (p23). The write-time refusal is CLI, not in this build. |
| PROC-6 | Partly | Beacon shows "Code review: Design and Web disagree. The organization's version applies until the project decides." (p20). There's no Needs you or inbox item, and no way to approve the project's version. |
| PROC-7 | Not in UI | Agent runtime delivery. Not in this build. |
| PROC-8 | Not in UI | CLI. The app's sheet does show the "why" chain: layer, outcome and note (p12, p18). |
| PROC-9 | Partly | The source level, version, kind and "Parent changed: Organization is now v2" all show (p11). The **Review** diff is missing. At 960×640 the row is unreadable (finding 1). |
| PROC-10 | Partly | "Read v2 at 09:12 · now v3" and "Not read yet" (p23 to p27). Sam shows "No current work. Showing the organization's processes and its own. Team and project processes apply when it starts a task." (p26). This is under Processes › Show, not on the agent's profile tab. |
| PROC-11 | Not met | No approval UI. |
| PROC-12 | Partly | The sheet lists "Prompts at guidance moments" and "Checklist steps it adds to a task" (p03, p19). Feeding them into tasks is not in the UI. |
| PROC-13 | Not met | No retro link. |
| PROC-14 | Partly | "One open branch per agent: On" (p17), also with the process missing (p34) and when self-approved (p21). There's no refusal and no **Allow once…**. |
| PROC-15 | Not met | No branch or stage in the work state. |
| PROC-16 | Partly | Skeleton rows (p01, p30). "Can't reach the handbook. Showing the last copy from 09:12." (p31, p32). The invalid file's copy differs (finding 2), and **Fix** is missing. |
| PROC-17 | Not in UI | Agent runtime. |
| PROC-18 | Partly | Levels and kinds are a word plus an icon. Drift is the text "Parent changed: …", inside the row button's name. There's no diff, so no "Changes only" view. |
| PROC-19 | Not met | No docs or handbook branches in the work state. |
| PROC-20 to PROC-26 | Not met | No autonomy level, Settings › Autonomy, critical list or escalations anywhere in `apps/org-ui`. |
| PROC-27 to PROC-30 | Not in UI | Runtime and agent guidance delivery. |

## Blitz: owner jobs and attempts to break it (after build; the same in before)

- **Owner jobs:** see what the organization follows; check why Web's code review differs; see what Atlas turned off and why; check what Noor read; find what needs fixing. Every job is reachable in one or two clicks.
- **Empty data:** `?processes=empty` gives "No processes yet / Write down how your agents work…" (p33). With the handbook also down, the warning and the empty state both show (p38). On a project with no processes, the rule line stays on (p34). There's no template action (gap 8).
- **Many items and long text:** 66 rows render with no errors and no page-level overflow. Long spaced names wrap well. Finding 4 covers long single names and the select.
- **Rapid clicks:**
  - 9 fast clicks on a row: no errors, and no stacked dialogs.
  - 8 fast place changes: the final URL, heading and select agree (`project:onb`).
- **Back and forward:**
  - Place changes use `replaceState`, so Back from any place leaves Processes for the previous screen (`/agents`).
  - Forward restores the last place, and Back with the sheet open leaves the page.
  - This is consistent behaviour. Say if place changes should be history entries.
- **Reload mid-flow:**
  - With the sheet open, reload keeps the place (Atlas) and closes the sheet.
  - Reload shows the skeleton again.
  - Reload on an agent view keeps it.
  - Leaving during the 250 ms load gives no error.
- **Garbage and stale places:**
  - `?place=team:zzz` says "That place isn't here any more. Showing the organization's processes." (p36).
  - `?place=%<b>x` falls back silently to the organization (p37).
- **Resizing:** I opened the sheet at 1440 and resized to 960×640 and back. There was no overflow, and the sheet stays 384 px.

## UX, IA, visual and a11y

- **Keyboard path** (Chromium Tab; WebKit Option+Tab):
  - The order is the nav links, Switch organization, Show select, then each process row, top to bottom.
  - Each stop shows a 2 px indigo outline.
  - Enter or Space on a row opens the sheet with focus on Close, and focus stays in the sheet.
  - Escape returns focus to the same row (p42 to p47, `blitz-after/kbd-*`).
  - The sheet's only tab stop is Close. The text is in a scrollable `pre`, which WebKit can't scroll by keyboard, though the sheet itself scrolls.
- **Contrast:** the lowest text pair in all 36 states is 4.84:1 (`gray-400` 12 px meta text on a row), which passes AA. The amber and red notices are higher.
- **Accessible names:**
  - The select is "Show processes for".
  - Rows are buttons named by their whole row text, including the drift text.
  - The sheet is `dialog "Code review (Web)"`, with `aria-modal`, a heading, a term and definition list, and regions for "Why it reads this way" and "The process".
  - The skeleton list is labelled "Loading processes", with `aria-busy`.
- **960×640:** with the sample data, no state scrolls horizontally (content `scrollWidth` equals `clientWidth` in all 18 views). The one layout break is finding 1.
- **Tokens:** Processes uses raw Tailwind gray, amber and red classes rather than the design system's status tokens. The app as a whole is the same (dark only), as in F1-01.

## What I covered

- **Processes, 45 states in both builds:**
  - Loading, and the organization list.
  - The sheet for 4 organization processes, scrolled and closed.
  - Every place: 5 teams, 4 projects and 4 agents, chosen with the Show select.
  - Sheets for drift and why, an addition, turned off, a checklist, and private.
  - Every mock scenario: `?handbook=down` (loading, organization, team), `?processes=empty` (organization, project, with the handbook down), `?processes=copied`, and missing and garbage `?place`.
  - A keyboard path of 8 steps.
- **The F1-01 main set:** 31 states.
- **Blitz:** stress data, rapid clicks, history, reload, resize, keyboard in two engines, ARIA and contrast. I ran it in both builds.
- **Checks in the after build:** check, unit tests and e2e.

## Method

- The scripts are in `validation-f2-01b-shots/`:
  - `validate.mjs` is adapted from F2-01a's script, with processes flows in place of copilot.
  - `blitz.mjs` and `a11y.mjs` run with `node <script> <before|after>`.
- Each build ran `bun run dev` from its worktree, on ports 5193 and 5194.
- Each flow ran in a fresh WebKit context with the clock installed at 2026-11-10T12:00Z. It waited for finite animations only, then took screenshots with `animations: 'disabled'`. Each step also has a `.norm.png` with every scroller at the top.
- The stress data was injected by rewriting the served `sample.ts` module through `context.route`. The repo was not touched.
- My overflow check in `validate.mjs` looked inside `main`, and the shell has none (finding 6). So the 960×640 overflow numbers come from `a11y.mjs`, which uses the content scroller.

## Files

- Report: `workspace/validation-f2-01b.md`
- Screenshots and data: `workspace/validation-f2-01b-shots/`
  - `{before,after}/<run>/<state>.png` and `.norm.png`, where `<run>` is `default-dark`, `default-light`, `960x640-dark` or `960x640-light`
  - `diff/<run>/` (the 3 differing pairs)
  - `blitz-{before,after}/` and `blitz-{before,after}.json`
  - `a11y-{before,after}.json`
  - `e2e-after-named/` (the suite's processes captures)
  - `summary.json` and `results.json`
  - Logs: `validate-run1.log`, `e2e-after.log`, `check-after.log`, `unit-after.log`, `blitz-*.log`
  - Scripts: `validate.mjs`, `blitz.mjs`, `a11y.mjs`
