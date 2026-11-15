# Validation · F2-01a copilot move (v1 783cb96 vs 5e673bc)

**Verdict:** no behaviour change and no new bugs. F2-01a moves 11 copilot files to `src/lib/modules/copilot` and `features/draft-text` unchanged. Elsewhere it changes only import lines and a comment, plus a test-only screenshot helper in `e2e/copilot.spec.ts` (`copilotShot`, which waits for animations). 51 copilot states were compared in both builds, plus the F1-01 screen set at 1440×900 and 960×640, dark and light, on WebKit. 311 of 328 pairs are byte-identical, and the other 17 are scroll offsets or clock noise (0 px once reset). E2E: `222 passed (4.5m)`. Avery's archive matches.

**Pre-existing bugs (the same in both builds), checked by me against work-items-ux.md on v1, sent to ada 10 Nov:**
1. Major: Esc closes Report a bug with text in it and loses it. Expected "Discard this bug report?" (§4 States, BUG-9). Shot: `c29-kbd-escape.png`.
2. Minor: Report bug is enabled with required fields empty. Expected disabled with "Add a title, what happened and a severity" (§4 Required, BUG-2). Shot: `c07-report-needs-severity.png`.
3. Minor: success stays on the list with "Bug B-1 reported." Expected: the bug opens and the toast reads "Bug B-15 reported. Riley will triage it." (§4 Success). Shot: `c09`.
4. Minor (a11y): after Enter in a panel's Instruction field, focus jumps to "What went wrong?" at the top of the dialog, which is off-screen. Shot: `c28`.
5. Trivial: "Draft used.Shortened to fit…" has no space. Shot: `c25-title-shortened-to-fit.png`.
6. Trivial: at 960×640 the toast covers the task sheet's copilot panel. Shot: `960x640-dark/c43`.
7. Trivial: the dialog's scroll position after using a panel varies between 0 and about 600 px.

Screenshots: `workspace/validation-f2-01a-shots/` (not committed). The full working report follows.

---

# Validation: F2-01a, the copilot moves to lib/modules/copilot

**Verdict: No behaviour change found. Nothing blocks the app from loading.**

- Change: merge `783cb96` ("Merge avery/f2-01-copilot into v1 (Tomas READY c50254e, CI green, ada GO)") on `v1`.
- Compared with its first parent `5e673bc` ("Merge kai/shell-audit into v1 (B1-03, Tomas READY 7e77de1)").
- Date: 2026-11-10. I tested the product as a reviewer only and changed nothing in the product repo.

## Summary

- **Copilot:** I drove 51 copilot states in both builds. They cover every state `e2e/copilot.spec.ts` covers, plus the in-flight states, keyboard use, and Dismiss and Cancel on the panels. See the table below.
- **Main screens:** I re-ran the same 31 states as F1-01.
- **Matrix:** each state ran at 1440×900 and 960×640, with the OS in dark and in light mode, on WebKit. That gives 82 states × 4 runs = 328 screenshot pairs per run. I ran the whole set twice.
- **Run 2 (the final one):**
  - 311 of 328 pairs are byte-for-byte identical PNGs.
  - Of the other 17, 16 differ only in where the dialog body is scrolled. With every scroller reset to the top, they are pixel-identical (0 px). The 17th is Chat's "Waiting 9 min" against "8 min", a clock minute ticking over.
  - Every scripted interaction succeeded in both builds.
  - The URL, title, `<h1>`, alerts, focused element and counts of visible and disabled controls match in every state. So does the full visible text, apart from the two timing cases explained below.
- **Run 1:** the same kinds of noise came up, but on different states and in both directions (see "Noise"). Nothing came up in both runs that would point to the merge.
- **Console:** the output is identical in both builds. The only messages come from the deliberate 404 page.
- **E2E:** the after build's suite passes, **`222 passed (4.5m)`**. All 17 copilot tests are included.
- **Avery's archive:** 6 of his 8 archived copilot screenshots are byte-identical to the same captures from my after-build e2e run. The other 2 show the same state, with spinner or scroll noise (see below).

**Source check:** `git diff -M 5e673bc 783cb96` touches 15 files:

