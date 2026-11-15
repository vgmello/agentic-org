# Validation · BUG-48 and BUG-18 (v1 2c2d540 vs 282ce3c)

**Verdict:** both fixed, with nothing else changed and no new bugs.

**BUG-48, fixed, checked by me:**
- **Code:** `offices/+page.svelte:80-88` gives each read a request number (`request = ++refreshSequence`). A result applies only if `request > successfulRefresh`, and only a successful read moves `successfulRefresh` forward.
- **Race:** the 32 earlier cases plus the storm went from 320/330 before to 330/330 after. The 16 new failure orders went from 110/160 to 160/160.
- **The two cases that failed before:** in `race-before.json` and `race-after.json`, `observe/newest-read-fails` and `good-after-push-newest-fails` each go from 0/10 to 10/10.

**BUG-18, fixed, checked by me:**
- **Code:** `ProcessSheet.svelte:99` adds `focus-visible:outline-solid`.
- **Probe:** `focus.json` shows the Close button's outline style going from `none` to `solid`, at 2 px with a 2 px offset in the `--ring` colour, in WebKit and Chromium with the OS dark or light. The sheet stays 440 px.
- **Screenshot:** `focus/after-chromium-dark-process-close-4x.png` shows the ring drawn clear of the button.
- **Other buttons:** four other shared Buttons are pixel-identical before and after.

**Pixels and checks:**
- **Pixel compare:** 415 of 428 pairs identical. 12 are the focused Close outline, as intended, and 1 is the Chat clock.
- **Local checks, through heavy:** check `COMPLETED 1158 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`, unit `Tests  1330 passed (1330)`, e2e `262 passed (7.2m)`.

The full working report follows.

---

# Validation: BUG-48 and BUG-18 (v1 2c2d540 vs its parent 282ce3c)

**Verdict: both bugs are fixed, and nothing else changed.**

- **BUG-48 is fixed.** A failed newer status read no longer discards an older read's newer answer. All 32 earlier race cases and the storm case pass 10/10 on 2c2d540. `observe/newest-read-fails` goes from 0/10 to 10/10. The 16 new failure-order cases pass 10/10. The five that put a failed read newest fail 0/10 on the parent, showing the bug, and pass after.
- **BUG-18 is fixed.** On keyboard focus, the process sheet's Close button now paints a solid 2 px `--ring` outline with a 2 px offset, in WebKit and Chromium, with the OS in dark and in light. The sheet is still 440 px wide. Other shared Buttons look the same on focus, pixel for pixel.
- **Pixels:** 428 screen pairs were compared. 415 are byte-identical. 12 differ only by the new Close outline, and 1 differs only by Chat's run-timed "Waiting N min".
- **New findings:** none.

| Bug | Before (282ce3c) | After (2c2d540) | Verdict | Code |
| --- | --- | --- | --- | --- |
| BUG-48 · a failed newer read discards an older read's newer answer | `observe/newest-read-fails` 0/10: the row stays Online, not Stopped | 10/10 Stopped in the row, the accessible name, the live region and the sheet | **Fixed** | `offices/+page.svelte:80-86` (`const revision = statusRevision; const request = ++refreshSequence;` … `request > successfulRefresh` → `successfulRefresh = request`) |
| BUG-18 · the Close button has no focus outline | `outline-style: none` (2 px, offset 2 px, `--ring` colour, all cancelled) | `outline-style: solid`, 2 px, offset 2 px, colour = `--ring` | **Fixed** | `ProcessSheet.svelte:99` (adds `focus-visible:outline-solid`) |

## BUG-48: status read order

### How it was tested

- **Harness:** `race.mjs`, copied from `validation-office-order-shots/race.mjs`.
  - The hook now wraps the test-owned fake at `/e2e/fakes/office-api.ts`. F1-03 moved the fake out of `src/`.
  - Each read takes a directive: hold it, change its answer, or make it fail.
  - Reads start the app's own ways: entering Offices, the 30 s check (Playwright clock `fastForward`), Restart, or a reload. Pushes go through `__mockOfficeStatus`.
