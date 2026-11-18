# Track X: Linux and containers (X1a to X6)

The Track X lead's plan and state for milestone two of the
[implementation plan](../../docs/implementation-plan.md#milestone-two--linux-in-parallel-track-x).
The stable contract is [ADR-0017](../../docs/adr/0017-linux-and-container-hosting.md)
(criteria LNX-1 to LNX-6) and the [Linux host design](../../docs/design/linux-host.md);
the checklist row is FC-164. Every handoff and review of this track also carries
[independent delivery and dependency boundaries](../../docs/independent-delivery.md).

## How this track is run

- **Lead and models.** The lead is a Claude session (model identifier recorded in the
  session, not here). The kit's active profile names `gpt-6.1-sol` for every phase;
  this lead runs on another model, so, as AGENTS.md requires, every phase and gate is
  kept and run with fresh-context subagents of the lead's own model: plan,
  implementation, fresh read-only review, repairs, original-author validation and a
  separate fresh final review. The Codex wrapper (`kit_codex.py`), Repowise and
  Hindsight are not installed in the cloud container this lead runs in, so
  `kit_hooks.py check` cannot pass there (its `kit_hindsight_embed.py test` step
  fails on the missing tool); the remaining kit checks and the project checks are run
  directly and their results are recorded per increment below.
- **Where the work is.** Branch `claude/hopeful-noether-lsmbac`, started from
  `origin/v1-integration` at `032ea63` and merged with it at `d408bfb9` (A1 part 1 and
  A2 part 1, which created `crates/org-platform`). The session that runs this lead may push only
  that branch; the merge into `v1-integration` is made by a session that may.
- **Systems.** Development and the full Linux checks run on Linux x86-64 (kernel
  6.18, as root in a container). macOS code is type-checked locally with
  `cargo check --target aarch64-apple-darwin` where the crate has no C build, and
  built and tested by the macOS CI job on every push. The signed-build, Keychain,
  Seatbelt and presence-dialog checks on a Mac are owed by the Track A engineer
  before X1c and X1d are relied on (design, section 7).
- **Writers.** One writer per file. Where an increment is split, each worker has its
  own worktree and file list, recorded under that increment.

## Increments

| Increment | State | Evidence |
| --- | --- | --- |
| X1a crate, `service` contract, check | Built 2026-11-18 | See below |
| X1b `fs`, `dirs`, `process`, `socket` | Not started | |
| X1c `admission`, `confinement`, `presence` | Not started | |
| X1d `credentials` | Not started | |
| X2 first worker on Linux (LNX-1) | Not started | |
| X3 host secrets on Linux (LNX-4) | Not started | |
| X4 web process confined on Linux (LNX-3) | Not started | |
| X5 service and container (LNX-2) | Not started | |
| X6 both systems in CI (LNX-5, LNX-6) | Not started | |

## X1a · The platform crate and the check

**Outcome.** An internal foundation; nothing a person can see. No change can add a
platform predicate outside `crates/org-platform`.

**What happened.** The Track A lead created `crates/org-platform` in A2 B0, with the
`Refused` type, the `service` contract and its launchd adapter, `disk` and `dirs`, and
a fake service manager in `testing`, while this track was building the same crate
from the design. A2's crate reached `v1-integration` first, so it is the crate: this
track's own `service` contract was dropped at the merge, and X1a is what A2's plan
entry says it is, the check, the allowlist, the layout for the remaining modules, and
a review of the `service` contract as built.

**Prerequisites.** None. **Early production-path proof.** The check runs against the
real workspace sources in `cargo test --workspace` and as its own CI step on both
systems.

**Files owned.** New: `crates/org-platform/tests/platform_conditionals.rs`,
`.github/platform-conditionals.allow`, `.github/scripts/platform_allowlist_shrinks.py`,
the module stubs `crates/org-platform/src/{fs/mod.rs,process.rs,socket.rs}`. Edited:
`crates/org-platform/src/lib.rs` (the layout text, `adapters!`, `supported!`,
`From<Refused> for io::Error`) and `Cargo.toml` (the check's dev-dependencies),
`Cargo.lock`, `.github/workflows/ci.yml` (two steps and one job),
`docs/design/linux-host.md`, `docs/implementation-plan.md` (X1a), `docs/FEATURES.md`
and `docs/FEATURE-AREAS.md` (FC-164), this file, `v1-state.md`,
`.agent-kit/project-skills/rust-workspace/SKILL.md` (the platform rule for every Rust
worker) and `.agent-kit/skill-catalog.json` (`crates/org-platform` joins the critical
host-and-storage area; evidence hashes). **Deleted.** Nothing of anyone else's.

**Decisions taken where the documents were silent** (each is written into the
[design](../../docs/design/linux-host.md), section 1):

1. The check is a test of `org-platform`, not a separate script: `cargo test
   --workspace` runs it for every engineer and on both CI systems with no extra
   tooling, and CI names it as its own step for a fast, readable failure.
2. The allowlist carries a count per file, and the check requires the exact count.
   A file-only list would let a listed file gain predicates unnoticed.
3. The check works on each file's whole token stream, with `syn` as the gate that the
   file is Rust. A walk of the syntax tree missed predicates in places `syn` keeps as
   raw tokens (first review, findings R2 and R3). It also counts `cfg_select!` and
   `cfg_match!`, raw identifiers, and any other use of the four target names, which
   are reserved for `org-platform`.
4. A second CI job compares the list with the commit where the branch left
   `v1-integration` and fails on an added path or a raised count. Without it "the list
   only shrinks" was a convention.
5. A capability with both adapters is declared by `adapters!()`; code only the two
   real adapters share sits in `supported! { … }`; the refusing adapter is compiled
   and tested on every target, and CI builds and tests the crate for Linux with musl,
   which gets only the refusing adapters. "Linux" is GNU/Linux. A2's modules keep
   their own two `cfg` lines until their Linux adapters are written.

**Review of the `service` contract as built (for the A2 lead, and for X5).** The
contract and its launchd adapter are A2's and are not changed here. What the systemd
adapter and the container mode need from it, to settle before X5 writes them:

- `validate` accepts an environment name that is not an identifier (for example
  `MY-VAR`) and line breaks in values. A systemd unit file silently drops the first
  and needs escaping for the second. X5 either tightens `validate` with the A2 lead
  or refuses those units in the Linux adapter.
- `ServiceStatus` has no place for "runs only while the user is logged in" (design
  3.4: `status` and setup report lingering). X5 adds it with the A2 lead.
- `Foreground` refuses everywhere and has no stated behaviour per method. X5 defines
  it (the design says nothing is registered and the runtime is the supervisor).
- `valid_instance_id` allows only lowercase letters, digits and `-`; the instance ids
  the host makes must fit that on Linux too.
- On Linux, `validate`, `xml_text` and `valid_instance_id` are used only by the
  `testing` module, so the Linux CI lint (`cargo clippy --workspace --all-targets
  --exclude org-host --exclude agentic-org -- -D warnings`, which leaves out the one
  consumer that enables `test-support`) failed on `v1-integration` at `d408bfb9`
  with three dead-code errors. This track's merge adds an `allow(dead_code)` for
  targets other than macOS to those three functions, the one change it makes to
  A2's files; the Linux adapter of X5 removes it.

**Checks and review.** Every phase was run by the lead's own model (a Claude session
and fresh-context Claude subagents), because the profile's `gpt-6.1-sol` wrapper is
not installed where the lead runs.

| Phase | Who | Result |
| --- | --- | --- |
| Plan and implementation | The lead | Commits `216913bc` to `963d6c71` |
| Fresh read-only review | A fresh subagent, on `216913bc` | No blocker; 4 major (three ways the check missed a real predicate; gaps in the lead's `service` contract), 7 minor |
| Repairs | The lead (the original author) | `c21c1f29`: the token-stream scanner, the list-only-shrinks CI job, the refusing-target CI step |
| Fresh final review | A second fresh subagent, on `c21c1f29` | Changes required: `v1-integration` had gained A2's `org-platform` (blocker for integration), and the scanner skipped what is nested in a conditional (major); 4 minor |
| Repairs | The lead | The merge with `v1-integration` that keeps A2's crate; nested conditionals, renamed `include`, tracked-file coverage |
| Final review of the exact commit | A third fresh subagent | Recorded in the next table row when it returns |

Checks on the merged tree, on Linux x86-64: `cargo fmt --check`; the CI lint commands
(`cargo clippy --locked --workspace --all-targets --exclude org-host --exclude
agentic-org -- -D warnings` and the `org-host`/`agentic-org` lint with the two allowed
lints); `cargo clippy --locked --workspace --all-targets -- -D warnings` for
`aarch64-apple-darwin` with C build steps stubbed (type-check and lint only); lint and
tests of `org-platform` for `x86_64-unknown-linux-musl`, the refusing target; the
`org-platform` tests; `kit_skills.py validate`. `kit_hooks.py check` cannot pass here
(Hindsight is not installed).

CI on GitHub for this branch: the LNX-5 step, the refusing-target step and the lints
pass on macOS and Linux. Four jobs are red on this branch and equally on
`v1-integration` at `032ea63`, for causes outside Track X: `cargo test` on macOS
(`agentic-org-web`, `release::tests::held_release_directory_cannot_switch_to_replaced_path`,
permission denied), `cargo test` on Linux (`agentic-org-web`,
`the_web_links_no_host_office_work_or_client_crate`: an offline `cargo metadata`
needs `core-foundation`), the frontend dependency audit (three advisories in
`@sveltejs/kit` and `picomatch`), and secret scanning (one finding in history). Until
this track's change, the first failing test binary stopped `cargo test`, so no crate
after `agentic-org-web` was tested in CI on either system; `--no-fail-fast` now runs
them all.