- The 11 files under `src/lib/copilot/` are 100% renames to `src/lib/modules/copilot/`.
- `DraftableText.svelte` moved to `modules/copilot/features/draft-text/`. Only its four import lines changed.
- `NewTaskDialog.svelte`, `ReportBugDialog.svelte` and `TaskSheet.svelte` changed only their import paths.
- `e2e/copilot.spec.ts` changed one comment path and replaced the shared `shot` helper with a per-test `copilotShot`. It also adds two more captures (`report-dialog-drafting`, `copilot-empty-instruction`). These are test-only changes.
- Nothing under `src/routes`, `src/lib/office` or `static` changed.

The app is dark only, as in F1-01. The light runs confirm that it ignores the OS setting, in both builds.

## Findings (introduced by F2-01a)

None. I found no difference that F2-01a introduced and no blocker.

## Noise, and why it is not a finding

Each of these appears in some runs and not others, and goes in both directions, before over after and after over before:

| What | Where | Evidence it is timing, not the merge |
|---|---|---|
| The dialog body lands at a different scroll offset for the same state: 0, 17, 85, 157, 177, 411 or 597 px. | The Report a bug dialog and its copilot panels | Run 1 had it in `default-dark` C1 to C7 and in `960x640-light` A10 to A13. Run 2 had it in `960x640-dark` and `default-light` instead. With every scroller reset to the top (`*.norm.png`), all 16 run-2 cases are 0 px. Avery's archived `copilot-empty-instruction.png` against my after build shows the same thing. |
| "Waiting 9 min · blocks release" against "8 min" | Main screen 20 (Chat) | The minute rounds against wall-clock time. Run 1 had it in 2 runs and run 2 in a different one. |
| The toast "Task T-1 created." is still on screen | Copilot C45, the task sheet after Undo | Seen in before and in after, in different runs. It is a 0 px difference because the toast was fading. |
| The welcome list was still loading ("Looking for organizations on this Mac…") | Main screen 01 | Run 1 only, in both directions (3 controls against 1, and 1 against 3). In run 2 the script waits for the list. |

## Copilot screens and states

Screenshots are in `validation-f2-01a-shots/{before,after}/<run>/<name>.png`:

- `<run>` is `default-dark`, `default-light`, `960x640-dark` or `960x640-light`.
- `<name>.norm.png` is the same state with every scroller at the top.
- Pixel diffs are in `diff/<run>/`, and run 1's diffs are in `run1-diff/`.

Every row exists in all four runs. The paths given are `default-dark`.

Each flow starts fresh: a new browser context, the "Acme" organization created, then the Atlas project. The `?copilot=` URL parameter picks the mock scenario.

