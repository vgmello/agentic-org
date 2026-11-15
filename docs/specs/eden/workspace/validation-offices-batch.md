# Validation: Offices batch, BUG-23 to BUG-27 (v1 a4d3aa0 vs its first parent 6848ba1)

**Verdict:** BUG-23, BUG-24 and BUG-27 are fixed. BUG-25 is fixed for contrast, but the footer still says "Shown from this Mac." BUG-26 is mostly fixed: the old 2–11 s overstatement is gone, but the countdown is rounded up, not down, so it overstates by less than 1 s at every write. Nothing regressed: the races pass 490/490, and every pixel difference is explained by these fixes. Two new LOW findings.

- **Change:** merge `a4d3aa0` ("Merge jules/offices-restart-repair into v1"), compared with its first parent `6848ba1`.
- **Files changed:** 6 in all. In the product: `OfficeDetailsSheet.svelte`, `offices/model.ts`, `offices/+page.svelte`. Tests and config: `model.test.ts`, `e2e/offices.spec.ts`, `playwright.config.ts`.
- **How I tested:** 2026-11-10, as a reviewer only, with nothing changed in the product repo.
  - Throwaway worktrees with dev servers (`bun run dev --mode e2e`): 5343 (before) and 5344 (after), plus a separate `checks` worktree for the suites. All are removed now, and the servers are stopped.
  - WebKit is the main engine and Chromium the check. The dark theme throughout. Widths 1440×900 and 320×640, plus 960×640 in the pixel set.

## Per-bug results

