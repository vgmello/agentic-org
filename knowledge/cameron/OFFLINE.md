# Cameron (ac-qa-engineer-cameron): offline handover, 2026-11-13

Branch `claude/pensive-mayer-2fyf8x` on `vgmello/agentic-org` holds everything. Last commits, in order:
`eea88721` (slice B: BUG-197/198), `8e8c907d` (slice A: BUG-194/195/196), then this note.
Earlier work, all merged into v1: P1 test gaps, BUG-186/187/188 (cfg-gating org-office-host tests on macOS),
mutation fix tests (`d85df1cf`).

## 1. The container (remote Claude Code session)
Linux, 4 vCPU, 15 GiB RAM, about 25 GB disk. Always run cargo/bun/playwright as the NON-ROOT user `ubuntu`,
never as root. The proxy env points at /root, so source `scripts/gate-env.sh` first (it copies nothing, it
only overrides CA/HOME/RUSTUP_HOME/CARGO_HOME/BUN_INSTALL/NPM_CONFIG_USERCONFIG); the CA bundle must be
copied once: `cp /root/.ccr/ca-bundle.crt /home/ubuntu/ca-bundle.crt`. Rust 1.99.1 comes from
rust-toolchain.toml via rustup installed for ubuntu (`rustup-init -y --profile minimal --default-toolchain none`).
Use a detached worktree per exact SHA (`git worktree add --detach /home/ubuntu/gate/wtX <sha>`; `chown -R
ubuntu:ubuntu` it; `git config --global --add safe.directory <path>` as root). Test binaries run under
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="timeout 120"` (a hang must not stall the gate).
Disk: delete old `target-*` dirs (6-7 GB each) before a cold build; one shared `CARGO_TARGET_DIR` per task.

## 2. Hourly Linux gate (standing job, review contract v2)
`scripts/hourly.sh <worktree> <tag>` runs, on the exact origin/v1 head, in this order:
1. `cargo fmt --all --check` (2-3 s)
2. `cargo clippy --locked --workspace --all-targets -- -D warnings` (strict, debug; 30-50 s warm)
3. `cargo clippy --locked --release --workspace --all-targets -- -D warnings` (strict, RELEASE; 22-86 s). Release
   catches what debug cannot (BUG-188: duplicate cfg_attr allow).
4. `cargo test --locked --workspace --no-fail-fast` (about 245 s warm, 227 s cold; v1 22452cb1: 2612+ passed, 0 failed,
   5 ignored doctests, 67 targets).
Green: write the receipt line to your journal, send NOTHING. Red: bisect `git log origin/v1 --first-parent --merges`,
name the culprit merge and author and the failing test in ONE message to ada. If UI merged, add the UI affected set
(section 4). Last green: v1 22452cb1 (hourly) and f40634dd, e3ea6b4c (full receipts).

## 3. Mutation testing (cargo-mutants 27.1, `cargo install --locked cargo-mutants` as ubuntu)
`scripts/mut.sh`: `cargo mutants --in-place --no-shuffle --timeout 120 --build-timeout 600 --output <dir> [-p crate] [--file f] -- --lib`
(`--in-place` cannot take `-j`). Changed-line mode for a Tier A fix: `--in-diff <git diff origin/v1 -- prod files>`
(`scripts/mutB.sh`). WARNING: if the session is killed mid-run the PRODUCTION FILE IS LEFT MUTATED
(look for `~ changed by cargo-mutants ~`); always `git diff` the production files before committing. Never run heavy
jobs during a mutation run (timeouts become false survivors); `scripts/pause_mut.sh STOP|CONT` pauses it.
Receipt (v1 f40634dd): org-docs 80 mutants/3 missed; org activity 63/1; work activity 107/4; org-server startup.rs 48/1.
Killed by tests (merged): Display, lookback block boundary, work corrupt-store checks. Accepted unkillable: org-docs
Alias arms (:208,:240, to be commented unreachable by their owner), work/activity.rs:196 (equivalent),
startup.rs host_disappeared guard (killed on macOS by the "poison" probe). Slice B changed-line mutants: 15, 10 caught,
2 unviable, 3 missed (2 equivalent `head.seq + 1`, 1 killed by the old-epoch case added after); the final rerun was cut
off, REDO IT before asking Tomas (`scripts/mutB.sh`, about 8 min).

## 4. UI gate (apps/org-ui)
bun must be exactly 1.4.4 (`npm i -g bun@1.4.4` into ubuntu's prefix); `bun install --frozen-lockfile`; Playwright
1.65.0 browsers: `./node_modules/.bin/playwright install chromium firefox webkit`, and as ROOT
`playwright install-deps webkit firefox chromium` once. Then `bun run check`, `bun run test` (vitest), `bun run build`,
and the plan from `bun scripts/ui-gate.ts select --explain --base origin/v1 [--files "<paths>"]` (it says FULL when
shared paths like src/lib/components/ui/** or src/lib/work/** changed; the standing instruction is the affected set,
full interactive CSP only when asked). Typical commands (all `--workers=2`): journeys
`playwright test --grep @journey --project=chromium --project=production`; categories with `--grep-invert @journey`
and the category spec files; CSP: `--config playwright.csp.config.ts --project=csp-chromium --project=csp-firefox
--project=csp-webkit` (+ `csp-chromium-full` with the journeys/categories). Full CSP (1270 tests) took 43.8 min.
WebKit lane is BACK (Safari is supported): `PW_WEBKIT=1 playwright test --project=webkit ...`; Linux WebKit is the
proxy, real Mac WebKit (Jules) is the truth for focus/pointer; two BUG-158 focus-restoration tests are flaky there.
Last receipts: v1 4acb522a CSP 1270/0; requests-3 1ff1a473 Chromium 452 + WebKit 100, 0 failures.

## 5. Firefox / BUG-191 state
Ordinary suite has no Firefox project; I used an untracked `playwright.ff.config.ts` (imports the base config, one
project `firefox` = the chromium project with `browserName: 'firefox'`). On v1 e8985b81, `playwright test
--config playwright.ff.config.ts org-chart focus-primitives.spec.ts non-chat-icons.spec.ts --workers=2`: 313 tests,
281 passed, 32 failed (focus-primitives 13, org-chart-responsive 18, org-chart-toolbar 1), 19 min. Matches Dakota's
count. Triage is Dakota's (she holds the measurements); until her table lands, Firefox is a separate non-gating
column in the receipt; then her list is the known-failures baseline and only NEW Firefox failures are reported.

## 6. BUG-194..198: repros and status (all fixed on this branch with failing-first tests; need review)
Slice B (`eea88721`, Tier A, tests+fix, org-office; org-office lib 576 passed):
- BUG-197 work page ceiling above head -> SnapshotRequired:
  `store::work::activity::tests::activity_refuses_a_ceiling_above_the_head_like_the_org_page_does`
  (fix: crates/org-office/src/store/work/activity.rs activity_page, head check next to the incarnation check).
- BUG-198 org page `before` must be a stored org.changed row with index <= its change count:
  `store::org::activity::tests::a_before_must_name_a_stored_change_row_and_an_index_within_it`
  (fix: store/org/activity.rs `names_a_change_row`). NOTE: index == len stays valid (existing test pins
  position(3,1) on a 1-change row; ada's wording said "at/past", I rejected only past; ask if stricter is wanted).
Slice A (`8e8c907d`, Tier A, org-docs; org-docs 19 + org-process 57 passed):
- BUG-194 `tests::the_closing_fence_is_exactly_three_dashes_and_a_line_end` (strict closing fence).
- BUG-195 `tests::field_paths_tell_apart_keys_that_would_look_like_paths` (keys with . [ ] " \ or empty are written
  as `["..."]`; ordinary keys unchanged, org-process tests pass).
- BUG-196 `tests::very_deep_flow_nesting_is_too_deep_not_a_parser_limit` (parser recursion limit -> TooDeep).
- F2 quoted `"<<"` and F4 signed `-.nan` kept as designed, documented in the org-docs module doc.
NOT DONE for the slices (owner's offline order): changed-line mutants for slice A, the fresh no-context review,
the evidence block + guard-to-test table (TP-1) request to ada/Tomas, AR-5 item 4. Reproduce any failing-first
test by reverting the production hunk (each test fails on the pre-fix code, verified before the fix).

## 7. Harnesses kept here
- `fuzz-docs/`: seeded-xorshift fuzz/property harness for org-docs (own `[workspace]`, path dep on
  ../../../crates/org-docs). `cargo test --release -- --test-threads=2` (about 7 min). `tests/verify.rs` holds
  the 5 minimal repros of BUG-194/195/196 (+ F2/F4); the F1/F3/F5 ones now pass.
- `cursor-property-tests.patch` (+1163 lines of scratch tests, applies to org-office tests at v1 22452cb1): edge
  tables (`qa_org_edge_table`, `qa_work_edge_table`) and randomized walks vs an oracle; findings C1/C2 (now BUG-197/198).
  Apply with `git apply`; do not commit as is.

## 8. Pitfalls that cost time
git as `ubuntu` fails with "dubious ownership" (set safe.directory or run git as root); `bun run build` is needed
before any playwright run that includes the `production` project (preview server); long `sleep` in a shell is blocked,
use `run_in_background` with an until-loop; the cloud channel is one-way, durable outputs go to this branch.