| # | Screen and state (spec) | Interaction | Result, run 2 (all 4 runs) | Screenshot (`after/default-dark/…`, same name under `before/`) |
|---|---|---|---|---|
| A1 | Atlas › Bugs, the entry point | Open the project and its Bugs tab | Same, byte-identical | `c01-bugs-tab.png` |
| A2 | Report a bug **open**, copilot on: "Start from a sentence", 4 × Draft or improve | Click "Report a bug" | Same | `c02-report-open.png` |
| A3 | Sentence typed | Fill in "What went wrong?" | Same | `c03-sentence-typed.png` |
| A4 | Sentence **drafting, in flight**: "Drafting the report…", Cancel draft, fields locked | Click "Draft the report", with the clock held | Same | `c04-sentence-drafting.png` |
| A5 | **Suggestion**: the banner "Drafted by the copilot. Nothing is reported yet.", Drafted tags, "The copilot suggests Major. You choose." | The draft lands | Same | `c05-sentence-drafted.png` |
| A6 | The person edits the title, and its tag goes | Type a new title | Same | `c06-title-edited.png` |
| A7 | Report without a severity, so the alert "Choose a severity." shows | Click "Report bug" | Same | `c07-report-needs-severity.png` |
| A8 | Severity chosen, so the suggestion goes | Pick Major | Same | `c08-severity-chosen.png` |
| A9 | **Accept**: reported, the toast "Bug B-1 reported.", and the row "Major?" | Click "Report bug" | Same | `c09-bug-reported.png` |
| A10 | Own text, then two drafts; the second suggests Trivial | Draft, then draft again | Same after normalising (run 1 only: scroll) | `c10-own-text-then-second-draft.png` |
| A11 | **Undo draft** restores the person's own text | Click "Undo draft" | Same | `c11-undo-draft.png` |
| A12 | Cancel draft while in flight | Draft, with the clock held | Same | `c12-cancel-draft-inflight.png` |
| A13 | Cancelled: the late draft never lands | Cancel, then wait 1.2 s | Same | `c13-cancel-draft-done.png` |
| C1 | Field panel **open**, "Improve this text", Sample data | "Draft or improve What happened" | Same after normalising (scroll) | `c14-panel-open-improve.png` |
| C2 | Panel **in flight**, "Drafting…" | Click Improve, with the clock held | Same after normalising | `c15-panel-drafting.png` |
| C3 | Panel **suggestion**: "The copilot proposes", notes, Use this draft, Try again, Dismiss | The draft lands | Same after normalising | `c16-panel-proposal.png` |
| C4 | **Accept**: the field holds the draft, with the Drafted tag and "Draft used. Undo" | "Use this draft" | Same after normalising | `c17-proposal-accepted.png` |
| C5 | Field **Undo** restores the person's own text | Click Undo | Same after normalising | `c18-field-undo.png` |
| C6 | Suggestion again | Open the panel and click Improve | Same after normalising | `c19-proposal-again.png` |
| C7 | **Dismiss**: the panel closes and the field is unchanged | Click Dismiss | Same after normalising | `c20-proposal-dismissed.png` |
| D1 | Panel open on an **empty** field, "Draft this text" | "Draft or improve Expected" | Same after normalising | `c21-panel-open-draft-empty.png` |
| D2 | **Empty**: "Say what it should be about, then ask for a draft." | Click Draft with nothing typed | Same after normalising | `c22-empty-instruction-error.png` |
| D3 | The error clears as the person types an instruction | Type an instruction | Same after normalising | `c23-instruction-typed-error-cleared.png` |
| D4 | Panel **cancelled** | Click Cancel | Same | `c24-panel-cancelled.png` |
| D5 | Draft longer than the field, "Shortened to fit the limit of 120 characters" | A 200-character instruction, Draft, then Use | Same | `c25-title-shortened-to-fit.png` |
| E1 | **Keyboard**: open the panel with Enter on "Draft or improve Steps to reproduce" | Focus the button, press Enter | Same. Focus is on the button. | `c26-kbd-panel-open.png` |
| E2 | Tab goes to the Steps field | Tab | Same. Focus is on `#bug-steps`. | `c27a-kbd-tab-to-field.png` |
| E3 | Tab goes to the panel's Instruction field, then type | Tab, then type | Same. Focus is on `#bug-steps-instruction`. | `c27b-kbd-tab-to-instruction.png` |
| E4 | Enter asks the copilot and does not submit the report | Enter | Same. Focus moves to `#bug-sentence` (pre-existing P2). | `c28-kbd-enter-asks.png` |
| E5 | Escape | Escape | Same. The dialog closes (pre-existing P1). | `c29-kbd-escape.png` |
| F1 | **Error, failed**: the sentence alert "The copilot couldn't draft this" | `?copilot=failed`, then "Draft the report" | Same | `c30-failed-sentence-error.png` |
| F2 | Panel **error, failed** (one alert at a time) | Field panel, then Improve | Same | `c31-failed-panel-error.png` |
| F3 | Panel cancelled after the error | Click Cancel | Same | `c32-failed-panel-cancelled.png` |
| F4 | The sentence error again | Click "Draft the report" | Same | `c33-failed-sentence-error-again.png` |
| F5 | Error **dismissed**, focus back on "Draft the report" | "Dismiss the draft error" | Same | `c34-failed-error-dismissed.png` |
| T1 to T5 | **Error, timeout**: "The copilot took too long", the same five steps | `?copilot=timeout` | Same (run 2: T1 scroll only, 0 px normalised) | `c30-timeout-…` to `c34-timeout-…` |
| O1 | Copilot **off** (no runtime): no smart buttons and no sentence box (SRV-18 style hidden state, not a bug) | `?copilot=off`, Report a bug | Same | `c35-off-report-dialog.png` |
| O2 | Off, and the bug is still reported by hand | Fill in the form and report | Same | `c36-off-reported.png` |
| O3 | Off: New task has a plain Description | Tasks tab, then New task | Same | `c37-off-new-task-dialog.png` |
| K1 | New task: the Description panel open | Fill in, then Draft or improve | Same | `c38-task-dialog-panel.png` |
| K2 | New task: suggestion | Click Improve | Same | `c39-task-dialog-proposal.png` |
| K3 | New task: accepted | "Use this draft" | Same | `c40-task-dialog-accepted.png` |
| K4 | Task created with the kept text, toast "Task T-1 created." | Click "Create task" | Same | `c41-task-created.png` |
| K5 | Task sheet open | Click the card | Same | `c42-task-sheet.png` |
| K6 | Task sheet: suggestion on Description | Draft or improve, then Improve | Same | `c43-task-sheet-proposal.png` |
| K7 | Task sheet: accepted, then "Description saved." | "Use this draft" | Same | `c44-task-sheet-accepted-saved.png` |
| K8 | Task sheet: Undo, which is saved too | Click Undo | Same, 0 px (toast timing in the text only; see Noise) | `c45-task-sheet-undo.png` |

