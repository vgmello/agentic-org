# Validation · F1-02 part 1, platform HTTP client core (v1 4b64032 vs 4216b73)

**Verdict:** no screen changed, and nothing new.
- **Pixel compare:** 496 of 500 pairs are byte-identical after a clean re-run. The other 4 are clock and antialiasing noise.
- **Console and network:** identical to the build before it. There are no new requests, and the new client is never called (F1-03 switches over).
- **Blitz and a11y:** match the build before it.
- **Local checks:** check `COMPLETED 1180 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`, unit `Tests  1206 passed (1206)`, e2e `229 passed (8.2m)`.

**One S4 build note for Jules, checked by me:** the production import guard flags `package.json` because the rule is `!local.startsWith('src/')` (`platform/tooling/production-import-guard.ts:58`). `build-C.log` lists it twice, once per environment. The app only reads its version from that file, so it looks like a false positive. The guard is report-only, so the build doesn't fail.

The full working report follows.

---

# Validation: F1-02 part 1, the platform HTTP client core (regression pass)

**Verdict: no screen or behaviour changed. 4b64032 introduces no problems.**

- Change: merge `4b64032` ("Merge jules/f1-02-client-core into v1"). Its first parent is `4216b73`, which is F2-01d (see `validation-f2-01d.md`). That makes it build B, and this merge is build C.
- Date: 2026-11-10. I tested as a reviewer only and changed nothing in the product repo. The throwaway worktrees are removed, and the dev servers are stopped.

## What the merge changes

`git diff --stat 4216b73 4b64032` shows 11 files and 1,134 lines added, all in `apps/org-ui`:

- New, under `src/lib/platform/`:
  - `http/client.ts`, `json-http.ts`, `clock.ts` and `errors.ts`, with tests.
  - `feature-factory.ts`, with a test. This is `FeatureCapability` and `unavailableFeature()`.
  - `tooling/production-import-guard.ts`, with a test.
- `vite.config.ts` adds `productionImportGuard({ mode: 'report' })`. It's `apply: 'build'`, so the dev server and the screens don't use it.
- No route, component or existing module imports any of it (`git grep` outside `src/lib/platform/`, tests excluded, finds nothing). So no screen can change, as intended. Production areas move over in F1-03.

## Summary

- **Pixel comparison:** the same 125 states as F2-01d (94 chat states and the F1-01 main set of 31), in 4 runs (1440×900 and 960×640, OS dark and light, WebKit).
  - That gives **500 pairs, and 494 are byte-identical PNGs.**
  - I re-ran the run with the 2 odd pairs without interference, and those states then match B byte for byte. That makes it **496 of 500, with the other 4 being clock and antialiasing noise** (see "Pairs that differ").
- **Behaviour:** the URL, title, headings, alerts, focus, control counts and text match in every state, apart from the clock text below. The same two 960×640 steps time out in B and in C (F2-01d finding 1).
- **Console and network: nothing new in C.**
  - The console messages in C are exactly B's: the three from the deliberate 404 (`31-not-found`).
  - The page requests are the same list in every run: `GET /`, `/agents`, `/chat`, `/chat?…`, `/create`, `/nope` (documents) and `/package.json?…` (script).
  - There's no request to another origin and no fetch or XHR. The new HTTP client is never called.
- **Blitz and a11y:**
  - `blitz-C.json` matches F2-01d's `blitz-B.json` in all 90 keys, apart from timing. That covers the owner job, long, many and rapid messages, fail and Retry, history and reload, resize, invalid input, keyboard and live regions in WebKit and Chromium, and the requirement probes.
  - `a11y-C.json` matches `a11y-B.json` in all 24 states and all 12 ARIA trees, ignoring the wall clock.
- **Checks in C:**
  - `bun run check`: `COMPLETED 1180 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS`
  - `bun run test`: `Tests  1206 passed (1206)` (55 files; B has 1087 in 51)
  - **E2E, final line: `229 passed (8.2m)`**, with none flaky or failed (`heavy`, 2 workers).
