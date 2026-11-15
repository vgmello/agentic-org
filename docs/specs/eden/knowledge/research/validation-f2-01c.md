# Validation · F2-01c employees move (v1 ad305cd vs 705158f)

**Verdict:** no behaviour change and no new problems. The 5 employee components are 100% renames into `modules/employees/features/*`, with a new `index.ts`. Otherwise the merge changes import lines, tests and one docs path.
- **Pixel compare:** 352 of 356 pairs are byte-identical, and 230 of the 232 employee pairs. The other 4: 2 scroll offsets that are 0 px once normalised, and 2 clock-minute changes in Chat. My e2e run reproduces 26 of Avery's 28 shots byte for byte; the other 2 differ by 1 to 2 px, as they already do between Avery's own commits.
- **Local checks:** check `COMPLETED 1169 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`, unit `Tests  1087 passed (1087)`, e2e `227 passed (4.9m)`.

**Pre-existing issues, the same in both builds, sent to ada 10 Nov.** How I checked each:
1. S3. Double-clicking "Pause <name>" pauses the agent, then the second click opens Add agent behind the dialog. Seen in `blitz-after/double-click-pause-confirm-opens-webkit.png` (the Add an agent sheet opens over the "Paused Noor Haddad" toast).
2. S3. How it runs › Scheduled at 960×640: the dialog is taller than the window (666 px) and can't scroll; its top edge is cut off. Seen in `after/960x640-dark/e72`.
3. S3. Agents roster at 960×640: the table overflows, Reports to is cut, words break mid-word ("Peopl/e", "Desig/n"), and the status icon shrinks. Seen in `after/960x640-dark/e01-agents-roster.png`.
4. S3. A long name pushes the Pause confirm button out of the dialog, and the title runs under the ×. Seen in `blitz-after/long-spaced-960-pause.png`. The same happens in Offboard.
5. S3 requirement. A Scheduled agent shows "Status unknown" (seen in `e76`); RUN-2 says Asleep with "next run <time>", and Offline for Always on.
6. S3 a11y. Focus drops to the page body after confirming Pause, Resume or Offboard. The dialogs have no focus restore (`PauseAgentDialog.svelte`).
7. S3 a11y. The How it runs radios are bare `<input type="radio" class="mt-1">` with no focus-visible style, and WebKit shows no ring when arrowing through them (`HowItRunsDialog.svelte:91-102`).
8. S3 visual. In WebKit the selects are about 20 px high next to 36 px inputs. Seen in the Add an agent sheet (Runs on, How it runs).
9. S3 contrast (the run's measurements). The Offboard button is 3.86:1, the roster filter counts 3.58:1, and the sheet's error text 4.34:1; all should be at least 4.5:1.
10. S3 UX. Disabled Add and Save give no reason. A name over 64 characters is refused only on submit, and the message doesn't name the field.
11. S3 requirement (RUN-7). The When field is free text, with no presets and no next 3 run times. It accepts cron (`* * * * *`) and an invalid time zone; the dialog has no validation code.
12. S4, follow-ups:
    - "No left agents." should say "No former agents."; the filter's id `left` leaks into the copy (`agents/+page.svelte:28-34`).
    - "Runs on Runtime not set" has no Set runtime action (PROF-1).
    - Duplicate display names are allowed.
    - The home-folder copy is inconsistent.
    - Errors give no next step.
    - The Offboard copy needs work.
    - Slow load shows a blank page, and `?org=fails` shows a 500 page with no Retry.
    - The strip is a `role=status` region holding buttons.
    - `?member=` doesn't follow the selection.
    - The toast covers the sheet's last fields.

**What I checked myself, item by item (second pass, 10 Nov):**
- **#9 contrast**, computed from the Tailwind colours:
  - the Offboard button, `text-destructive` on `bg-destructive/20` over gray-800: **3.85:1**
  - the sheet's error, the destructive Alert description at red-400/90 on the card: **4.34:1**
  - the filter counts, gray-400 on the selected tab (white/10 over gray-800): **4.16:1** (the run measured 3.58:1 on its tab state)
  All three are below 4.5:1, so #9 is confirmed.
- **#10:** confirmed. `AddAgentSheet.svelte:236` disables Add agent on `missing` with no visible reason.
- **S4 items I confirmed in the code:**
  - "No {filter} agents." with the id `left` (`agents/+page.svelte:95`)
  - `runtimeLabel` returns "Runtime not set", so the quick view reads "Runs on Runtime not set"
  - there's no duplicate-name check in the employees module
  - the error page has only "Back to the start" (`+error.svelte`)
  - `SelectedMember.svelte:59` is a `role="status"` region holding the action buttons
- **The remaining S4 items, all now confirmed (third pass):**
  - **Home-folder copy:** the hints differ. Add says "an absolute path on {officeName}" and Edit says "an absolute path on this Mac". I had first written "the labels match", which was wrong.
  - **Errors with no next step:** the API message is shown verbatim, for example "The office went away." (`org/api.ts:130`), with nothing to do next.
  - **Offboard copy:** `org/agents.ts:93` always says "Any open tasks <name> has come back unassigned…", with no count and no line about reports. DEAL-4 says it lists only the consequences that apply.
  - **Slow load:** `after/default-dark/e24-agents-loading.png` is an empty page, with no skeleton.
  - **`?member=`:** `org-chart/+page.svelte:85` reads it once and never updates it when the selection changes.
  - **The toast:** in `blitz-after/kbd-webkit-add-focus.png`, "Resumed Noor Haddad." covers the Reports to field of the Add an agent sheet.

**Requirements** (from the run): met: RUN-6, STAFF-2 (UI). Partly met: DEAL-1, 3, 4, 5, PROF-1, PROF-4, RUN-1, 2, 10, 13. Not met: STAFF-4 (no proposal or approval yet), RUN-7, RUN-11, EMP-13, ROLE-9, HRL-2.

Screenshots: `workspace/validation-f2-01c-shots/` (not committed). The full working report follows.

---

# Validation: F2-01c, the employees module move with an explicit entry point

**Verdict: no behaviour change found. ad305cd introduces no problems. Every finding below already exists in 705158f.**

- Change: merge `ad305cd` ("Merge avery/f2-01-employees into v1") on `v1`.
- Compared with its first parent, `705158f` ("Merge avery/bug-5-office-status into v1"). I confirmed it with `git log -1 --format=%P ad305cd`, which gives `705158f… 1fdfa17…`.
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. Both throwaway worktrees are removed, and the dev servers are stopped.

## Summary

- **Source check** (`git diff -M 705158f ad305cd`, 12 files):
  - Five components are 100% renames from `src/lib/components/` into `src/lib/modules/employees/features/`:
    - `AddAgentSheet` to `hire-employee/`
    - `EditAgentDialog` to `edit-profile/`
    - `HowItRunsDialog` to `configure-run/`
    - `OffboardAgentDialog` to `offboard-employee/`
    - `PauseAgentDialog` to `pause-employee/`
  - The new `modules/employees/index.ts` re-exports the five.
  - `routes/(shell)/agents/+page.svelte` changed 1 import line, and `org-chart/+page.svelte` changed 5 import lines into 1. No markup or logic changed.
  - The test-only changes are in `agents-roster.spec.ts`, `org-chart-agents.spec.ts` and `shot.spec.ts`. They add the named captures, the failure and retry steps (`__mockOrgFailNext`) and a finite-transition test.
  - There's one docs path fix, in `standalone-server-removal-inventory.md`.
  - No import of the old paths remains anywhere.
- **Pixel comparison:** I captured 89 states in 4 runs (1440×900 and 960×640, OS dark and light, WebKit). That gives **356 pairs, and 352 are byte-identical PNGs.**
  - 230 of the 232 employee pairs are byte-identical.
  - The 4 pairs that differ are scroll and clock noise (see "Pairs that differ").
- **Behaviour:** every scripted step succeeded in both builds. The URL, title, headings, alerts, focused element, focus-visible style, control counts and full visible text match in every employee state. The console is the same in both builds: the only messages come from the deliberate 404 (`31-not-found`).
- **Blitz and a11y probes:** I ran them in both builds and they gave the same results.
  - `a11y-after.json` matches `a11y-before.json` in all 22 states, and the dialog trees match too.
  - `blitz-after.json` matches `blitz-before.json` key for key, apart from timing (ms counts, focus rings caught mid-transition) and one race that I re-ran 5 times per build, with the same outcome in both (finding 1).
- **Avery's 28 named screenshots** (`app-avery/apps/org-ui/playwright-report/employees-ad305cd/`): my e2e run of ad305cd reproduces **26 byte for byte**. The other two are full-page `edit-agent.png` (2 px) and `edit-agent-error.png` (1 px); both `-context` crops of those two are byte-identical. Avery's own sets for 1fdfa17, 7fc4c9f and ad305cd also differ in those files, so they are antialiasing noise.
- **Checks in the after build:**
  - `bun run check`: `COMPLETED 1169 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
  - `bun run test`: `Tests  1087 passed (1087)`
  - **E2E, final line: `227 passed (4.9m)`**, with none flaky.

The app is dark only, as in F1-01, so the light runs render the same as the dark runs.

## Findings introduced by ad305cd

None.

## Pre-existing findings (present in 705158f, unchanged by this merge, not yet catalogued)

The moved code is identical apart from the import lines. Every screenshot below renders byte-identically in both builds, and every blitz value below is the same in both builds.

1. **S3 · Interaction · Double-clicking "Pause <name>" opens the Add an agent sheet.**
   - Repro: open the org chart, select Noor Haddad, choose **Pause…**, then double-click **Pause Noor Haddad** with about 200 ms between the clicks.
   - Expected: a confirmation acts once, and nothing else opens (design system, Confirmations; DEAL-3 calls Pause one action).
   - Actual: the first click pauses Noor and starts closing the dialog. The second click goes through the closing dialog to the toolbar's **Add agent**, which sits underneath, and opens the sheet.
   - It happened every time with `mouse.click` twice 200 ms apart, in WebKit and Chromium, in both builds. With 80 ms between the clicks it didn't happen. It happened in 2 of 5 forced-click runs per build. Offboard isn't affected, because nothing sits under its button.
   - Fix: keep the overlay catching clicks until the close animation ends, or ignore clicks while it closes.
   - Screenshots: `blitz-after/double-click-pause-confirm-opens-webkit.png` and `…-chromium.png`.

2. **S3 · Layout · At 960×640, How it runs with Scheduled is taller than the window and can't scroll.**
   - Actual: the dialog is 666 px tall, from top −13 to bottom 653, with `overflow-y: visible`. The title touches the top edge and Save touches the bottom. The error line makes it taller still.
   - Expected: no clipping at the minimum window, 960×640 (design system, Sizes).
   - Fix: `max-h-[90vh] overflow-y-auto`, as `EditAgentDialog` already has.
   - Related: at 960×640, Edit's Save and Cancel sit below the fold inside its scrolling dialog (66 px more to scroll), so a refusal there is easy to miss.
   - Screenshots: `after/960x640-dark/e72-how-it-runs-scheduled-empty.png`, `e75-how-it-runs-error.png` and `blitz-after/resize-runs-scheduled-960.png`.

3. **S3 · Layout · At 960×640 the Agents roster breaks words, hides a column and squeezes the status icon.**
   - Actual: the table is 627 px wide in a 608 px box, so **Reports to** is cut off behind a sideways scroller.
   - Words break mid-word ("Peopl/e", "Lindqvis/t", "Platfo…"), because of `[overflow-wrap:anywhere]`. The header reads "How / it / runs".
   - The status icon shrinks to 4×14 px (14×14 at 1440), so "Asleep" loses its moon.
   - Expected: no horizontal scroll or clipping at 960×640 (design system). Status is always words plus an icon (spec §10).
   - Fix: `shrink-0` on the icon, `break-words` instead of `anywhere`, and drop or merge a column (Now, Office) below about 1100 px.
   - Screenshots: `after/960x640-dark/e01-agents-roster.png` and `blitz-after/long-unbroken-960-roster.png`.

4. **S3 · Layout (stress) · A long but valid name (up to 64 characters) breaks the Pause and Offboard dialogs.**
   - Actual: at 960×640, "Pause <name>" and "Offboard <name>" don't wrap (the Button's `whitespace-nowrap` beats `[overflow-wrap:anywhere]`). The button runs out of the dialog, which then scrolls sideways by 141 to 165 px. The title also runs under the × button.
   - Expected: no clipping. The primary button repeats the verb (design system), so it must wrap or truncate with the full name in its accessible name.
   - Screenshots: `blitz-after/long-spaced-960-pause.png`, `long-spaced-960-offboard.png` and `long-unbroken-960-pause.png`.

5. **S3 · Requirements · Scheduled and Always-on agents show "Status unknown".**
   - Repro: How it runs › Scheduled › "weekdays at 9" › Save.
   - Actual: the quick view and the roster say "Status unknown". Mira, set to Always on, also says "Status unknown".
   - Expected (RUN-2): Scheduled shows "Asleep · next run <time>", and Always on with no session shows "Offline".
   - Screenshot: `after/default-dark/e76-how-it-runs-saved.png`. Evidence: `blitz-after.json` keys `job-scheduled-status` and `req-roster-rows`.

6. **S3 · A11y · Focus drops to the page after confirming Pause, Resume and Offboard.**
   - Actual: `document.activeElement` is `body` after a keyboard confirm, in WebKit and Chromium. Cancel and Escape do return focus to the trigger.
   - Expected: focus stays somewhere useful (WCAG 2.4.3). For example, Resume after Pause, Pause… after Resume, and the team card or the chart heading after Offboard.
   - Evidence: `blitz-after.json` keys `kbd-*-after-pause-confirm`, `kbd-*-after-resume` and `kbd-*-after-offboard`.

7. **S3 · A11y · In WebKit, arrowing through How it runs shows no focus.**
   - Actual: in WebKit, the engine Tauri uses on macOS, the radio reached with ↑ or ↓ has `:focus-visible` false and no outline. In Chromium it shows a 1 px outline at 50% alpha.
   - Expected: the design system's `--ring`, 2 px with a 2 px offset.
   - Screenshot: `blitz-after/kbd-webkit-runs-radio-focus.png`.

8. **S3 · Visual · In WebKit the selects are 20 px high.**
   - Actual: Runs on, How it runs and Reports to in Add an agent, and Runs on in Edit, are native selects 20 px high next to 36 px inputs. They are 35 px in Chromium.
   - Expected: controls 36 px high (design system, Sizes).
   - Fix: `appearance-none h-9` with a chevron, or the shadcn Select.
   - Screenshot: `after/default-dark/e09-add-sheet-filled.png`.

9. **S3 · Contrast · Three text pairs are below AA (4.5:1).**
   - The Offboard confirm button's text (destructive variant): 3.86:1.
   - The counts in the roster filters ("8", "0", `text-gray-400` on the pressed pill): 3.58:1.
   - The Add sheet's error description ("The office went away."): 4.34:1.
   - Evidence: `a11y-after.json`, the `lowest` lists.

10. **S3 · UX · Forms refuse without saying why.**
    - Add agent stays disabled with no message in these cases:
      - Name or Role is blank or whitespace.
      - "Another runtime…" has no name.
      - A name in CJK only gives no suggested messaging name.
    - Edit's Save is disabled with no message, and no `aria-invalid`, when Name is blank.
    - How it runs' Save is disabled with no message when Time zone is cleared. Its help line only covers When.
    - A name over 64 characters is accepted while typing. On submit the sheet says "Couldn't add the agent · Use 64 characters or fewer." without naming the field.
    - Expected: say what's missing (STAFF-5's rule: "Next is disabled until every field has a value and says what's missing"; design system, Errors: what happened and what to do).
    - Screenshots: `blitz-after/invalid-zone-cleared.png`, `invalid-edit-blank-name.png` and `long-name-over-64-refused-960.png`.

11. **S3 · Requirements · How it runs saves a cron line and an invalid time zone.**
    - Actual: "When: `* * * * *`" with "Time zone: Not/AZone" saves. The agent then reads "Scheduled · Status unknown".
    - Expected (RUN-7): plain words and presets, the next 3 run times, and never cron.
    - Screenshot: `blitz-after/invalid-cron-saved.png`.

12. **S4 · UX · Two agents can have the same name.**
    - Actual: Add agent accepts a second "Sam Okafor" (with messaging name `sam-2`), and Edit can rename Kai to "Noor Haddad". Reports to then lists two identical names.
    - Expected: warn, or show the messaging name next to duplicates, since the display name leads everywhere (spec §10).
    - Screenshot: `blitz-after/invalid-two-sam-okafor-roster.png`.

13. **S4 · Copy · The Former filter's empty state says "No left agents."** Expected: "No former agents." Screenshot: `after/default-dark/e03-roster-filter-former-none.png`.

14. **S4 · Copy · A missing runtime reads "Runs on Runtime not set".** It appears in the quick view and has no action. Expected (PROF-1): "Runtime not set · **Set runtime**". Screenshot: `blitz-after/req-unknown-runtime-strip.png`.

15. **S4 · Copy and requirements · The home folder is free text in both forms, with different hints.**
    - Add says "an absolute path on HQ", and Edit says "an absolute path on this Mac".
    - Expected (PROF-4 and STAFF-4): the home path is server-managed and read-only, and "No external home path … is accepted".

16. **S4 · Copy · The error and offboard messages don't follow the spec.**
    - "The office went away." gives no next step (design system: what to do next).
    - Offboard always says "Any open tasks … come back unassigned". It never says how many, and it leaves out "Nobody reports to <name>". Expected (spec §9, DEAL-4): it lists only what applies, for example "1 open task comes back unassigned".

17. **S4 · States · Slow and failed loads have no designed state.**
    - With `?org=slow` the Agents screen is blank until the organization loads: there's no skeleton.
    - With `?org=fails`, Agents and the org chart show a full-page "500 · Something went wrong · The office couldn't read its storage." with only **Back to the start**. There's no Retry.
    - Expected (design system): skeletons for loading, and errors that say what to do next.
    - Screenshots: `after/default-dark/e24-agents-loading.png`, `e22-agents-read-fails.png` and `e23-chart-read-fails.png`.

18. **S4 · A11y · The selected-member strip is a live region.** It is `role="status"` and holds 7 or 8 buttons, so each selection announces the whole block, button labels included. Fix: put only the name and status in a polite live region (`blitz-after.json`, `aria-strip`).

19. **S4 · History · The org chart's URL doesn't follow the selection.**
    - Selecting another agent keeps `?member=` on the first agent, so a reload selects the wrong agent.
    - Back with an Edit dialog open leaves the page and drops what was typed, without asking. This is the same history model as in F2-01b. Say if selection should be a history entry.

20. **S4 · Visual · The bottom-right toast covers the Add sheet's last fields** (Reports to and Home folder) for its lifetime. Screenshot: `blitz-after/kbd-webkit-add-focus.png`.

**Already known, so I didn't re-report them:**
- BUG-2: the chart toolbar overflows at 960, so the content scrolls 241 px sideways.
- BUG-18: the same root cause gives the Add an agent sheet 384 px (`max-w-md` loses to `sm:max-w-sm`) against the design's 440 px, and gives the buttons the 3 px, 50% shadcn focus ring.
- BUG-19: there's no `main` landmark. The only landmark is `nav "Main"`.

## Pairs that differ (4 of 356)

| Pair | Pixels | Why it isn't the merge |
|---|---|---|
| `960x640-dark/e67-edit-conflict` | 91,595 (**0 after scroll normalising**) | The Edit dialog's own scroller sat at 10 px in one run and 0 in the other, and the page at 180 px against 0. The normalised shots are byte-identical. The text, alert and focus are the same. |
| `960x640-light/e67-edit-conflict` | 99,411 (**0 normalised**) | The same scroll noise. |
| `default-light/20-chat` and `960x640-light/20-chat` | 751 each | "Waiting 9 min" against "8 min", and "12 min" against "11 min": a wall-clock minute ticked over, as in F2-01a and F2-01b. Chat isn't touched by the merge. |

The diff images are in `validation-f2-01c-shots/diff/`.

## Requirements in the after build

"Met" means met in the browser UI on the TypeScript mock. Every row is the same in 705158f.

| ID | Status | Evidence |
|---|---|---|
| DEAL-1 | Partly | "Deallocate" appears nowhere in `src`. Pause, Resume and Offboard exist, and Remove from team is in Edit teams. Unassign isn't in the agent's actions. |
| DEAL-3 | Partly | Pause confirms (e40), refuses with the error kept (e42), and shows Paused on the card and the strip (e43). It has no **"Paused · stop pending at office"** state, and the dialog copy isn't the spec's ("HQ will record Pause…"). Resume works in one click (e46). |
| DEAL-4 | Partly | Offboard names the teams, the lead role, the reports and their new manager, and the reserved name (e50, e54, e55). It moves the agent to Former members with "left Nov 10, 2026" (e53) and to the Agents › Former filter (e56). The error keeps the agent (e51). It always shows the generic open-tasks line (finding 16). |
| DEAL-5 | Partly | **Show paused** hides paused agents and counts (e44). It **isn't remembered**: after a reload it's on again (`req-show-paused-after-reload`). |
| PROF-1 | Partly | The quick view shows the runtime, messaging name, office and status (e31, e32). A missing runtime reads "Runs on Runtime not set", with no Set runtime (finding 14). |
| PROF-4 | Partly | Edit changes title, focus, model and runtime, with "Changes take effect at the agent's next session." (e60). The home folder is editable, though the spec says it's read-only (finding 15). Edit also changes name, role and messaging name, with a clear warning about the handle (e62). |
| STAFF-2 | Met (UI) | An added agent shows on its team card and is selected in the strip (e81). The Activity row isn't in this build. |
| STAFF-4 | Not met | Add an agent creates the member directly. There's no hire proposal or approval. It takes a free home path and a free-text role. |
| RUN-1 | Partly | How it runs offers Always on, On call, Scheduled and Started by a person, and saves each (e70 to e78). Add an agent offers no Scheduled. |
| RUN-2 | Partly | On call shows "Asleep · wakes when called". Scheduled and Always on show "Status unknown" (finding 5). |
| RUN-6 | Met | "Defaults for all agents: it goes back to sleep after 5 minutes …; a run stops after 30 minutes of work in its home, or 2 hours on a task." (e70, e71). |
| RUN-7 | Not met | The schedule is free text with no presets and no next 3 runs, and it accepts cron (finding 11). |
| RUN-10 | Partly | Pause and Resume work, but there's no pending-stop state (as for DEAL-3). |
| RUN-11 | Not met | There's no wake-up guidance in How it runs. |
| RUN-13 | Partly | "On call and scheduled agents only cost while they run." is there. Recent runs and cost aren't. |
| EMP-13 | Not met | How it runs has no health evidence, goal delivery, internal work or checkpoints. |
| ROLE-9 | Not met | There's no **Change role…**. Role is a free-text field in Edit. |
| HRL-2 | Not met | The Agents roster has no Built-in group with the HR lead. |
| Design system, Empty states | Met | "No agents yet · Agents are the AI employees of your organization. Add the first one…" and **Add your first agent** (e20, e21). |
| Design system, Confirmations | Met | Each confirmation names the object and the effect, and repeats the verb on its button (e40, e50). Offboard's button is red. |

## Blitz: owner jobs and attempts to break it (the same in both builds)

- **Owner job, end to end, in 2.5 s:** add Rita Gomes from Agents (the messaging name `rita-gomes` and Reports to Kai, the Platform lead, are suggested). Then open her from the roster, give her a schedule, pause her, resume her, offboard her, and find her under Former. There were no console errors.
- **Empty data:**
  - `?org=empty` shows the empty state.
  - The sheet says "There are no teams yet.", and Reports to lists only the owner.
  - Adding the first agent replaces the empty state with the table.
- **Long names** (61 and 59 characters, 960×640):
  - The sheet, the roster and the Edit and How it runs dialogs don't overflow.
  - Pause and Offboard break (finding 4).
  - Over 64 characters is refused late (finding 10).
- **Many items** (88 agents, 82 of them in Web): the roster renders in 0.35 s and the chart in 0.4 to 0.6 s. Reports to lists 92 options, with no errors.
- **Rapid clicks:**
  - 6 forced confirms pause once, with one toast and one badge.
  - 6 Add clicks add one agent.
  - 10 open-and-Escape cycles of Edit leave no dialog behind.
  - Fast selection then Edit edits the last agent selected.
  - The double-click falls through (finding 1).
- **Back and forward:**
  - Back from the chart returns to Agents, and Forward restores the agent.
  - Back with a dialog open discards it (finding 19).
- **Reload mid-flow:**
  - Reload with Edit typed into closes the dialog and saves nothing.
  - A half-filled Add sheet adds nothing.
  - A reload right after confirming Pause shows Paused, applied once.
- **Resizing:** I opened Edit, the sheet and How it runs at 1440, then resized to 960×640 and back. Only How it runs breaks (finding 2).
- **Invalid and duplicate input:**
  - Messaging names: a taken name (also in a different case), a trailing space, 65 characters, a space and a leading dot are each refused inline with a clear reason. 64 characters is accepted.
  - A relative home path is refused on submit, with the reason.
  - Duplicate display names are accepted (finding 12).
  - Cron and a bad time zone are accepted (finding 11).
- **Missing data** (injected in the mock's storage): an unreadable runtime shows "Runtime not set: choose one" in Edit. An unreadable run mode shows "Not recognised · This app can't read how it runs. Choose one." with Save disabled. That's good.

## UX, IA, visual and a11y

- **Keyboard path** (Chromium Tab; WebKit Option+Tab):
  - In the strip, Enter opens each dialog with focus inside, on Cancel for Pause and Offboard, the first field for Edit and Add, and the checked radio for How it runs.
  - Tab stays inside the dialog, and Escape returns focus to the trigger.
  - The roster's filters and names show a 2 px indigo outline.
  - The gaps are findings 6 and 7.
- **Accessible names:**
  - Every control in all 22 probed states has a name (none unnamed).
  - Dialogs are named by their titles ("Pause Noor Haddad?", "How Sam Okafor runs").
  - The run modes are a `group "Run mode"` of radios with their help in the name.
  - Team checkboxes are named by path ("Engineering › Web").
  - The roster table has a caption, row headers, and "Nothing reported" on the Now cells.
- **Contrast:** see finding 9. Everything else probed is 4.58:1 or better.
- **At 960×640:** the sheet and the Pause, Offboard and Edit dialogs fit with sample data. The roster and How it runs don't (findings 2 and 3). The chart's sideways scroll is BUG-2.
- **Tokens:** these screens use raw Tailwind gray, red and amber classes and shadcn defaults, rather than the design system's status and `--destructive` tokens. This is app-wide (dark only), as in F1-01.

## What I covered

- **58 employee states, in both builds and 4 runs each:**
  - The roster: All, the empty Paused and Former filters, Active, Paused with an agent, and Former with an agent.
  - The Add an agent sheet: open, a taken handle, an invalid handle, another runtime, filled in, a refusal, success, reopened clean, and Escape.
  - The empty organization and Add your first agent, the structure read failing on Agents and the chart, and the slow load.
  - Opening an agent from the roster, the quick view, and an unplaced agent.
  - Pause: the dialog, Cancel, a refusal, success, Show paused off, and Resume.
  - Offboard: a lead with reports, a refusal, success, Former members open, an agent with no reports, and an unplaced agent.
  - Edit: open, a taken handle, the handle-change warning, a required field missing, a refusal, saved, another runtime, and the stale (HQ changed) conflict.
  - How it runs: On call, Always on, Scheduled empty and filled, the time zone cleared, a refusal, saved, and Started by a person.
  - Add agent from the chart, and an agent added on the chart.
  - A keyboard path of 4 steps.
- **The mock scenarios:** `?org=empty`, `?org=fails`, `?org=slow`, `?member=<id>`, and `__mockOrgFailNext` for each command.
- **The F1-01 main set:** 31 states.
- **Blitz:** the probes listed above, plus keyboard in WebKit and Chromium, ARIA trees, light at 960 and requirement probes.
- **a11y:** 11 states × 2 sizes.
- **Checks in the after build:** check, unit tests and the full e2e suite.
- **Comparison with Avery's 28 shots.**

## Method

- The scripts are in `validation-f2-01c-shots/`, copied from `validation-bug-5-shots/` and adapted. I didn't edit the originals.
  - `validate.mjs` keeps the employee flows (each flow in a fresh context) and the F1-01 main flow.
  - `blitz.mjs` and `a11y.mjs` run with `node <script> <before|after>`.
  - `compare-avery.mjs` is adapted from F2-01a.
- Each build ran `bun run dev` from its worktree, on ports 5195 (before) and 5196 (after).
- Each flow ran in WebKit with the clock installed at 2026-11-10T12:00Z. It waited for finite animations only, then took screenshots with `animations: 'disabled'`. Each step also has a `.norm.png` with every scroller at the top.
- Stress data (many agents, unreadable runtimes and modes) was injected through the mock's `localStorage` organization. The repo was not touched.
- The double-click and rapid-confirm results were re-run 5 times per build (`rapid.mjs`, `dbl.mjs` in the scratchpad) before I reported them.
- E2E ran with `PW_PORT=4293 bunx playwright test` in the after worktree.

## Files

- Report: `workspace/validation-f2-01c.md`
- Screenshots and data: `workspace/validation-f2-01c-shots/`
  - `{before,after}/<run>/<state>.png` and `.norm.png`, where `<run>` is `default-dark`, `default-light`, `960x640-dark` or `960x640-light`
  - `diff/<run>/` (the 4 differing pairs)
  - `blitz-{before,after}/` and `blitz-{before,after}.json`
  - `a11y-{before,after}.json`
  - `avery-compare/` (my e2e captures, `mine-*.png`, and `avery-compare.json`)
  - `summary.json` and `results.json`
  - Logs: `validate-run1.log`, `blitz-*.log`, `a11y-*.log`, `e2e-after.log`, `check-after.log`, `unit-after.log` and `avery-compare.log`
  - Scripts: `validate.mjs`, `blitz.mjs`, `a11y.mjs` and `compare-avery.mjs`
