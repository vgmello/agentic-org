# Validation · office refresh-order fix (v1 5068a6a vs e35db5e)

**Verdict:** the read order is fixed, and nothing else changed. One new S4 edge case (O1).
- **Race (WebKit, 32 cases × 10 runs per build):** before, 16 cases fail 10 of 10. These are the cases where an older read finishes after a newer one: every status, reverse order, a shuffled six-read storm, a push followed by out-of-order reads, restart with a read in flight, and reload. After, 32 of 32 cases pass. The race spec with `--repeat-each=10`: after `30 passed (1.4m)`; before `10 failed`, `20 passed`.
- **Pixel compare:** 245 of 248 pairs are byte-identical, including all 124 Offices pairs. The other 3 are the Chat clock. Avery's 5 captures are identical to mine.
- **Local checks:** check `COMPLETED 1180 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`, unit `Tests  1206 passed (1206)`, e2e `230 passed (6.3m)`, run through heavy with 2 workers.

**O1 · S4 · new in 5068a6a, confirmed by me in the code.** A newer read that fails throws away an older read's newer answer. `refreshOffices()` now takes `const revision = ++statusRevision` when it starts, and applies a result only if `revision === statusRevision`. When check N+1 starts, it bumps the revision; if it then fails, check N's later result (for example Stopped) is dropped. The status stays at the old value (Online) until the next check or push: 10 of 10 runs, race case `observe/newest-read-fails`. The before build shows Stopped. Fix: on failure, don't count the read as the newest, or track the newest successful read separately.

Nothing else is new. BUG-23..27, BUG-18 and BUG-19 are still there; BUG-19's fix is in F1-03, not in this merge.

Scripts and screenshots: `workspace/validation-office-order-shots/` (not committed). The full working report follows.

---

# Validation: office refresh order (5068a6a), a follow-up to BUG-5

**Verdict: the fix works and changes nothing else. In the before build, an older status read that finishes late reverts the status in every run (0 of 10 in each of 16 read-order cases). In the after build all 32 cases pass 10 of 10. Nothing else changed: 245 of 248 screenshot pairs are byte-identical and all 124 Offices pairs are identical. The 3 pairs that differ are Chat's "Waiting N min", which depends on run timing. I found one new S4 edge (O1): if the newest read fails, an older read that brings newer news is thrown away until the next check.**

- Change: merge `5068a6a` ("Merge avery/office-refresh-order: newest-started status read wins") on `v1`, compared with its first parent `e35db5e`.
- Diff: `routes/(shell)/offices/+page.svelte` changes 2 lines. `refreshOffices()` now takes `++statusRevision` instead of `statusRevision`, so every read, not only a push, makes older reads stale. The comment was updated to match. Separately, `e2e/office-status-refresh.spec.ts` gains 71 lines with one new test, "a delayed probe refresh cannot overwrite a newer restart refresh without a push".
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. Both worktrees are removed and both dev servers (5213 before, 5214 after) are stopped.

## Summary

| Check | Before (e35db5e) | After (5068a6a) |
| --- | --- | --- |
| Race, 32 cases × 10 runs (WebKit) | 16 cases pass 10/10. **16 fail 10/10**: every read-order case | **32 / 32 cases pass 10/10** (320 runs) |
| Race spec, `--repeat-each=10` (Chromium) | **`10 failed`, `20 passed (1.5m)`**: the new test fails 10 of 10 | **`30 passed (1.4m)`** |
| Pixels, 62 states × 4 runs (248 pairs) | | **245 byte-identical**. All 124 Offices pairs are identical. 3 Chat pairs differ by run timing (§2) |
| Behaviour per step (URL, text, focus, alerts, overflow) | | Same in all 248 steps, apart from the Chat text above |
| Avery's 5 captures vs mine (e2e, Chromium) | | **5 / 5 byte-identical** |
| Blitz (64 probes), a11y (20 states) | | Identical, except the restart duration (874 ms vs 851 ms) |
| check / unit / full e2e (after) | | `0 ERRORS 0 WARNINGS` / `1206 passed` / **`230 passed (6.3m)`** |

## 1. The race between reads

**Method** (`validation-office-order-shots/race.mjs`, adapted from the BUG-5 `race.mjs`; WebKit, 1440×900, dark):

- The script wraps the page's own `officeApi.peekOrganizations`, the same dev-module instance the e2e patches.
- Each read gets its own directive:
  - `answer`: the HQ record that read returns, as if the host computed it at that moment.
  - `hold`: keep the answer until the script releases it.
  - `fail`: make the read reject.
- Reads start the app's own ways:
  - **initial**: entering Offices, or a reload;
  - **30 s check**: Playwright's clock is installed with time flowing, and `clock.fastForward(30 s)` fires the interval;
  - **restart**: the details sheet's Restart office (`onrestarted`).
- Pushes go through `window.__mockOfficeStatus`.
- Each run checks four places:
  - the row's badge and secondary line;
  - the row button's accessible name;
  - the polite live region;
  - the details sheet's Status, when it's open.
- I watched the row every 25 ms for 500 ms after the last release, to catch a flip back.