- **Jules's archive** (`app-jules/apps/org-ui/playwright-report/f1-02-4b64032`), compared with my e2e run of 4b64032:
  - 39 of its 59 named shots are byte-identical.
  - The 16 chat shots that differ do so in wall-clock times only, the same pattern as against Avery's archive in F2-01d.
  - The others are edit-agent-context (1 px), live-paused (LIVE-8 timing, 272 px), and shared-state and its crop (`shot.spec`'s infinite-animation fixture, about 300 px).
  - The automatic `test-finished-1.png` shots vary by timing and aren't evidence.

## Findings introduced by 4b64032

None on any screen.

**One note for Jules, about the build only (not a screen, S4):** `bun run build` in C prints the guard's report twice, once per environment. It lists 17 entries, including `package.json` and `src/routes/+layout.svelte`.
- `package.json` is outside `src/`, so the rule `!local.startsWith('src/')` flags it, although the app only reads its version from it. That looks like a false positive.
- The list is useful for F1-03. Report mode means it never fails the build. Evidence: `build-C.log`.
- I raise this so it doesn't become noise, not as a defect.

## Pre-existing findings

Everything in `validation-f2-01d.md` is unchanged in C, with the same screenshots, blitz values and a11y results. Nothing new.

## Pairs that differ

| Pair | Pixels | Why it isn't the merge |
|---|---|---|
| `default-dark`, `960x640-dark` and `960x640-light` `/20-chat` | 751 each | "Waiting 8 min" against "9 min", and "11 min" against "12 min": a wall-clock minute ticked over. |
| `default-dark/16-project-new-task-dialog` | 12 | The same 12 antialiased pixels on the New task dialog's focus ring as in F2-01d. B's capture is the odd one: A and C are byte-identical there. |
| `960x640-light/c39-renamed` and `c40-auto-channel-members` | 201,038 and 1,188 | **My test, not the merge.** I started C's e2e run in the same worktree while this run was capturing. Its `svelte-kit sync` rewrote `.svelte-kit/generated`, and the dev server on 5203 sent a full page reload at 17:45:15 (`dev-C.log`). That reset the in-memory chat between steps c38 and c39. I re-ran the C-channel flow at 960×640 light after e2e had finished (`rerun-960-light/`), and all 22 shots (11 steps, raw and normalised) are byte-identical to B. |

The diff images are in `validation-f1-02a-shots/diff/`.

## What I covered

- The 94 chat states and 31 F1-01 main states, in C, in 4 runs each, compared with the B captures from F2-01d. The F2-01d report lists the states.
- Console and network per run.
- Blitz and a11y in C, compared with B.
- Check, unit and the full e2e suite in C, plus a build to see the new guard's output.
- Jules's archived e2e screenshots.

## Method

- `validate.mjs` is copied from F2-01d. Its new `REUSE=B=<dir>` option copies B's screenshots and step results from the F2-01d run instead of capturing B again. C was captured fresh (`PAIR=B=5202,C=5203`).
- The scripts import Playwright from the B worktree, which is now removed. Point the import at a current `apps/org-ui/node_modules` before re-running them.
- C ran `bun run dev --port 5203` from a detached worktree at 4b64032.
- E2E ran with `PW_PORT=4302 ~/.agency/bin/heavy bunx playwright test --workers=2`.
- `compare-jules.mjs` is adapted from `compare-avery.mjs`. It skips `.last-run.json` and non-PNG files.

## Files

- Report: `workspace/validation-f1-02a.md`
- Screenshots and data: `workspace/validation-f1-02a-shots/`
  - `{B,C}/<run>/<state>.png` and `.norm.png` (B is copied from `validation-f2-01d-shots/B`)
  - `diff/<run>/`
  - `rerun-960-light/C/960x640-light/` and `rerun-960-light/dry.json`
  - `blitz-C/`, `blitz-C.json` and `a11y-C.json`
  - `jules-compare/f1-02-4b64032/` (`jules-*.png`, `mine-*.png`)
  - `summary.json` and `results.json`
  - Logs: `validate-run1.log`, `rerun-960-light.log`, `dev-C.log`, `blitz-C.log`, `a11y-C.log`, `compare-jules.log`, `e2e-C.log`, `check-C.log`, `unit-C.log` and `build-C.log`
  - Scripts: `validate.mjs`, `blitz.mjs`, `a11y.mjs` and `compare-jules.mjs`