- **Each run checks four places:** the row (badge and line), the row button's accessible name, the polite live region (`live-status`) and, where it's open, the details sheet. The row is also sampled every 25 ms for 500 ms, to catch any flip back.
- **Setup:** WebKit, 1440×900, dark, 10 runs per case, both builds at once.
- **Harness errors:** none this time. The earlier run's four "execution context destroyed" flakes didn't recur.

### Race table (passes / runs)

| Case | Before 282ce3c | After 2c2d540 |
| --- | --- | --- |
| reads/older-after-newer × 9 statuses (stopped, restartPending, starting, keyMissing, noReason, updateNeeded, unknown, invited, online) | 10/10 each | 10/10 each |
| reads/in-order | 10/10 | 10/10 |
| reads/three-reverse | 10/10 | 10/10 |
| push/older-read-after-push × 9 statuses | 10/10 each | 10/10 each |
| push-between/newer-first | 10/10 | 10/10 |
| push-between/older-first | 10/10 | 10/10 |
| push-between/read-after-push-is-newer | 10/10 | 10/10 |
| push-between/push-during-both | 10/10 | 10/10 |
| push-between/push-then-reads-reversed | 10/10 | 10/10 |
| push-between/other-org-push | 10/10 | 10/10 |
| restart/initial-read-in-flight | 10/10 | 10/10 |
| restart/background-read-in-flight | 10/10 | 10/10 |
| restart/restart-read-overtaken | 10/10 | 10/10 |
| reload/held-read-dropped | 10/10 | 10/10 |
| reload/older-after-newer | 10/10 | 10/10 |
| **observe/newest-read-fails** | **0/10** (Online) | **10/10** |
| storm/six-reads-shuffled (extra, 33rd) | 10/10 | 10/10 |
| **32 earlier cases + storm** | **320/330** | **330/330** |

### New failure orders

Reads are numbered in plan order: 0 is entering Offices, then one per 30 s check. The sheet is open in every case.

| Case | Setup | Expected | Before | After |
| --- | --- | --- | --- | --- |
| fail-between/321, 123, 213, 231, 132, 312 | 1 Stopped, 2 fails, 3 Starting; each case releases them in its named order | Starting (3 is the newest successful read) | 10/10 each | 10/10 each |
| **fail-newest/older-goods-reversed** | 1 Stopped, 2 Starting, 3 fails; released 3, 2, 1 | Starting (1 must not revert 2) | **0/10** (Online) | 10/10 |
| **fail-two/321**, **fail-two/231** | 1 Stopped, 2 and 3 fail; good read last | Stopped | **0/10** each (Online) | 10/10 each |
| fail-two/all-fail | 0, 1 and 2 all fail | Online (saved status kept) | 10/10 | 10/10 |
| fail-push/older-good-after-push | 1 stale Online, push Stopped, 2 fails; released 2, 1 | Stopped (the push beats the older read) | 10/10 | 10/10 |
| **fail-push/good-after-push-newest-fails** | push Stopped, then 1 Starting and 2 fails; released 2, 1 | Starting (1 is newer than the push) | **0/10** (Stopped) | 10/10 |
| fail-push/push-during-failed-read | 1 fails while a Key missing push arrives | Key missing | 10/10 | 10/10 |
| fail-push/push-during-both | 1 Online and 2 failing, both pending, when an Update needed push arrives | Update needed | 10/10 | 10/10 |
| fail-push/push-after-failure-then-older-good | 1 Stopped pending; 2 fails; push Starting; then 1 finishes | Starting | 10/10 | 10/10 |
| fail-push/failure-push-newer-read | 1 fails, push Stopped, then 2 Online | Online (2 is newer than the push) | 10/10 | 10/10 |
| **16 new cases** | | | **110/160** | **160/160** |

On the parent, the failures are exactly the cases where a failed read is the newest. They hold the stale status in every place checked, for example:

- **fail-newest:** row "Online 7 agents", accessible name "…, Online, 7 agents, opens details", live region "HQ: Online", sheet Status "Online".
- **fail-push/good-after-push-newest-fails:** the push's "Stopped" stays where "Starting" is expected.

After the fix, every case agrees in all four places, with no flip back in the 500 ms sample.

### What still holds after the fix

Every earlier ordering guarantee still passes 10/10:

- An older read never reverts a newer successful read.
- An older read never reverts a newer push.
- A newer read beats an older push.
- A push for another organization changes nothing.
- The Restart and reload paths hold.

Two code details:

- The merge also adds `statusRevision += 1` on an organization change (`+page.svelte:28`), which fences reads from the previous organization.
- Avery's new e2e also covers a successful empty result fencing older reads (`office-refresh-newer-empty-result`).

I didn't test an organization switch with a read in flight; no flow in the race harness switches organization.

### Other Offices checks

`blitz.mjs` covers owner jobs and attempts to break Offices. 64 probes ran on each build; 61 match exactly. The 3 that differ are harness noise, not product changes:

- `job-restart-took-ms`: 825 → 875 ms (timing).
- `reason-inUse`: the countdown reads "55 s" before and "54 s" after (timing).
- `kbd-chromium-sheet-sequence`: one step reads the second "Restart office" ring as `shadow-ring` before and `NONE` after.
  - That probe reads the style straight after Tab, while the Button base's `transition-all` is still running.
  - Measured once the transition ends (`focus.mjs` waits for `getAnimations()`), Restart office and the office sheet's Close have identical styles and pixels in both builds (below).

### Avery's evidence

The shots in `app-avery-processes/apps/org-ui/playwright-report/office-failure-1a837b7` agree with my runs. For example, `office-failed-newer-failure-stopped-context.png` shows the sheet's Status as "Stopped" with "It stopped unexpectedly".

## BUG-18: the Close button's focus ring

- **Method:** `focus.mjs`, adapted from `validation-processes-batch-shots/bugs.mjs`.
- **Keyboard focus:** a process row is focused and Enter opens the sheet, so focus lands on Close. A Tab (Alt+Tab in WebKit) keeps it there, since Close is the sheet's only button.
- **Measurement:** the computed style is read once transitions have finished, and compared with `var(--ring)` resolved on a probe element.
- **Shots:** `deviceScaleFactor: 4`, clipped to the button plus 10 px.
- **Matrix:** 2 builds × WebKit and Chromium × OS dark and light, at 1440×900.
- **Theme note:** the app is dark-only (`app.html` sets `class="dark"` and `color-scheme: dark`), so OS light renders the same dark sheet. Light-mode values and pixels equal the dark ones.

| Close button, keyboard focus | Before (282ce3c) | After (2c2d540) |
| --- | --- | --- |
| `:focus-visible` | true | true |
| outline-style | **none** | **solid** |
| outline-width | 2px | 2px |
| outline-offset | 2px | 2px |
| outline-color | `oklch(0.585 0.233 277.117)` | `oklch(0.585 0.233 277.117)` (= `--ring` in both browsers) |
| box-shadow ring | 0 px (`ring-0`) | 0 px (`ring-0`) |
| border | 1 px `--ring` | 1 px `--ring` (unchanged) |
| button / sheet width | 32 px / **440 px** | 32 px / **440 px** |

These values are the same in WebKit and Chromium, with the OS in dark or light, both when the sheet opens and after Tab.

**4× shots:**

- The after shots show a solid ring 4 CSS px outside the button: 2 px offset plus 2 px width.
- The before-vs-after diff is a box at [24,24]–[183,183] device px in a 208×208 clip, which is CSS 6–46 around a button at 10–42.
- It covers 4,616 changed device px in WebKit and 4,591 in Chromium, the same in dark and light. That is the ring's area: (40² − 36²) × 16 = 4,864, less the rounded corners.
- The before shots show only the 1 px border.

