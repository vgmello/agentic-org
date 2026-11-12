# Tiered UI gate

The full browser suite took about 47 minutes when this gate was adopted (measured once, not enforced; ordinary Chromium + WebKit, interactive CSP, gateway CSP). Per slice we run a tier instead. Everything is driven by one file, [`gate-manifest.json`](gate-manifest.json).

| Tier | What | When |
| --- | --- | --- |
| Journeys | The tests tagged `@journey`: 55 tests in 13 flows (create/open organization, navigation, chat, agents, org chart, projects/work/tasks/bugs, offices, requests, processes, copilot, retired import, production-unavailable) plus an 8-test WebKit subset for pointer/focus | Every UI change, never skipped |
| Areas | The whole category (all its spec files) whose source paths changed | Added to the journeys |
| Full | Everything, as before (`test:e2e:full`) | A shared/high-blast-radius or unknown path changed; and ada runs it periodically on v1 head, not per slice |

## Run it

All commands run from `apps/org-ui`. On the shared Mac wrap them in `~/.agency/bin/heavy`. The selector runs Playwright with `--workers=2` unless you pass `--workers <n>`; the plain `test:e2e:*` scripts and `playwright` itself still need `--workers=2` added by hand.

```sh
bun run test:e2e:journeys                 # build + journeys on Chromium + production (+ WebKit subset)
bun run test:e2e:select                   # explain: rule per changed file, then the commands it would run
bun run test:e2e:affected                 # select from `git diff origin/v1...HEAD` + working tree, build, run in order, 2 workers
bun run test:e2e:areas                    # list categories (specs and source globs) and journeys
bun run test:e2e:verify                   # Playwright --list: journeys == manifest (also per entry), journeys + categories == the whole suite; prints each mismatch
bun run test:e2e:full                     # unchanged full ordinary suite (same as test:e2e)
bun run test:e2e:csp                      # unchanged full interactive CSP; test:e2e:csp-gateway unchanged
```

Selector options (`--flag value` or `--flag=value`): `--base <ref>` (default `origin/v1`, checked with `git rev-parse --verify`), `--file <path>` (repeatable) or `--files <newline-separated paths>` (instead of git; each must be a repository-relative path such as `apps/org-ui/src/...` of a file that exists now or in HEAD, otherwise it is treated as unknown and runs the full suite), `--workers <n>` (default 2), `--explain`, `--no-webkit` (warns when the plan is the full suite). Unknown flags are an error. Playwright arguments go after `--`; because `bun run` swallows the first `--`, write `bun run test:e2e:affected -- --workers 3 -- --headed` (the first `--` is bun's, the second the selector's), or call `bun scripts/ui-gate.ts run -- --headed`. Steps run one at a time and stop at the first failure. If git cannot list the changes (missing base, shallow clone, not a repository), `select` and `run` use the full suite; so they do when `e2e/gate-manifest.json` cannot be read or parsed (`areas` and `verify` then exit 2). Paths that are absolute, contain `..` or a backslash, or are git-quoted are treated as unknown, so they also run the full suite.

## How selection works (scripts/ui-gate-lib.ts)

Changed files are `git diff base...HEAD`, the working tree and untracked files. Each file is classified, in this order:

1. `rules.ignore`: markdown, vitest unit tests (`src/**/*.{test,spec}.{js,ts}`; a test-named file anywhere else, such as `e2e/x.test.ts`, is unknown), `scripts/ui-gate*.ts` and `apps/org-ui/.gitignore`: no browser impact. A root `.gitignore` is outside the app and not on the no-impact list, so it runs the full suite. `e2e/gate-manifest.json` is deliberately **not** ignored; it is shared, so editing the manifest runs the full suite.
2. `rules.shared` (UI primitives, app shell/layout, routes layouts, `src/app.*`, hooks, vite/svelte/playwright config, `package.json`, `bun.lock`, `.github/workflows/**`, `e2e/fakes`, `e2e/ready.ts`, `src/lib/org` core, `src/lib/office`, `src/lib/work`, `src/lib/name.ts`, messaging drafts and http): **full suite**, ordinary and CSP.
3. A category's `sources` (or its own spec files): that category runs in addition to the journeys.
4. `rules.cspFull` (CSP policy/tooling, vite/svelte config, build assets): full interactive CSP. `rules.gateway` (`apps/org-gateway`, `crates/**`, root `Cargo.toml`/`Cargo.lock`/`rust-toolchain.toml`, `config/csp`, gateway/CSP/build files): the 3 gateway CSP tests.
5. Outside `apps/org-ui`, only `rules.noImpact` is no UI impact (the other apps, docs, spikes, LICENSE, CODEOWNERS; the other apps were checked not to reference the UI or the gateway crates). **Anything else, inside or outside `apps/org-ui` (root `package.json`, `bunfig.toml`, `packages/**`, `.github/actions`, ...), is unknown and runs the full suite (fail closed).**

Ordinary plan: `--grep @journey`, then `--grep-invert @journey` over the selected categories' spec files, so no test runs twice. Interactive CSP plan (when a category or CSP/shared path changed): the CSP-specific specs on three browsers, the journeys and the selected categories on `csp-chromium-full`, leaving out the specs that project ignores (`cspIgnoredSpecs`, checked against `playwright.csp.config.ts`; a category left with none gets no CSP step). **A category-only change runs interactive CSP for the CSP-specific specs, the journeys and that category only** (not the whole CSP suite). A shared/unknown path or a CSP/build config change runs the full CSP. Docs-only or non-UI changes run the ordinary journeys only.

## Rules

- Journeys are never skipped for a UI change, and a red journey is a stop.
- A shared or unknown path means the full suite. Do not narrow `rules.shared` to save time; move a file out of it only when it is truly owned by one area.
- Cross-area coupling must be declared. `ui-gate.test.ts` builds the `src`/`e2e` import graph and fails when a category-owned file is imported by shared code or by a category that does not also own it. Fix it by moving the file into `rules.shared`, listing it under every importing category, or (with a reason) `allowedCoupling`. The entries there are the shell layout importing the messaging barrel for `provideDraftStore`, and the spec-to-page visits described next.
- The same guard reads the `visit(page, '/x')`/`goto('/x')` literals in each spec: the opened page must be owned by the spec's category (or be shared), else it needs an `allowedCoupling` entry with a reason. Generic cases (every spec sets up through `/create`; shell-chrome specs opening Offices or Chat) are declared that way instead of widening ownership. Not seen: navigation by clicking, and routes built from variables, arrays or helpers (`shell-topbar` and `shell-switch-footer` visiting every shell route, `look-primitives`, `org-chart-responsive`, `production-unavailable`, `project-profile`, and `non-chat-icons.spec.ts`, which picks `/projects`, `/projects/beacon` or `/org-chart` with a ternary). So a page change may not run the shell-chrome or narrow-width specs for that page's states; that speed tradeoff is accepted, and a shared or unknown change still runs everything. The `allowedCoupling` entries for shell-chrome and look specs say so.
- `alsoRuns` (manifest) selects a category to run in addition when given files change, without making it own them: a way to say "this area's tests exercise that page" without the import-graph guard demanding co-ownership of every import of the page. Today the org-chart page (`+page.svelte`, `+page.ts`) and `ChartFitMenuItem.svelte` also run `projects-work`, because `non-chat-icons.spec.ts` opens `/org-chart` and asserts page-owned behaviour (the take-off toast and last-team dialog, the View menu with Actual size, the empty/loading/error states). The guard checks each entry names a known category, a reason, and globs that match a current file.
- `budget.measured` is a self-report: `verify` and the guard check the test count and the budget, but the recorded seconds are whatever the last person who ran the journeys wrote down.
- Every spec file belongs to exactly one category, and every journey names a real tagged test. `bun run test` (vitest, `src/lib/platform/tooling/ui-gate.test.ts`) fails otherwise, and also when any current source file is unmapped.
- The journey budget is 60 tests and 5 minutes on Chromium + production with 2 workers; the 8-test WebKit subset runs on top of that and is not counted in the 60. `budget.measured` records the last measurement; after changing the journeys, run them and update it (the guard compares test counts).
- A full run (ordinary plus CSP, plus the 3 gateway CSP tests, which a full plan always includes, and which a tiered plan adds when gateway files changed) happens on v1 head periodically, run by ada, not per slice.