The mock answers after 700 ms. For A4, A12 and C2 I held the page clock (`page.clock.pauseAt`) so the in-flight state could be captured, then released it.

The copilot has no other surface in this build. The design (`docs/design/smart-features.md`) builds only "Draft or improve" and "Report a bug from a sentence" in P1. `team.staff`, the feature-request and "why?" copilots in `docs/specs` are not built.

## Main screens (the F1-01 set)

These are the same 31 steps as `validation-f1-01.md`: welcome, create, the Agents shell and dialog, the Kai profile, the Org chart views, Projects, Beacon and its Tasks and dialog, Work and its Bugs tab, Chat and its channel, thread and dialog, Sessions, Activity, Processes, Offices, Settings and 404.

In run 2, all 31 are byte-identical in all 4 runs, except `20-chat`. That one differs by 751 px, the "Waiting 9 min" minute rollover in `default-light` only (see Noise).

Paths: `validation-f2-01a-shots/{before,after}/<run>/01-welcome.png` to `31-not-found.png`.

## Avery's archived screenshots

His worktree is `/Users/vgmello/projects/agentic-consulting/app-avery`. It is now on `avery/f2-01-processes` at `b41ab19`, and its `apps/org-ui/playwright-report/copilot-783cb96/` holds 8 PNGs. They are Chromium, 1440×900, from the e2e `copilotShot` captures, dated 13:24 to 13:25 today.

I compared each one with the same capture from my after-build e2e run (`test-results/<same folder>/<same file>`) using `compare-avery.mjs`:

| Capture | Result |
|---|---|
| report-dialog-drafted | Byte-identical |
| report-dialog-improve-proposal | Byte-identical |
| copilot-failed | Byte-identical |
| copilot-timeout | Byte-identical |
| report-dialog-no-copilot | Byte-identical |
| task-description-drafted | Byte-identical |
| report-dialog-drafting | Same state. 5,592 px differ, all inside the spinner, button and field area (x 455–984, y 330–520). The spinner is mid-rotation and the fields are mid-fade to disabled. This capture is taken without waiting for animations. |
| copilot-empty-instruction | Same state (the empty-instruction error on Expected). The dialog body is scrolled about 180 px further in mine. This is the scroll noise above. |

My copies are in `validation-f2-01a-shots/avery-compare/` (`mine-*.png` and `avery-compare.json`). So his archive matches the after build.

## Pre-existing observations (not F2-01a; identical in both builds)

These are for Eden to decide whether to file. The references are to `docs/specs/work-items-ux.md` and `docs/design/smart-features.md` at `783cb96`.

1. **P1 · Escape throws away a bug report without asking.**
   - Repro: open Report a bug, type a title, open "Draft or improve Steps to reproduce", get a proposal, then press Esc.
   - Expected: work-items §4 States and BUG-9 say that closing the form with text in it asks "Discard this bug report?" (Discard · Keep editing).
   - Actual: the dialog closes at once, and the title, the instruction and the proposal are lost.
   - Screenshots: `c28-kbd-enter-asks.png` then `c29-kbd-escape.png`. The same happens on Escape at any point, not only from a panel.
   - Suggested severity: Major. Data loss of the person's text, on a flow the copilot makes longer.
2. **P2 · After Enter in the panel's Instruction field, focus jumps to the top of the dialog.**
   - Repro: with the keyboard only, open a field's Draft or improve, type an instruction and press Enter.
   - Expected: focus stays in the panel, for example on the proposal or "Use this draft", so a keyboard user can accept or dismiss.
   - Actual: when the proposal arrives the Instruction field is removed, and focus goes to "What went wrong?" at the top of the dialog, which is off-screen. The person has to Tab all the way back down.
   - Seen on WebKit (`c28` focus = `textarea#bug-sentence`). The dialog's own Dismiss correctly returns focus to "Draft the report" (`c34`).
   - Suggested severity: Minor, accessibility.