| Bug | Before (6848ba1) | After (a4d3aa0), WebKit and Chromium | Result | Code |
| --- | --- | --- | --- | --- |
| **BUG-23** Restart office keeps focus | Enter on Restart office: focus goes to `BODY` 4–13 ms later. It stays there for the whole "Restarting…" (sampled to 29 s in the stuck case) and after the refusal. The button is `disabled`. | Focus stays on `button#restart-office` from Enter, through "Restarting…" (`aria-disabled="true"`, `aria-busy="true"`, not `disabled`, opacity 0.5), to the refusal. The refusal reads "The office host is still starting. Try again in a moment." (`role="alert"`). The busy state is announced by the sr-only `role="status"` "Restarting…". A repeated Enter and Space while busy make 1 call, not 3. In the stuck case focus stays on the button for 29 s, then returns to "Restart office". After a successful restart, focus moves to the sheet's × (`sheet-close`) and then back to the row on close. | **Fixed** | `OfficeDetailsSheet.svelte:197` (`aria-disabled={restarting}`, was `disabled` at :149), `:86` (guard), `:189` (status), `:119` (× after success) |
| **BUG-24** unsupported_version copy and repair steps | "Parts of Agentic Org on this Mac don't match. Update or reinstall Agentic Org, then log out and back in." The only actions are Copy details and two "Close" buttons. | Reason, exactly: "The release components on studio-mac.local don't match. Run diagnostics and repair the selected instance there." The toggle "Show local CLI repair instructions" ↔ "Hide repair instructions" has `aria-expanded` false/true and `aria-controls` pointing at the section in the dialog (hidden while closed). Focus stays on the toggle after Enter, Space and a pointer click (WebKit too). The section reads, exactly: "On studio-mac.local, run:", then `agentic-org doctor` (pre/code), then "Copy command", which changes to "Copied" (Chromium's clipboard = `agentic-org doctor`), then "It checks the Agentic Org instance on that machine and says what to repair. Follow its steps there. This page updates on its own when the office is back.", then "If studio-mac.local runs more than one instance, add --name <instance>." with `--name <instance>` in a code span. For the legacy host "This Mac", both lines say "the office's machine". No restart button and no note. The reason and the steps have no restart, reinstall or "this Mac" wording, for all 5 hosts (including a long dotted one, a 68-character unbroken one and HTML markup, which renders as text). At 320 px: the sheet is 320 wide, nothing overflows sideways, the title clears the ×, and Copy details is reachable by scrolling. | **Fixed** | `model.ts:182` (`repairLocation`), `:230` (reason), `OfficeDetailsSheet.svelte:170-178` (toggle and steps), `:134` (WebKit focus) |
| **BUG-25** footer contrast | `text-gray-500` 12 px, rgb(106,114,130) on rgb(30,41,57) = **3.03:1** | `text-gray-400` 12 px, rgb(153,161,175) on rgb(30,41,57) = **5.64:1** (same at 320 and in both engines) | **Contrast fixed. Wording not changed.** | `offices/+page.svelte:279` |
| BUG-25 wording | "Shown from this Mac." | Still "Shown from this Mac." In a browser that may be on another device this isn't accurate. The spec's footer is "Shown from HQ. Statuses include last-seen information." (offices spec §6.1, line 790). | **Not addressed** (see N1) | same line |
| **BUG-26** countdown honesty | First write after a push of "in 45 s": 46–55 s, depending on the 10 s tick's phase. That's 2–11 s over floor(true). Real time: 49, 52, 46, 51, 48, 46 for a true 44.99 s. | Clock-controlled, 11 tick phases × 3 lead times: the first write is 45 for 45.000 s, in every phase. It's 45 for 44.6 s and 46 for 45.4 s. Each later tick writes ceil, for example 37 when 36.23 s remain. Real time: 45 for a true 44.997–44.999 s, 6 of 6 runs. Row and sheet always agree, and the label doesn't tick ("restarting shortly"). | **Mostly fixed:** no longer stale, but rounded **up**: +1 s over floor(true) at every write unless the remaining time is a whole second (see N2) | `+page.svelte:107` (`$effect.pre` sample), `:150`; `model.ts:219` (`Math.ceil`) |
| **BUG-27** one Close, focus returns | Two buttons named "Close" (the × and the footer) in the online, crashed and unsupported sheets. In WebKit, opening by pointer then closing with the × leaves focus on `BODY`. | One "Close" (the ×) in every state. All five close paths return focus to the row that opened the sheet, in both engines: Enter on ×, Escape, a click on ×, a click on the overlay, and pointer open then × (WebKit). It also returns there after a status change while the sheet was open. At 320 too. | **Fixed** | `+page.svelte:179-184` (`restoreDetailsFocus`), `:282`; `OfficeDetailsSheet.svelte` footer rendered only `{#if why}` |

**Evidence (all under `validation-offices-batch-shots/`):**

- **Probe data:** `bugs.json` and `bugs-bug23_stuck_…json` (a rerun of the stuck and BUG-27 probes after two harness fixes; it supersedes the `bug27` harness-error rows in `bugs.json`).
- **Shots:** `bugs/` for each probe: `bug23-busy-*`, `bug23-refused-*`, `bug24-{row,sheet,open,copied}-<host>-<width>-<build>-<engine>.png`, `bug24-steps-end-320-after-*.png`, `bug25-*.png`, `bug26-phase4-*.png` and `bug27-open-*.png`.

**Notes that aren't findings:**

- **WebKit pointer clicks.** A pointer click on Restart office or Copy command leaves focus on the sheet container, not the button, in both builds. That's WebKit's platform convention, and focus never drops to the body.
- **The Where row.** With the legacy host, the Where row still reads "This Mac (you're here)" next to the reason's "the office's machine". That's BUG-61, the host label.
- **Jules's shots** in `offices-merge-a4d3aa0` agree with these runs.

## New findings

**N1 · LOW (S4) · The Offices footer says "Shown from this Mac." in a browser**

- **Repro:** open Offices from any browser.
- **Expected:** wording that's true off the office machine. The spec's §6.1 has "Shown from HQ. Statuses include last-seen information."
- **Actual:** "Shown from this Mac." BUG-25 fixed only its colour.
- **Evidence:** `bugs/bug25-1440-after-webkit.png`.
- **Code:** `apps/org-ui/src/routes/(shell)/offices/+page.svelte:279`.

**N2 · LOW (S4) · The countdown rounds up, so it overstates by up to 1 s at each write**

- **Repro:** push `offline` / `crashed` with `next_restart_at` = now + 45 s.
- **Expected:** never more than the true remaining time, rounded down, so 44 at 44.997 s.
- **Actual:** "Restarting in 45 s" with 44.997 s left (6 of 6 real-time runs, both engines). Each 10 s tick writes ceil too, for example 37 at 36.23 s.
- **Evidence:** `bugs.json` → `bug26-real` and `bug26-phases`.
- **Code:** `apps/org-ui/src/lib/offices/model.ts:219`, `Math.ceil` (doc comment at :213, "rounded up").
- **Context:** between the spec's 10 s updates the shown value necessarily ages, by up to about 10.6 s in both builds. That's the cadence, not this bug.

**N3 · LOW (S4, pre-existing in 6848ba1) · A restart refusal's message outlives the state it was about**

- **Repro A:**
  1. Open a crashed office.
  2. Press Restart office and get the `not_ready` refusal.
  3. The office's reason changes to `unsupported_version`.
- **Result A:** the red `role=alert` "The office host is still starting. Try again in a moment." stays under the repair instructions, where there's no restart.
- **Repro B:** a restart answered `unavailable` shows the host's raw message as an error. The spec's §3.3, rev 15, says to show that reason's row. The message also stays after the office is listed Online.
- **Expected:** the note clears when the office's status or reason changes, and `unavailable` shows the reason row.
- **Evidence:**
  - `extra/notready-then-unsupported-after-webkit.png`
  - `extra/unsupported-during-restart-after-*.png`
  - `extra.json`
- **Code:** `OfficeDetailsSheet.svelte:69` (reset only on an office id change) and `:103` (`restartNote = … err.message` for `refresh`), shown at `:193`.

## Regressions

- **Races:** `race.mjs` on the after build, WebKit, 10 runs each.
  - The 32 earlier cases: **320/320**.
  - The storm case: **10/10**.
  - The 16 failure orders: **160/160**.
  - In all, **490/490**, with no page errors (`race-after.json`).
- **Pixels:** `validate.mjs`, flows office and main, at 1440×900 and 960×640, OS dark and light, 62 states × 4 runs.
  - **248 pairs: 119 are byte-identical and 129 differ. No step failed**, and the console showed the same 3 messages in both builds.
  - Every difference is explained:

| Pairs | Steps | Why |
| --- | --- | --- |
| 48 | 29-offices, o01, o02, o20, o30, o32, o33, o50, o60, o62, o70, o72 | 637 px, exactly the footer text box (BUG-25 colour) |
| 28 | o04–o08, o11, o23 (sheet closed) | Focus is back on the row (BUG-27). It was `body`, now the row button with its 2 px ring. Plus the footer. |
| 24 | o03, o10, o22, o51, o61, o71 (sheets with no reason) | The footer Close is gone, so the footer is empty (BUG-27). Controls go from 16 to 15. |
| 28 | o09, o21, o31, o34, o40, o41, o42 (sheets with a reason) | The footer Close is gone, so Restart office and Copy details move down 44 px. The header's new `pr-14` wraps "…approves new offices." onto two lines, so the list moves down 20 px. o42's `disabled` count goes from 3 to 2 (`aria-disabled`, BUG-23). |
| 1 | 20-chat at 960x640-light | Run timing: "Waiting N min". The known effect, not from this merge. |

- **Edge probes** (`extra.mjs`, both builds and both engines):
  - `not_authorized`: focus was on `BODY` before (WebKit) and stays on Restart office after.
  - `unknown_method`: the button is removed, and focus moves to Copy details, in both builds.
  - An `unsupported_version` push while "Restarting…" is focused: focus moves to the repair toggle, not the body.

## Local checks on a4d3aa0

These ran in a clean worktree. Every full suite ran through `~/.agency/bin/heavy`, with 2 workers.

- `bun run check`: `COMPLETED 1160 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS` (exit 0)
- `bun run test`: `Test Files  50 passed (50)` / `Tests  1347 passed (1347)` (exit 0)
- `PW_PORT=4393 bun run test:e2e --workers=2`: `304 passed (9.4m)` (exit 0). This includes BUG-23, both BUG-24 tests, BUG-25, BUG-26 and both BUG-27 tests.
- Also, the opt-in WebKit project (`PW_WEBKIT=1 … --project=webkit`): `3 passed (17.3s)`.

## Files

`validation-offices-batch-shots/` holds:

- **Scripts:**
  - `race.mjs`, `validate.mjs`: copied from `validation-bug-48-18-shots/` with the ports and Playwright path changed. The originals are unedited.
  - `bugs.mjs`, `extra.mjs`, `sheet320.mjs`: new.
- **Results:**
  - `race-after.{json,log}`, `race/`
  - `bugs*.json`, `bugs*.log`, `bugs/`
  - `extra.{json,log}`, `extra/`
  - `results.json`, `summary.json`, `validate-run.log`, `before/`, `after/`, `diff/`
- **Check logs:** `check-after.log`, `unit-after.log`, `e2e-after.log`, `e2e-webkit-after.log`