**"Every status"** means these nine:

- Online
- Stopped/crashed
- Stopped with a restart pending (`offline` + `next_restart_at`)
- Starting
- key missing
- Stopped with no reason
- Update needed
- `unknown` (Status unknown)
- `pending` (Invited)

| Case (10 runs each) | Must read | Before | After |
| --- | --- | --- | --- |
| **Older read after newer read**, no push: the initial read is held (stale), and the 30 s check answers first with the new status. Sheet open. ×9 statuses | the newer read's status | **0/10 each** (reverts to the stale status in the row, name, live region and sheet) | 10/10 each |
| Three reads finish newest first (Online, Starting, key missing) | key missing | **0/10** (reads Online) | 10/10 |
| Storm: six overlapping reads (initial + five 30 s checks), released in 10 shuffled orders. Sheet open | the newest one, Status unknown | **0/10** | 10/10 |
| Control: two reads finishing in order | the newer one | 10/10 | 10/10 |
| **Older read after a push**: initial read held, push, release. Sheet open. ×9 statuses | the pushed status | 10/10 each | 10/10 each |
| Push between reads: read0 held, push Stopped, read1, release read1 then read0 | Stopped | 10/10 | 10/10 |
| … released read0 then read1 | Stopped | 10/10 | 10/10 |
| … read1 (started after the push) has newer news, Starting | Starting | 10/10 | 10/10 |
| … the push arrives while both reads are pending | the push (key missing) | 10/10 | 10/10 |
| **A push, then two reads started after it finish out of order** | the newer read (Update needed) | **0/10** (the older read's Stopped stays) | 10/10 |
| Control: a push for another organization between reads | the read applies (Stopped) | 10/10 | 10/10 |
| **Restart with the initial read in flight** (`?office=crashed`): Restart's refresh answers Online, then the held initial read lands | Online | **0/10** (Stopped and **Restart office** come back; `race/before-restart-initial-read-in-flight.png`) | 10/10 |
| **Restart with a 30 s check in flight** | Online | **0/10** | 10/10 |
| **Restart's own read overtaken**: Restart's read is held, and a newer 30 s check answers Online first | Online | **0/10** (reads Stopped. By the code, the restart loop polls again 2 s later and heals it) | 10/10 |
| Reload while a read is held: its answer never lands | the host's status (Online) | 10/10 | 10/10 |
| **Reload, then the first read finishes after the 30 s check** (hook installed before the first read: 10 of 10 in both builds) | the newer read (Stopped) | **0/10** | 10/10 |

- **Before:** an older read finishing late always wins, unless a push arrived while it was pending. That's the exact gap the BUG-5 fix left, and the new e2e test covers it.
- **After:** no run flipped back. In every run the row, the accessible name, the live region and the sheet agree.
- **Harness note:** the first after run had 4 runs abort with "Importing a module script failed". These were Vite reloads while my full e2e started a second dev server on the same worktree, not assertion failures. I re-ran those cases, plus the storm case, with no other load: 10/10 each (`race-after-rerun.json`). The before build had one such abort too, and its rerun is `race-before-rerun.json`.
- **Not tested:** "Unreachable" isn't a pushed status, and no read runs while the check is down (BUG-5 validation). I didn't repeat it here, because the merge doesn't touch the probe.

### O1 · S4 · new in 5068a6a · a failed newer read discards an older read's newer answer

- **Repro** (`race.mjs` case `observe/newest-read-fails`):
  1. Open Offices with the host Online.
  2. A 30 s check's read starts, and the host's answer is Stopped. Hold it.
  3. The next check's read starts and fails, for example with a transient `not_connected`.
  4. Release the first read.
- **Actual:** in the after build, the row, name and live region stay **Online** (10/10). The Stopped answer is dropped because a newer read was started, even though that newer read produced nothing. It corrects itself at the next 30 s check or the next push.
- **Before:** Stopped shows (10/10).
- **Expected:** "an older read that finishes late must never revert the status". This answer doesn't revert anything: it's newer than what's shown. So it should apply, or at least the failed read shouldn't make it stale. OFC-12 asks that stale status is never shown as Online.
- **Impact:** low. It needs a failed peek overlapping a slower one, and the host's live push normally carries the change anyway.
- **Possible direction (engineering's call):** compare against the revision of the last read that applied, plus a push counter, instead of the newest read started.
- **Evidence:** `race-after.json` / `race-before.json`, case `observe/newest-read-fails`.

## 2. No other change: pixel comparison

`validation-office-order-shots/validate.mjs` is the BUG-5 script with only its paths and ports changed:

- **Builds and settings:** WebKit, both builds, at 1440×900 and 960×640, in dark and light. The clock is fixed and every scroller is shot a second time at its top.
- **The 31 Offices states (o01–o72):**
  - Checking…, Online, and the details sheet;
  - pushed Stopped, Starting, Update needed, key missing and back to Online;
  - crashed through Restart office to Online;
  - key missing, no reason, restart pending;
  - `not_ready` and `stuck`;
  - Unreachable;
  - a count error and the count skeleton;
  - the keyboard path.
- **The 31 F1-01 main screens.**

**Result: 248 pairs, 245 byte-identical. The 124 Offices pairs are all identical.** Three pairs differ:

| Pair | Pixels | Why |
| --- | --- | --- |
| `default-dark/20-chat` | 751 | "Waiting 9 min" → "8 min" and "Waiting 12 min" → "11 min" (Needs you) |
| `default-light/20-chat` | 751 | same |
| `960x640-dark/20-chat` | 751 | same |

- `messaging/sample.ts` sets the sample's times from the moment the sample is made, and the view's "now" moves on from there. So the minute count depends on how long the run took and can cross a minute boundary.
- The fourth Chat pair (`960x640-light`) is identical.
- The merge touches only Offices.
- The diff images are in `diff/*/20-chat.png`.
- **Console:** the same in both builds. The only errors come from the deliberate 404 in `31-not-found`.

**Avery's captures:** the five e2e screenshots in `app-avery-processes/apps/org-ui/playwright-report/office-order-5068a6a/` are byte-identical to the ones my full e2e run produced at 5068a6a (`avery-compare/`). The brief's path, `app-avery/…`, doesn't exist. The folder is in `app-avery-processes`, and its `previous-avery-worktree/office-order-5068a6a` copy is identical too.

## 3. Blitz and a11y on Offices

`blitz.mjs` and `a11y.mjs` ran on both builds. They covered:

- the owner jobs;
- every reason;
- 300 rapid pushes on the list and in the sheet;
- restart with pushes in the middle;
- back, forward and reload;
- resizing to 960 and back;
- the keyboard path in WebKit and Chromium;
- contrast and landmarks.

**Results:**

- **Blitz:** 63 of 64 probes are identical. The one that differs is the restart's duration (874 ms vs 851 ms).
- **a11y:** the JSON is identical.
- **Owner job:** restart works (Restarting… → Online in 0.85 s).
- **Pushes:** every rapid-push round ends on the last push.
- **Page errors:** none.

I found nothing new. The already-catalogued items still show and aren't re-reported here: BUG-23 (focus drops to the body while restarting and after a refusal), BUG-24 (unsupported_version copy), BUG-25 ("Shown from this Mac." at 3.03:1), BUG-26 ("Restarting in 48 s" for 45 s), BUG-27 (two "Close" buttons), BUG-18 (sheet width and ring) and BUG-19 (landmarks are still only `nav "Main"` and `section`, with no main). BUG-19 is marked fixed in the catalogue, but the fix isn't in 5068a6a.

**Requirements (§3, OFC-11..13):** the after build meets the live-status rules better than before. A status the host confirmed, by a push or a newer read, is no longer replaced by an older read: §3.3 ("the office's status updates live") and OFC-12 ("stale status … never stale Online"). The restart flow (§3.3 "Restarting… with a spinner, then the live status") no longer brings Stopped and Restart office back after a successful restart. OFC-13 is unchanged: the name, live region and icons are identical in both builds. Every other OFC-11..13 gap is as in the BUG-5 validation (BUG-23..27).

## 4. Local checks (after build, 5068a6a)

- `~/.agency/bin/heavy bun run check`: `COMPLETED 1180 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
- `~/.agency/bin/heavy bun run test`: `Test Files  55 passed (55)` / `Tests  1206 passed (1206)`
- `PW_PORT=4214 ~/.agency/bin/heavy bun run test:e2e --workers=2`: `230 passed (6.3m)`
- `office-status-refresh.spec.ts --repeat-each=10 --workers=2`:
  - **after:** `30 passed (1.4m)`.
  - **before:** I copied the 5068a6a spec into the before worktree temporarily and then restored it. The result was `10 failed` and `20 passed (1.5m)`. All 10 failures are the new test, which received "It stopped unexpectedly / Stopped" where it expected Online. The two BUG-5 tests pass 10/10 in both builds.
- **Tooling note:** for about 3 minutes around 18:11, `~/.agency/bin/heavy` crashed with a `RecursionError` whenever it had to queue, because `warn()` called itself. It was fixed on disk at 18:14, before I ran any suite through it.

## Paths

Everything is under `workspace/validation-office-order-shots/`:

- **Scripts:** `race.mjs` (rewritten for read order), `validate.mjs`, `blitz.mjs` and `a11y.mjs` (paths and ports adapted). The originals in `validation-bug-5-shots/` are untouched. The scripts import Playwright from the removed worktree: recreate `…/scratchpad/office-order/after` to re-run them.
- **Race:** `race/` (one shot per case and build), `race-before.json`, `race-after.json`, `race-*-rerun.json` and their logs.
- **Pixels:** `before/`, `after/`, `diff/`, `results.json`, `summary.json`, `validate-run1.log`.
- **Blitz and a11y:** `blitz-before/`, `blitz-after/`, `blitz-*.json`, `a11y-*.json` and their logs.
- **Checks:** `check-after.log`, `unit-after.log`, `e2e-after.log`, `e2e-after-repeat.log`, `e2e-before-repeat.log`.
- **Avery comparison:** `avery-compare/mine` and `avery-compare/avery`.