3. **P3 · A missing space in the "shortened" note.** It reads "Draft used.Shortened to fit the limit of 120 characters." It should read "Draft used. Shortened …". Screenshot: `c25-title-shortened-to-fit.png`. Trivial.
4. **P4 · Where the dialog sits after using a panel is unpredictable.**
   - For the same state, the dialog body lands at a scroll offset anywhere between 0 and about 600 px, depending on timing.
   - Sometimes the dialog's title and close button are scrolled out of view. For example, compare `before/default-dark/c14-panel-open-improve.png` with the after one in run 1, under `run1-diff/default-dark/`.
   - This is cosmetic, but it also makes screenshot evidence flaky.
   - Suggested severity: Trivial or Minor.
5. **P5 · At 960×640 the toast covers the task sheet's copilot panel.** "Task T-1 created." sits over the proposal and the "Use this draft" row for a few seconds. Screenshot: `960x640-dark/c43-task-sheet-proposal.png`. Trivial.
6. **Outside the copilot, but on the same dialog:**
   - **Report bug is enabled with nothing filled in.** The spec says it is disabled with "Add a title, what happened and a severity" until all three are set. Instead the button is enabled, and the form shows "Choose a severity." on submit. Screenshot: `c07`.
   - **Success doesn't follow the spec.** The spec says the bug opens and the toast reads "Bug B-15 reported. Riley will triage it." Instead the app stays on the list with "Bug B-1 reported." Screenshot: `c09`.
   - The e2e suite asserts the current behaviour, so these may be deliberate later decisions. Check before filing.

The design's principles hold on these screens:

- Drafts only: nothing is reported until "Report bug" is pressed, as A5 to A9 show.
- The smart buttons are hidden when no runtime is set up (O1, O3).
- Notes are shown beside the draft.
- "Sample data" marks the mock.

## Console output

These are identical in both builds, in all four runs, and in both validation runs. They all come from main step 31, the deliberate unknown route:

| Build | Type | Message |
|---|---|---|
| before and after | error | Failed to load resource: the server responded with a status of 404 (Not Found) |
| before and after | warning | The next HMR update will cause the page to reload |
| before and after | error | Error: Not found: /nope |

Neither build logged a console error, warning or page error on any copilot state or any other screen.

## E2E (after build)

`cd apps/org-ui && PW_PORT=5193 bunx playwright test --reporter=line` gave:

```
  222 passed (4.5m)
```

That includes all 17 tests in `e2e/copilot.spec.ts`. The suite runs Chromium, not WebKit. The full log is `validation-f2-01a-shots/e2e-after.log`.

## How each build was started

1. I made throwaway worktrees in my scratchpad from `/Users/vgmello/projects/agentic-consulting/app`:
   - `git worktree add --detach <scratch>/f2/after 783cb96`
   - `git worktree add --detach <scratch>/f2/before 5e673bc`
2. I installed dependencies with bun 1.4.4: `bun install --frozen-lockfile` in each `apps/org-ui`. Each installed 118 packages with no errors.
3. I started each one on the browser TypeScript mock (the office mock plus `src/lib/(modules/)copilot/mock.ts`), with no Tauri:
   - before: `cd before/apps/org-ui && bun run dev --port 5191 --strictPort`
   - after: `cd after/apps/org-ui && bun run dev --port 5192 --strictPort`
4. When I stopped them, `lsof` confirmed that each port's process ran from its own worktree: 5191 from `f2/before/apps/org-ui` and 5192 from `f2/after/apps/org-ui`.
5. I drove both with `validation-f2-01a-shots/validate.mjs`, which is Eden's F1-01 script adapted:
   - Copilot flows were added, each in a fresh browser context.
   - In-flight states were captured with the page clock held.
   - Screenshots used `animations: 'disabled'` and `caret: 'hide'`.
   - Settling ignores infinite spinners.
   - The script also records alerts, the focused element, disabled controls and scroll offsets, takes a scroll-normalised `.norm.png` per state, and checks bytes as well as pixels.
   - It ran on Playwright 1.65.0 (from the after build's `node_modules`) with WebKit, at 1440×900 and 960×640, with `colorScheme` dark and light.
   - The clock started at 2026-11-10T12:00:00Z, and each context started with empty storage.
6. Data files:
   - Run 2: `summary.json`.
   - Run 1: `summary-run1.json`. That run had no scroll recording and no `.norm` shots, and it waited less on the welcome screen.
7. I ran the e2e suite once in the after worktree on port 5193.
8. I stopped all servers and removed both worktrees afterwards. The product repo's working tree was not touched.