**Other shared Buttons:** none changed. All were reached with Tab (Alt+Tab in WebKit), and compared on computed style and 4× pixels:

| Button | Variant / size | Before vs after |
| --- | --- | --- |
| Office sheet Restart office | default | identical style, 0 px diff |
| Office sheet Close | outline | identical style, 0 px diff |
| Org chart team actions menu | ghost, icon-sm (same variant and size as the process Close) | identical style, 0 px diff |
| Project New task | default, with icon | identical style, 0 px diff |
| New task on pointer press | no `:focus-visible`, no ring | identical in both builds |

That's 16 shot pairs, all byte-identical. The shared look stays as it was in both builds: outline none, plus a 3 px `ring/50` box-shadow and a 1 px `--ring` border. The diff touches only `ProcessSheet.svelte:99` and leaves `button.svelte` alone.

## Pixel comparison of the main screens, Processes and Offices

- **Method:** `validate.mjs`, copied from the processes-batch run, with flows `office,process,main`.
- **Setup:** WebKit, 1440×900 and 960×640, OS dark and light, clock at 2026-11-10T12:00Z, a fresh context per flow.
- **Totals:** 107 states × 4 runs = **428 pairs. 415 are byte-identical and 13 differ.** No step failed.
- **Console:** both builds logged the same 3 messages per run, all on the deliberate 404 step (`31-not-found`).

| Pair(s) | Diff px | Why |
| --- | --- | --- |
| p44-kbd-enter-opens-sheet, p45-kbd-tab-in-sheet, p47-kbd-space-opens-turned-off × 4 runs (12 pairs) | 330 each, box 40×40 at the Close button (x 1388–1427 at 1440, 908–947 at 960; y 12–51) | BUG-18 fix: these are the keyboard-opened process sheets with Close focused. The only behaviour difference is `outline none → solid 2px`; the text, focus, URL and overflow are the same |
| 960x640-light/20-chat | 751, box [618,282]–[723,490] | Run timing: "Waiting 9 min" → "Waiting 8 min" and "Waiting 12 min" → "Waiting 11 min". The same known effect as in the processes-batch report; not from this merge |

Every Offices state (o-steps), every other Processes state and the rest of the main screens are byte-identical.

## Local checks on 2c2d540

These ran in a separate clean worktree at 2c2d540, each wrapped in `~/.agency/bin/heavy`.

- `bun run check`: `COMPLETED 1158 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS` (exit 0)
- `bun run test`: `Test Files  50 passed (50)` / `Tests  1330 passed (1330)` (exit 0)
- `PW_PORT=4293 bun run test:e2e --workers=2`: `262 passed (7.2m)` (exit 0), including OFC-12 / BUG-48 (both orders) and PROC-18 / BUG-18

## New findings

None. The pre-existing focus and Offices items are already catalogued (BUG-23, BUG-33, BUG-53 and others) and aren't repeated here.

## Method and files

- **Worktrees:** `git worktree add --detach` at 282ce3c (`before`) and 2c2d540 (`after`, plus `checks` for the suites) under the session scratchpad `bug48/`. All are removed now, and the dev servers are stopped.
- **Dev servers:** `bun run dev --mode e2e` on 5243 (before) and 5244 (after).
- **Scripts:** all in `validation-bug-48-18-shots/`. The originals are unedited.
  - `race.mjs`: the 33 earlier cases plus section 8, the 16 new failure orders.
  - `focus.mjs` and `focusdiff.mjs`: BUG-18.
  - `validate.mjs`: the screen pixels.
  - `blitz.mjs`: the Offices blitz.
- **Results:**
  - `race-{before,after}.json` and `.log`, with first-run shots in `race/`.
  - `focus.json`, `focusdiff.json`, `focus/` (4× shots) and `focus-diff/`.
  - `results.json`, `summary.json`, `validate-run.log`, `before/`, `after/` and `diff/`.
  - `blitz-{before,after}.json` and `.log`.
  - `check-after.log`, `unit-after.log` and `e2e-after.log`.
