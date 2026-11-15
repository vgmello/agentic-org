# Validation · BUG-5 fix, office status race (v1 705158f vs 49348ee)

**Verdict:** BUG-5 is fixed, and nothing else changed.
- **Race:** before, 0 of 100 race runs pass; after, 100 of 100. The pushed status (Stopped, restart pending, Starting, key missing, Update needed, Online, five pushes in a row, a push after a reload) used to flip back to the older status in the row and in the details sheet. Now the newest status always shows. Controls pass 18 of 18 in both builds.
- **Pixel compare:** 247 of 248 pairs are byte-identical, including all 124 Offices pairs. The other is 12 px of animation on the Projects New task dialog.
- **E2E:** after, `226 passed (5.6m)`. The new race test, run 10 times on the before build, fails 10 of 10, so it proves the fix. The old OFC-11 test passes in both builds, so it never caught the race.
- **Requirements:** OFC-11 and OFC-12 met (except `unsupported_version`); OFC-13 met. OFC-1, 9 and 10 partly met (phase 1 has HQ only). OFC-2 to OFC-8 aren't built (Add office: "Available in a later release.").

**Pre-existing issues (the same in both builds), checked by me, sent to ada 10 Nov:**
1. S3 a11y. In the details sheet, Restart office loses keyboard focus to the page body while it reads "Restarting…" and after a refusal. Code: `OfficeDetailsSheet.svelte:149` uses `disabled={restarting}`, whereas the row's Retry uses `aria-disabled` and keeps focus. Shot: `blitz-after/kbd-webkit-after-refused-restart.png`.
2. S3 copy and action. For `unsupported_version`, the app says "Parts of Agentic Org on this Mac don't match. Update or reinstall…" (`offices/model.ts:173`) and offers only Copy details. Spec §3.3 table: "The release components on <office machine / instance> don't match. Run diagnostics and repair the selected instance there.", with **Show local CLI repair instructions** · Copy details.
3. S3 contrast. "Shown from this Mac." is `text-gray-500` at 12 px on the page background, about 3.0:1. The design system needs 4.5:1. Code: `offices/+page.svelte:251`. Shot: `after/default-dark/o02-online.png`.
4. S4. Right after a push, the countdown overstates. A restart 45 s away reads "Restarting in 48 s" until the next 10 s tick (from the run's timing; the shot `blitz-after/reason-pending-sheet.png` shows 48 s).
5. S4 a11y. The office sheet has two buttons named "Close": the sheet's default × (`showCloseButton` left on) and the footer Close (`OfficeDetailsSheet.svelte:152`).

**Dropped after checking:** the reviewer's note about the announcement wording ("HQ: Online" against "Office 2 is online"). The spec gives no wording for status announcements; it says only that status changes are announced politely (§6.1). BUG-18 (sheet width and focus ring) and BUG-19 (no main landmark) also apply to Offices, but they're already catalogued.

Screenshots and scripts: `workspace/validation-bug-5-shots/` (not committed). The full working report follows.

---

# Validation: BUG-5, a pushed office status survives the initial refresh

**Verdict: BUG-5 is fixed, and the merge changes nothing else. I reproduced the race in every before run (0 of 100 race runs pass in 49348ee) and it never happened after the fix (100 of 100 race runs pass in 705158f, plus 18 of 18 control runs in each build). 705158f introduces no new problems. The findings below were all already in 49348ee and none of them are catalogued yet.**

- Change: merge `705158f` ("Merge avery/bug-5-office-status into v1") on `v1`.
- Compared with its first parent `49348ee` ("Merge quinn/b3-02-spawn into v1").
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. Both throwaway worktrees are removed and both dev servers are stopped.
- Source diff (2 files):
  - `routes/(shell)/offices/+page.svelte`, 13 lines. A `statusRevision` counter goes up on every push that changes this organization's list. `refreshOffices()` applies its answer only if no such push arrived while it was pending.
  - The new `e2e/office-status-refresh.spec.ts`.

## Summary

| Check | Before (49348ee) | After (705158f) |
| --- | --- | --- |
| Race, push during the initial refresh (100 runs) | **0 / 100 pass** (the refresh wins every time) | **100 / 100 pass** |
| Controls: push after the refresh, reload with the host stopped, Unreachable (18 runs) | 18 / 18 | 18 / 18 |
| Pixel comparison, 62 states × 4 runs (248 pairs) | | **247 byte-identical.** All 124 Offices pairs are identical. The one that differs is 12 px of animation timing on Projects |
| Behaviour per step (URL, text, focus, alerts, overflow) | | No difference in any of the 248 steps |
| Blitz (64 probes) and a11y (20 states) | | Identical, except the restart's duration (826 ms vs 833 ms) |
| New spec + OFC-11, `--repeat-each=10` | **10 failed, 20 passed**: the new race test fails 10 of 10 | **30 passed (1.2m)** |
| Full e2e suite | | **`226 passed (5.6m)`** |

## 1. The fix: the race, before and after

**Method** (`validation-bug-5-shots/race.mjs`, WebKit 1440×900, dark):

- The script wraps the page's own `officeApi.peekOrganizations`, the same dev-module instance the BUG-5 e2e patches.
- **Natural** cases only watch the real 300 ms mock peek and push through `window.__mockOfficeStatus` at 10, 60, 120, 180, 240 or 280 ms into it. The log confirms that each push landed inside the pending peek (60 of 60 single-push runs).
- **Held** cases hold the peek's answer, as the e2e does, while the details sheet is open.
- The mock's push doesn't change the mock host's record, so the peek answers with the older state, as a peek computed before the change would.
- Each case is checked in four places:
  - the row's badge and secondary line
  - the row button's accessible name
  - the polite live region (`role=status`)
  - the details sheet, when it's open
- The office status appears nowhere else. The sidebar footer and organization switcher say "This Mac · HQ office · Switch organization", with no status.

| Case (runs) | Pushed status: must read | Before | After |
| --- | --- | --- | --- |
| natural/stopped (10) | Stopped · "It stopped unexpectedly" | 0/10 (reads Online) | 10/10 |
| natural/restartPending (10) | Stopped · "…Restarting in N s" (`offline` + `next_restart_at`) | 0/10 | 10/10 |
| natural/starting (10) | Starting | 0/10 | 10/10 |
| natural/keyMissing (10) | Stopped · "This office's key isn't available…" | 0/10 | 10/10 |
| natural/updateNeeded (10) | Update needed | 0/10 | 10/10 |
| natural/online (10): host record says crashed, push says Online | Online | 0/10 (stale Stopped comes back) | 10/10 |
| natural/five-pushes (10): Starting → Stopped → Online → restart pending → key missing | the newest one, key missing | 0/10 (reads Online) | 10/10 |
| held-sheet/stopped (5) | row, label, live region and sheet all read Stopped | 0/5 (all four revert to Online) | 5/5 |
| held-sheet/restartPending (5) | Stopped everywhere | 0/5 | 5/5 |
| held-sheet/starting (5) | Starting everywhere | 0/5 | 5/5 |
| held-sheet/three-pushes (5): Starting → Online → Stopped | Stopped everywhere | 0/5 | 5/5 |
| reload/push-in-first-peek (10): reload, hook installed before the first peek (10 of 10), push at 60 ms | Stopped | 0/10 | 10/10 |
| after-refresh/stopped (10), control | Stopped | 10/10 | 10/10 |
| reload/host-stopped (5), control: host record and push agree, then reload | Stopped after the reload | 5/5 | 5/5 |
| unreachable/pushes-ignored (3): `?mock=unreachable`, push Online, Stopped and Starting | stays Unreachable, never a stale Online (OFC-12) | 3/3 | 3/3 |

- **Before:** the pushed status shows for a moment and then flips back. Sampled every 25 ms, the sequence was, for example, `Checking… → Stopped → Online`.
- **After:** no run flipped back. The newest pushed status is the last one shown in every run.
- **Unreachable is not a pushed status.** It comes from the app's own check, and while that check is down no peek runs (0 peeks recorded).
- The rapid-push blitz agrees: 5 rounds of 60 pushes 5 ms apart, list and sheet, 10 of 10 end on the last push in both builds. With no refresh pending, the race can't occur.

**Mock behaviour, not a product bug:** the mock's test hook doesn't write the host's record. So a push followed by navigating away and back, or by a reload, shows the host's own status again (Online). That's correct for the mock. When the host record agrees with the push, the status survives the reload (reload/host-stopped, 5 of 5).

**Residual risk (no finding):** the fix drops a whole peek answer if any push for the organization arrives while it's pending. With HQ alone that's exactly right. With several offices, newer information about other offices in that answer waits up to 30 s, until the next check. That check applies normally, because no push arrived during it.

Screenshots (`validation-bug-5-shots/race/`):

- `before-held-sheet-stopped.png` shows the bug: Stopped was pushed while the sheet was open, and the row and sheet both read Online.
- `after-held-sheet-stopped.png`, `after-natural-*.png`, `after-reload-push-in-first-peek.png` and `after-unreachable-after-pushes.png` show the after build.

## 2. No other change: pixel comparison

`validation-bug-5-shots/validate.mjs`, adapted from F2-01b's script:

- **Builds and settings:** both builds, WebKit, at 1440×900 and 960×640, with the OS in dark and in light.
- **Stabilising:** the clock is fixed, finite animations are allowed to finish, and every scroller is captured a second time at its top.
- **31 Offices states** (o01–o72), each flow in a fresh context:
  - Checking… and Online, the details sheet open and closed
  - pushed Stopped, Starting, Update needed and key missing, and Online again with the sheet open
  - crashed, its sheet, Restart office through to Online
  - key missing with Try again, and no reason ("It isn't running")
  - restart pending (countdown) and its sheet
  - restart answered `not_ready` (the error, then the button comes back), and `stuck` (Restarting… stays)
  - Unreachable with Retry, and its sheet
  - a failed agent count and its sheet, and the count skeleton
  - the keyboard path: row focus, Enter opens the sheet, Esc returns to the row
- **The 31 F1-01 main screens** (01-welcome to 31-not-found).

**Result: 62 × 4 = 248 pairs, of which 247 are byte-identical PNGs. All 124 Offices pairs are identical.** The scroll-normalised copies agree.

One pair differs:

- `default-dark/16-project-new-task-dialog` (Projects › Beacon › New task) differs by **12 pixels**, at the bottom-right corner of the Title field's ring (`validation-bug-5-shots/diff/default-dark/16-project-new-task-dialog.png`).
- In both builds the step has the same text, the same focus (`input#new-task-title`), the same URL and the same controls.
- This merge didn't touch the screen, and the other three runs of it are identical. It's the dialog's open animation or ring transition, captured a frame apart.

**Console:** the same in both builds. The only errors come from the deliberate 404 (`31-not-found`). There was one Vite "Importing a module script failed" with an HMR reload on the very first page of the before build's first run: dependency pre-bundling on a cold server, not the app. The screenshot still matched.

## 3. Requirements (docs/specs/offices-and-teams-ux.md at 705158f)

Assessed on the browser UI with the TypeScript mock: HQ only, Sample data.

| Criterion | After build | Evidence |
| --- | --- | --- |
| OFC-1 · list with names, location, status, last seen/sync; row opens details | **Partly** | HQ row: name, HQ tag, "This Mac (you're here)", status with icon and word, "7 agents"; the slide-over has Role, Where, Status, Why and Agents here (o02, o03). There's no last-seen, sync, version, Rename or projects. Phase 1 has HQ only (§6.1 build note) |
| OFC-2..8 · Add office, invitations, approve, remove | **Not met (deferred)** | Add office is disabled with "Available in a later release." as its description (§6.1 historical build note); no flows |
| OFC-9 · unreachable / Live updates paused | **Partly** | Unreachable with "Can't reach this office" and Retry, never a stale Online (o50, race unreachable 3/3). There are no satellites, so no last-seen/sync |
| OFC-10 · unplaced agents | **Partly** | "Unplaced · 1 agent" with a disabled Place… and its reason. There's no Offboard… and no needs-you item |
| **OFC-11** · stopped offices show the reason and an authorized remedy; live status | **Met for every reason except `unsupported_version`, which is partly met** | Reasons: crashed, start_timeout, restarts_exhausted, storage_error, in_use, key_missing (Try again), stopped_by_owner (Start office), invalid_request (no restart), an unknown reason and no reason all match §3.3 (blitz `reason-*`). Restart office → Restarting… → Online in 0.83 s; `not_ready` retries, then shows the message and the button comes back; `stuck` keeps the spinner. **Live updates now hold** (§1). `unsupported_version` differs: see P2 |
| OFC-12 · missing or stale live status is Unknown, never stale Online | **Met** (as far as the mock can show it) | Checking… until the probe answers (o01), Unreachable when it doesn't, `unknown` → "Status unknown"; a stale peek can no longer replace a newer push (§1). A host without the `office.status_changed` capability can't be produced on the mock |
| OFC-13 · icon and text for each status; name, role, location, reason and count for assistive tech; no ticking; polite live region | **Met, with S4 copy notes** | Each status has its own Lucide icon plus the word (Online `circle`, Starting `loader`, Stopped `circle-x`, Update needed `circle-arrow-up`, Status unknown `circle-help`, Invited `mail`, Unreachable `wifi-off`). Row label: "HQ, HQ, This Mac (you're here), Stopped: it stopped unexpectedly, restarting shortly, 7 agents, opens details", which never ticks ("restarting at 12:10" when the restart is ≥ 1 min away). Live region `role=status`. See P4 and P5 |
| OFC-14..25 (create organization), OFC-26..38 (joins, waiting) | Not covered by the Offices UI | Out of scope here |
| §6.1 "With HQ alone, a one-line hint under the list" | Not shown | The hint would point at the disabled Add office, so it's acceptable under the phase-1 note |

## 4. Blitz and quality

`validation-bug-5-shots/blitz.mjs` and `a11y.mjs`, both builds. The results are identical apart from timing.

- **Owner jobs:**
  - Seeing the status works.
  - Restarting a crashed office works: Restart office → Restarting… (disabled) → Online. The row, label, live region and sheet all update, and focus goes to Close.
  - Add office is disabled with its reason.
  - Copy details: WebKit refused clipboard reads in the harness, so I relied on the e2e (Chromium), which passes and checks there's no token.
- **Rapid status changes:** 300 pushes in 5 ms steps, with the list and with the sheet open. Every round ends on the last push, with no page errors. A push of Starting during a restart, then the host's Online, ends Online with the spinner gone.
- **Back, forward and reload:** Back with the sheet open leaves the page and leaves no stray dialog. Forward returns with the list. Quick forward and back during the initial refresh gives no errors. After a reload, the host's state shows.
- **Resize, 1440 → 960×640 → 1440, with the sheet open and the longest reasons:**
  - No horizontal page scroll and nothing off-screen.
  - Long reasons truncate in the row (with a `title`) and wrap fully in the sheet.
  - Unreachable with Retry fits at 960.
- **Keyboard:**
  - Tab and Option+Tab reach the row button with a 2 px indigo outline.
  - Enter opens the sheet, Tab stays inside it, and Esc returns focus to the row.
  - Restart office loses focus (P1).
- **Not by color alone:** every badge has the word plus an icon in its status color, and the word is gray-200 in every status.
- **Contrast:**
  - Lowest on the page: 3.03:1 (P3).
  - Next: Add office 4.58, and the 12 px secondary line and "7 agents" at 4.84.
- **Visual tokens:**
  - The status colors follow spec §2: green online, sky starting, red stopped and unreachable, amber update, gray unknown.
  - The sheet is 384 px wide, not the `max-w-md` of spec §2. The button focus ring is 3 px at 50% alpha. Both have the same cause as **BUG-18** (the shared Sheet and Button) and also apply to Offices, so I'm noting them here and not re-reporting them.
- **Landmarks:** the shell has no main landmark (**BUG-19**, already catalogued).

### Findings (all already in 49348ee, none new in 705158f, none catalogued yet)

**P1 · S3 · a11y · Offices › details sheet: Restart office drops keyboard focus.**

- **Repro:** create an organization with `?office=crashed`, open `/offices?restart=not_ready`, open HQ, focus **Restart office** and press Enter.
- **Actual:** focus goes to `body` while the button reads "Restarting…", and it stays on `body` after the refusal shows. The same happens in Chromium and WebKit, because the button becomes `disabled` while it works.
- **Expected:** §3.3 says "While it runs: 'Restarting…' with a spinner", and for `not_ready` "the button comes back". The design system asks for full keyboard use (principle 6, WCAG 2.4.3).
- **Fix:** keep the button focusable while it works (`aria-disabled` plus `aria-busy`, as the row's Retry already does), or move focus to the alert.
- **Screenshot:** `validation-bug-5-shots/blitz-after/kbd-webkit-after-refused-restart.png`.
- **Pre-existing:** `blitz-before.json` is identical.

**P2 · S3 · copy and action · Offices › Stopped with `unsupported_version`.**

- **Actual:** the row and sheet say "Parts of Agentic Org on this Mac don't match. Update or reinstall Agentic Org, then log out and back in." and offer only **Copy details**.
- **Expected:** §3.3 (rev 8): "The release components on <office machine / instance> don't match. Run diagnostics and repair the selected instance there.", with **Show local CLI repair instructions** · Copy details. That makes OFC-11 partly met for this reason.
- **Screenshots:** `validation-bug-5-shots/blitz-after/reason-unsupported-sheet.png` and `validation-bug-5-shots/blitz-after/resize-960-unsupported-sheet.png`.

**P3 · S3 · contrast · Offices footer.**

- **Actual:** "Shown from this Mac." (12 px, `text-gray-500` on the dark ground) measures **3.03:1** in every state at both sizes.
- **Expected:** the design system's "All text is 4.5:1" (WCAG 1.4.3).
- **Screenshot:** `validation-bug-5-shots/after/default-dark/o02-online.png`.

**P4 · S4 · copy · the live region.**

- **Actual:** it announces "HQ: Stopped: it stopped unexpectedly" and "HQ: Online". It also announces the app's own "HQ: Checking…" while the page loads.
- **Expected:** §6.1 gives "Office 2 stopped: it stopped unexpectedly" and "Office 2 is online", and announces "a status change confirmed by its owning instance" only.
- **Evidence:** `blitz-after.json` (`live-region`, `reason-*`).

**P5 · S4 · countdown overstates right after a push.**

- **Actual:** a pushed `next_restart_at` 45 s ahead first reads "Restarting in 48 s", and for `in_use` "Trying again in 54 s". The line uses the clock from the last 10 s tick, not the time of the push.
- **Expected:** §3.3: N from `next_restart_at`. A 10 s update rate is allowed, but the first value shouldn't exceed the real time left.
- **Screenshot:** `validation-bug-5-shots/blitz-after/reason-pending-sheet.png`.

**P6 · S4 · a11y · two buttons named "Close" in the office sheet.** The × button and the footer button have the same name (aria snapshot `aria-sheet-stopped` in `blitz-after.json`). Naming the × "Close details", or dropping one of them, would tell them apart.

Not re-reported (already catalogued): BUG-2, BUG-6..12, BUG-14..20 (BUG-18's sheet width and ring and BUG-19's missing main landmark also apply to Offices).

## 5. The e2e suite (after build, Chromium, `PW_PORT=4197 npx playwright test`)

Final line: **`226 passed (5.6m)`**. This includes both new tests (`office-status-refresh.spec.ts:56`) and `offices.spec.ts` OFC-11 (line 265).

Repeated, `--repeat-each=10`, on the new spec plus `offices.spec.ts:265`:

- **After:** `30 passed (1.2m)`.
- **Before:** I copied the new spec into the before worktree temporarily, then deleted it. The result was `10 failed` and `20 passed (1.4m)`.
  - The race test, "a delayed initial refresh cannot overwrite a newer office status push", failed 10 of 10. So the new test proves the fix.
  - "An unrelated push does not suppress a valid refresh" and the original OFC-11 passed 10 of 10 in both builds. The original OFC-11 is not a reliable detector of the race.

## Method and paths

- Worktrees `…/scratchpad/bug5/before` (49348ee) and `…/after` (705158f) were created with `git worktree add --detach`. Each ran `bun install --frozen-lockfile` and then `bun run dev`, on ports 5193 and 5194, on the TypeScript mock. All of it has been removed.
- Scripts (copied from `validation-f2-01b-shots/` and adapted; the originals are untouched): `validate.mjs`, `blitz.mjs`, `a11y.mjs`. New: `race.mjs`, `focusring.mjs` (probes the sheet buttons' focus rings).
- Outputs, under `workspace/validation-bug-5-shots/`:
  - Pixel comparison: `before/`, `after/` and `diff/` (4 runs each), `results.json`, `summary.json`, `validate-run1.log`.
  - Race: `race/`, `race-before.json`, `race-after.json`, `race.log`.
  - Blitz: `blitz-before/`, `blitz-after/`, their `.json` and `.log` files.
  - a11y: `a11y-before.json`, `a11y-after.json`, and their logs.
  - E2E logs: `e2e-after.log`, `e2e-after-repeat.log`, `e2e-before-repeat.log`.