## Notes on the tags

Tags were added to the `test(` lines only; no assertion, fixture or test body changed. The one non-tag edit is in `navigation.spec.ts`: a `JOURNEY_SECTIONS` constant and a tag expression `JOURNEY_SECTIONS.includes(name) ? '@journey' : []` on the existing per-section loop, so five of the ten section tests are journeys (counted as `count: 5` in the manifest and checked by `test:e2e:verify`).

## Add a spec

Create `e2e/<name>.spec.ts` and add it to one category's `specs` globs (it must match exactly one). If it covers a new area, add a category with its `sources` globs. If it needs WebKit, add it to the `webkit` project in `playwright.config.ts` and to `webkitSpecs` (the guard compares them). A test file that is not `*.spec.ts` and runs only in its own Playwright project (today `non-chat-icons-default-font.ts` in `chromium-default-font`) is listed in `projectFiles` (file to project, compared with `playwright.config.ts` by the guard); it goes in a category's `specs` like any spec, and the selector then adds that project to the ordinary category step and leaves the file out of the CSP step.

## Add a journey

A journey is the smallest existing test that proves a flow end to end; prefer one that already passes reliably. Tag it on the `test(` line, `test('title', { tag: '@journey' }, async ({ page }) => ...)`, add `{ "spec", "match", "count" }` under a journey in the manifest (`match` is a substring of that one `test(` line; `count` is how many Playwright tests it expands to), set `"webkit": true` only if the WebKit project already runs it, run `test:e2e:verify` and the journeys, and update `budget.measured`. Do not weaken or move assertions to make a test fit.

## Known limits of the import-graph guard

The guard follows static `import`/`export ... from` statements and `'/src/lib/...'` string paths. It cannot see `import.meta.glob` (it fails if one appears under `src` or `e2e`, so import the files explicitly), `new URL('./x', import.meta.url)`, computed paths, or Vite aliases other than `$lib`. Anything wired that way must be declared in `rules.shared` by hand.

## Known limit of the `projectFiles` guard

The test that compares `projectFiles` with `playwright.config.ts` reads one pattern, `{ name: '...', testMatch: '...' }` on one line with a string `testMatch`. A project written with an array or multi-line `testMatch`, or a different key order, is not seen, so such a file would only be caught later by `test:e2e:verify` (its count would not add up). Keep new dedicated projects in that one-line form, or extend the guard.

## Known limit of `verify`

`verify` proves the ordinary-Playwright partition (journeys + category selections = the whole `e2e/` suite, including the WebKit duplicates). It does not list the interactive CSP or gateway runs; those steps are covered by the exact-argv unit tests and by the CSP config guard.

The focus-primitives flow also opens Agents, Org chart, Projects, Work/Requests, Chat, Offices, Processes and the copilot dialog through `consumerFocus`'s route helper. Those visits are beyond the literal spec route scanner. The dedicated ProcessSheet regression is declared in `allowedCoupling`; all shared primitive/config/manifest changes still select FULL. Page-only changes keep their category and journey coverage. Its dedicated `chromium-focus-default-font` project proves actual 20px defaults; the ordinary Chromium/WebKit matrix additionally uses 40px roots without claiming a native WebKit default-font preference.
