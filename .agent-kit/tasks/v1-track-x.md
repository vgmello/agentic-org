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
  A2 part 1, which created `crates/org-platform`), at `831f10ca` (the store
  architecture, A1's SQLCipher removal) and at `42dd1322` (A1.5's single launch
  contract, A2's setup and service on macOS, A3's stored browser sessions, instance
  IDs as UUIDv7; see "Merges with v1-integration" below). The session that runs this
  lead pushes that branch. **Owner's go-ahead, 2026-11-18:** the lead may merge into
  `v1-integration` what is ready, once an independent reviewer has approved the exact
  commit; nothing else is pushed there. The path to that merge is under "Merge into
  v1-integration" below.
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
| X1a check, list, CI guards, layout | Built and accepted 2026-11-18 | See below |
| X1b `fs`, `dirs`, `process`, `socket` | Built and accepted 2026-11-18 (three slices, each reviewed) | See below |
| X1c `admission`, `confinement`, `presence` | Built and accepted 2026-11-18 (two slices, each reviewed); Mac run owed | See below |
| X1d `credentials` | Built and accepted 2026-11-18; Mac run owed | See below |
| X2 first worker on Linux (LNX-1) | Built, reviewed and repaired; LNX-1 passes on Linux and in a default Docker container; a fresh final review of the repairs is owed | See below |
| X3 host secrets on Linux (LNX-4) | Built, reviewed and repaired (`ce7dd5a1`); a fresh final review of the repairs is owed | See below |
| X4 web process confined on Linux (LNX-3) | Adapter reviewed and repaired; Landlock optional; host slice integrated (`4823b23a`); the real-host LNX-3 test and the last web tests are in work; a fresh review of the host work is owed | See below |
| X5 service and container (LNX-2) | Built on branch `x/x5-service` (`a31873e8`), not integrated; under fresh review; the browser is not served from the container | See below |
| X6 both systems in CI (LNX-5, LNX-6) | Not started | |
| X7 HTTPS for a container with the instance's own certificate | Pending, recorded by the owner 2026-11-18; with Track A | Implementation plan X7 |

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
`crates/org-platform/src/lib.rs` (the layout text and `From<Refused> for io::Error`) and `Cargo.toml` (the check's dev-dependencies),
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
5. A capability with both adapters declares them with explicit `cfg` lines at the top
   of its `mod.rs`; code only the two real adapters share sits in `shared.rs`; the
   refusing adapter is compiled and tested on every target, and CI builds and tests
   the crate for Linux with musl, which gets only the refusing adapters. "Linux" is
   GNU/Linux. Two macros that generated those declarations were written first and
   removed in X1b: `cargo fmt` formats only modules it can see declared, so the
   adapter files had escaped formatting. A2's modules keep their own two `cfg` lines
   until their Linux adapters are written.
6. The CI list comparison accepts a new path while the total does not rise, so another
   track can rename or split a listed file; a count never rises and the total never
   rises.

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
| Final review of the exact commit | A third fresh subagent, on `9beae801` | Accept. No blocker or major; 7 minor, 6 nit |
| Repairs after acceptance | The lead | The minors: selector arms hide nothing, `path` only as a module attribute, a tracked file gone from disk, the duplicate workspace member, the list comparison's rename rule, stale statements in the documents |

Checks on the merged tree, on Linux x86-64: `cargo fmt --check`; the CI lint commands
(`cargo clippy --locked --workspace --all-targets --exclude org-host --exclude
agentic-org -- -D warnings` and the `org-host`/`agentic-org` lint with the two allowed
lints); `cargo clippy --locked --workspace --all-targets -- -D warnings` for
`aarch64-apple-darwin` with C build steps stubbed (type-check and lint only); lint and
tests of `org-platform` for `x86_64-unknown-linux-musl`, the refusing target; the
`org-platform` tests; `kit_skills.py validate`. `kit_hooks.py check` cannot pass here
(Hindsight is not installed).

CI on GitHub: for `c21c1f29` (before the merge with A2's crate) the LNX-5 step, the
refusing-target step and the lints passed on macOS and Linux. Later commits' runs are
recorded under the increment they belong to. Four jobs are red on this branch and equally on
`v1-integration` at `032ea63`, for causes outside Track X: `cargo test` on macOS
(`agentic-org-web`, `release::tests::held_release_directory_cannot_switch_to_replaced_path`,
permission denied), `cargo test` on Linux (`agentic-org-web`,
`the_web_links_no_host_office_work_or_client_crate`: an offline `cargo metadata`
needs `core-foundation`), the frontend dependency audit (three advisories in
`@sveltejs/kit` and `picomatch`), and secret scanning (one finding in history). Until
this track's change, the first failing test binary stopped `cargo test`, so no crate
after `agentic-org-web` was tested in CI on either system; `--no-fail-fast` now runs
them all.

## X1b · Files, locations, processes and sockets behind the crate

**Outcome.** An internal foundation; nothing a person can see. Durable flushes,
exclusive renames, filesystem checks, default locations, process supervision and
socket peer checks come from `org-platform`.

**Prerequisites.** X1a. **Early production-path proof.** Each slice moved one function
first and ran its consumer's existing tests through the new path before the rest.

**Split and why.** Three slices with no file in common, two writers at a time in
separate worktrees, the lead coordinating and integrating:

| Slice | Owner | Files | State |
| --- | --- | --- | --- |
| `process` | Worker in `x/x1b-process` | `crates/org-platform/src/process/`, `crates/org-supervise/` | Integrated at `baf38f12` |
| `socket` | Worker in `x/x1b-socket` | `crates/org-platform/src/socket/`; in `crates/org-socket/`: `peer.rs`, `rights.rs`, `rights/` (not `confinement*`, `image.rs`, `examples/`, which are X1c) | Integrated at `0d970d35` |
| `fs` and `dirs` | Worker in `x/x1b-fsdirs` | `crates/org-platform/src/{fs,dirs}/`, `crates/org-proto/src/location.rs`, the files of design section 7's X1b row in `org-host`, `org-office`, `org-client`, `org-spool`, and the location call sites | Integrated at `70760642` |

Each worker ran on the lead's model with the resolved skill packet for its paths
(project conventions, delegate communication, rust-workspace, host-and-storage) and
[independent delivery](../../docs/independent-delivery.md).

**Decisions.**

1. Behaviour is preserved on macOS and GNU/Linux. The one intended change of the
   `process` and `socket` slices: a Unix target that is neither now builds and refuses
   at run time, where `org-supervise` had a `compile_error!` and `org-socket` ran
   macOS-flavoured branches.
2. `process`: the platform reports facts and the consumer keeps the rules. Two
   closures cross the contract for that reason (`release_spawned`'s `exited`,
   `signal_checked`'s `mark_accepted`), and the scan reports the mark's value while
   `org-supervise` decides whether it is a valid session mark
   (`recovery::signal_allowed_by`, unit-tested over every evidence kind).
   `validate()` is the first refusal on every entry point.
3. `socket`: one receive is a `DescriptorReceive` value. On macOS it holds the
   caller's fence lock from before `recvmsg` until the descriptors are close-on-exec;
   on Linux it holds nothing. The refusing adapter cannot construct one, so it has no
   flags or lengths to invent.
4. `dirs`: the Linux adapter gives the design's XDG locations now and reads the XDG
   variables itself, keeping A2's `application_base(home)` signature; `org-proto`
   keeps only path composition that takes the base and the limit. The runtime base
   for sockets (`$XDG_RUNTIME_DIR/agentic-org/`) is not built: Linux sockets stay
   under the office home until X2 needs it.
5. `fs`: one strict `full_sync` and one strict exclusive rename for every caller.
   `org-client` used to fall back when the device flush failed, and on Linux to link
   and unlink; both follow the strict rule now.
6. A release host's office home is pinned to the account's default with the
   environment left out (`dirs::account_application_base`). The first version of
   this slice let an absolute `XDG_DATA_HOME` move both sides of the comparison, so
   a Linux release host could be pointed at any directory its user owned (review
   finding D1). `XDG_DATA_HOME` now makes a release host refuse, as `ORG_OFFICE_HOME`
   does on both systems.
7. `org-platform` is libc-only, and a second crate, `org-platform-host`, holds what
   only the host links (ADR-0017 decision 6 as amended). The `fs` slice had put
   `core-foundation` into `org-platform` for the macOS file name comparison, which
   made the macOS web process, office worker and `agent-os` link CoreFoundation; the
   X1c worker moving the presence bridge found the general problem. The comparison is
   `org_platform_host::names` now, and `crates/org-platform/tests/libc_only.rs` keeps
   the rule.

**Review.**

| Slice | Phase | Result |
| --- | --- | --- |
| `process` | Fresh read-only review, on `baf38f12` | Accept. Behaviour preserved on Linux (run) and, by type-check, lint and a hunk-by-hunk reading, on macOS. 5 minor |
| `process` | Repairs by the lead | The mark rule named and tested; a deprecated type alias that the removed `compile_error!` had hidden on a refusing target; CI lints `org-supervise` for the refusing target |
| `socket` | Fresh read-only review, on `0d970d35` | Accept. The descriptor fence is held over exactly the same statements as before, read line by line. 2 minor, 5 nit |
| `socket` | Repairs by the lead | No `libc` message type in the platform API; a receive is `!Send` on every system; SAFETY comments |
| `fs` and `dirs` | Fresh read-only review, on `70760642` | Accept. Flush, rename and probe behaviour preserved; 6 minor, the pin finding among them |
| `fs` and `dirs` | Repairs by the lead | The environment-free pin with tests; per-system flush-evidence tests; the libc-only split |

**Checks.** After the three integrations and the repairs, on Linux x86-64 as an
unprivileged user: the whole workspace's tests, 3,083 passed after the third
integration; the 22 that fail also fail before Track X (19 `org-host` tests that need
a worker to start, which X2 brings; 3 that need IPv6 or network access this container
lacks); `org-host` and both platform crates were run again after the repairs with the
same failing set. `cargo fmt --check`, the two CI lint commands, the macOS-target lint
of the whole workspace, and the refusing-target lint and tests of both platform crates
pass. The list is at 58 files and 353 predicates.

**macOS, from CI** (the lead cannot run macOS). GitHub Actions `macos-15`, runs for
`0d970d35` (process and socket), `70760642` (fs and dirs) and `261a7ae9`
(`org-platform-host`), each with `--no-fail-fast`: every test binary of the moved
code ran and passed. At `261a7ae9`: `org_platform` 78 passed (2 ignored: the real
launchd and disk-encryption tests), `platform_conditionals` 24, `libc_only` 3,
`org_platform_host` 2, `org_supervise` 96 with `lifecycle` 11, `recovery_native` 10
and `spawn` 15, `org_socket` 61 with `stream_adoption` 4 and `typed_adoption` 3,
`org_office` 567, `org_client` 39, `org_spool` 14, `org_proto` 214. The LNX-5 step and
the lint pass. Three test targets fail on macOS, each for a cause that predates X1b
(they fail identically at `6b7f4336`, before any slice merged) and none in a file
Track X changed: `agentic-org-web`'s
`release::tests::held_release_directory_cannot_switch_to_replaced_path` and six
`org-host` `release::store::tests` (all `PermissionDenied` under the release store's
temporary directory on the runner), and `agentic-org-web`'s `dependency_closure`
(an offline `cargo metadata` that needs a crate of the other system; it fails on
Linux for the mirror-image reason). Before this track's `--no-fail-fast`, the first
of them stopped the run, so `v1-integration`'s own CI has never run the `org-host`
tests on macOS. On Linux CI, run for `0d970d35`: all of the moved code's binaries
pass; the one failure is the same `dependency_closure` test. Linux runs for the
later commits were still queued when this was written.

**Open, carried forward.**

- `crates/org-supervise/clippy.toml` forbids `Child::wait`, `Command::spawn` and
  `libc::wait` so that exit observation stays `WNOWAIT`. The moved code is in
  `org-platform`, which has no such file; `disk/macos.rs` and `service/macos.rs`
  (A2's) use `Command`. X1c adds the fence to `org-platform` with the A2 lead.
- `crates/org-supervise/src/spawn.rs` keeps one predicate, on a test module that
  asserts `OwnedChild` behaviour only macOS has. It leaves the list in X2 with the
  `testing` module, like the other test files.
- Three predicates stay in X1b's files on purpose: the `cfg!` gate in
  `org-host/src/human.rs::paths` (custody policy; X1d), and two Linux-only tests,
  `instance/directory.rs` (`native_instance_children_refuse_a_different_device`) and
  `instance/temp.rs` (`a_private_memory_backed_temp_base_is_refused`), which leave
  the list in X2 with the other test files.
- The rule "override when absolute, else the base for an absolute `HOME`" is composed
  in three crates (`org-client`, `org-host`, `agentic-org-office`), each with its
  test. A relative or unset `HOME` gives no office home even when `XDG_DATA_HOME` is
  absolute.
- `org-socket`'s `unix_datagram` adoption calls no platform function and so does not
  refuse on a refusing target, which is intended: it performs every check it performs
  elsewhere. (X1c moved the confinement code out, so `org-socket` builds for a
  refusing target and CI lints it there.)
- For A2 part 2: X1b's `fs`/`dirs` slice removes about 250 lines from
  `org-host/src/instance/directory.rs` and changes `home.rs`; the next Track A merge
  in those files needs a rebase.

## X1c · Admission, confinement and presence behind the crates

**Outcome.** An internal foundation; nothing a person can see. Starting and admitting
a worker or the web process, confining the web process and the recovery presence
check go through the platform crates on macOS, with refusing adapters elsewhere.

**Prerequisites.** X1a, X1b. **Split.** Two slices, two workers in separate worktrees:

| Slice | Owner | Files | State |
| --- | --- | --- | --- |
| `confinement` and `presence` | Worker in `x/x1c-confine` | `crates/org-platform/src/confinement/`, `crates/org-platform-host/src/presence/` and `build.rs`; out of `crates/org-socket/src/confinement*`, `crates/org-host/src/recovery/presence.rs`, `crates/org-host/build.rs`; the call sites in `org-host/src/web.rs` and `apps/agentic-org-web/src/process.rs` | Integrated at `27125f6c` |
| `admission` | Worker in `x/x1c-admission` | `crates/org-platform-host/src/admission/`, `crates/org-platform/src/image/`; out of `crates/org-host/src/{spawn, server_check, code_signing, peer, app_auth, bootstrap, release*, supervisor/**, offices, lib, web}.rs` and `crates/org-socket/src/image.rs` | Integrated at `61acd21a` |

**Decisions.**

1. `confinement` has the design's shape (`confine_web`, `verify_web`, `Enforcement`)
   with `io::Result`, a process id instead of the admission `Child` until X4, and two
   host checks instead of one, because one would reorder macOS's steps.
2. `presence` is in `org-platform-host`, because the bridge links Foundation,
   LocalAuthentication, Security and SystemConfiguration (X1b decision 7). Its macOS
   adapter carries its own copy of the peer audit-token read until the admission
   slice lands; the lead makes one call the other at that integration.
3. The Linux adapters of both refuse in X1c. The host's behaviour on Linux is
   unchanged: no web process starts, and the recovery release is unavailable.
4. The confinement example stays in `crates/org-socket/examples/`: it needs that
   crate's descriptor passing, and `org-platform` depends on no workspace crate.
5. The ready line carries the Landlock status only. A domain not enforced in full is
   refused when the line is read (`web::reported`), and where a system has seccomp
   the host observes it on the process itself, as the design says.

6. `admission` keeps the design's contract and adds what today's macOS order needs:
   the spawn lock passed as `fence` so it never covers the image check; `start` and
   `Held::release` for the web path's re-check before resume; `recheck` and
   `readmit`; typed refusals so the host keeps its log texts and error classes; the
   host's signing policy as data (`SigningRule`). Policy stays in `org-host`:
   `ServerCheck` decides whether to spawn and builds the `ExpectedImage`; the
   platform crates name no signing identifier, team or entitlement of this product.
7. The running image's identity (`csops`) and the release target are
   `org_platform::image`, libc-only, because the web process needs them too. On
   Linux with another C library the release target now refuses, where a `cfg!` used
   to answer Linux: that target gets only refusing adapters.
8. Two orderings changed, both judged harmless by the review: the child's start
   time is stamped after the resume instead of just before, and on a failed resume
   the child is killed before its standard input closes.

**Review.**

| Slice | Phase | Result |
| --- | --- | --- |
| `admission` | Fresh read-only review, on `61acd21a` | Accept. Both step sequences (worker, web process) read line by line against the old code: no path resumes a child before its check, every re-check is made at the same point on the same bytes, the spawn code is byte-identical, nothing is admitted that was refused. 4 minor, 6 nit |
| `admission` | Repairs by the lead | Records and stale citations now; two small code repairs (the pin-after-admit composition as a pure function tested on every system; an unreachable guard's error class) after the X2 adapter slice, which edits the same files, is integrated |
| `confinement` and `presence` | Fresh read-only review, on `27125f6c` | Changes required, in test evidence only: the example's "a threaded process is refused" check could no longer fail, because `confine_web` also connects. Production code preserved by reading: the Seatbelt profile and the bridge are byte-identical, and the order in the web process and the host is unchanged. 2 minor, 5 nit |
| `confinement` and `presence` | Repairs by the lead | The example asserts the refusal is the entry check's and that nothing was confined; the report mapping is a pure function with tests; stale statements in ADR-0008, ADR-0011, the checklist and the design |

**Checks.** On Linux x86-64 as an unprivileged user, after the integration: the whole
workspace's tests with the same 22 known failures and no other; after the repairs,
`org-host` again with the same failing set. `cargo fmt --check`, the two CI lint
commands, the macOS-target lint of the whole workspace and of the example, and the
refusing-target lint and tests of both platform crates pass. After the admission
integration: the whole workspace's tests again, 22 known failures and no other; the
plain Linux lint of the whole workspace with no excluded crate passes with dead code
allowed and fails without it on three test helpers of `org-host` whose only callers
are tests that are macOS-only until X2 (`recovery_db_unlocked_for_test`,
`with_test_presence`, `recovery_for_test`). The list is at 41 files and 249
predicates.

**Not met from the plan's "Done when", and carried to X2.** "None of the moved files
has a platform predicate" holds for source; `server_check.rs`, `app_auth.rs`,
`supervisor/mod.rs`, `offices.rs`, `web/tests.rs` and `bootstrap/tests.rs` keep
predicates on macOS-only tests, which leave the list in X2 with the `testing` module.
"The Linux lint passes with no allowed lint and no excluded crate" waits for those
same tests to run on Linux.

**Owed from a Mac, by the Track A engineer, before this is relied on** (nothing of
this slice could be run; it was type-checked and linted for macOS, and the bridge was
not compiled):

1. `cargo test --locked --workspace --no-fail-fast` on macOS (CI does this on every
   push; record the run).
2. `cargo run --locked -p org-socket --example gateway_confinement`, outside a parent
   sandbox: the PASS line, with the threaded child refused at entry.
3. `cargo test -p org-platform confinement` and `cargo test -p org-platform-host`
   (the presence fixture test with ARC and ASan).
4. `otool -L` on `agentic-org-web`, `agentic-org-office` and `agent-os`: no Security,
   CoreFoundation, LocalAuthentication, Foundation or SystemConfiguration, as before.
5. A signed build: the host starts the web process, the Seatbelt probes pass, the
   handoff completes; `agentic-org recovery release` at the console shows the dialog,
   cancel denies, approve releases, and over SSH it is not local.
6. Admission, on a signed, hardened Developer ID build with `ORG_APP_TEAM_ID`: the
   host starts an office worker and logs no refusal; an unsigned or ad-hoc worker is
   refused ("it isn't the signed office server …; it was stopped before it ran and
   got nothing") and receives no key; a web image with `keychain-access-groups` or
   `get-task-allow` is refused at admission; with a selected release, an image with
   another CDHash is refused ("selected release image mismatch", the text since X2); a debug
   build with a pin is refused ("selected release requires platform-audited image");
   a signed CLI is admitted on `host.sock` and one signed by someone else is refused;
   `check_self` passes for the installed host; the release install's static check
   accepts the four executables and refuses a re-signed one; the key store selection
   still picks the Keychain.
7. First run ever of the macOS-only tests that moved: `cargo test --locked
   -p org-platform -p org-platform-host -p org-host --no-fail-fast` (CI does this).

**Release builds.** `org-platform-host`'s `test-support` feature (the scripted and
by-path test admissions) is enabled only by `org-host`'s dev-dependency. A command
that builds test targets together with the binaries (`cargo build --workspace
--all-targets --release`) would unify it into the host binary's platform crate;
nothing calls it there, but release binaries are built without test targets
(`cargo build --release -p <executable>`). The release procedure (L9) states that.

**For the Track A lead.** Commit `2d560d7c` (A1.3b, the SQLCipher removal) deleted
`crates/org-office/src/store/tests/sqlcipher_init.rs`, the only file that declared
the `opener_build*` test modules. `crates/org-office/src/store/tests/opener_build.rs`,
`opener_build_pins.rs` and `opener_build_tests.rs` are no longer compiled, so the
build-script fence they held runs nowhere. X1c moved the presence bridge out of
`crates/org-host/build.rs`; those files still pin the old script and were left
untouched. Delete them, or re-attach the fence and re-pin it.

## X1d · Credentials behind the crate

**Outcome.** An internal foundation; nothing a person can see. Both host stores sit
on one `Credentials` contract in `org_platform_host::credentials`.

**Built** by one worker in `x/x1d-credentials` (`ea3e3900`), integrated at `f2ef6ba6`:
the Data Protection Keychain as the macOS adapter (the two former `keychain.rs`
files; attributes and add/update sequences reported unchanged), one directory engine
that replaces the two development stores (the stricter of the two for both),
`keys::KeyStore` and `host_secrets::HostSecrets` as typed views with their name and
add-only rules. `org-host` no longer depends on an Apple framework crate. A Linux
release build keeps refusing until X3.

**Decisions.**

1. The contract is an enum, `CredentialLocation { Keychain, Directory }`: exactly one
   store is chosen. `data_root` and `expect_items` of the design belong to X3's
   release store.
2. The two development layouts on disk are unchanged
   (`<dir>/[<instance>/]<office>.key`; `<base>/host-secrets/<instance>/<name>`),
   because the macOS integration tests in `apps/agentic-org/tests/` pin them. X3
   chooses the release layout.
3. The office-keys view takes a shared lifetime lock and opens at first use (a debug
   key folder may be shared by several homes); host secrets take an exclusive one.
4. An add publishes by exclusive rename, as the design says, not by link and unlink.
5. The development office-keys folder gets the stricter checks: a wrong owner or
   mode is refused, never repaired.

**Checks.** After the integration, on Linux x86-64 as an unprivileged user: the whole
workspace's tests, 3,164 passed, the same 22 known failures and no other. `cargo fmt
--check`, the CI lint commands, the macOS-target lint of the whole workspace and the
refusing-target lint and tests of both platform crates pass. The list is at 37 files
and 221 predicates.

**Review.** A fresh read-only review of `fb171f20..f2ef6ba6` (interrupted once by a
usage limit, then resumed): accept. Every Keychain dictionary and call sequence was
compared entry by entry with the old code and with the framework crate's source and
is the same; no case creates a key where the old code refused, nor the reverse; the
directory engine keeps or tightens every check of the old host-secrets store. 5
minor, 5 nit. Repairs by the lead: the key store's documentation states the one
error that can follow a complete store; a location's `Debug` output carries no path.

**Deviations kept on purpose** (development stores only; nothing has shipped, so no
old layout or crash residue is handled): the lock file is `.lock`, where the old
host-secrets store used `lock`, so a development store made before this change shows
one ignored entry in its listing and does not exclude an older host; a temporary
left by a crash of the old add-only publish (two links) makes the new open refuse
that store. A file named like a key but with the wrong kind or mode fails the whole
listing, so the orphan sweep does nothing until it is fixed. Five predicates stay in
`host_secrets/tests.rs` and leave the list in X2 with the other test files.

**Owed from a Mac**, on a signed and entitled build: the 16 Keychain adapter tests and
the engine's tests on APFS, `cargo test -p agentic-org` (`tests/keys.rs`,
`instances.rs`, `host_control.rs`), creating an organization and restarting the
host with the same key, and that items written before this change are still found.

## X2 · The first office worker on Linux (in work)

**Split.** Two slices in order: the Linux admission adapter
(`crates/org-platform-host/src/admission/linux*`, `testing.rs`,
`crates/org-platform/src/image/linux.rs`), then the host composition (the admission
built once per host lifetime with the manifest digest, native clients by UID, the
host's own image, the Linux release rule, the root refusal and the non-dumpable host,
the script-worker tests on the by-path test admission, the host's tests on Linux, the
LNX-1 test with `agent-os`).

**Slice 1, the adapter: integrated at `00b4b61c`** (branch `x/x2-linux-admission`,
four commits ending at `643c22da`). On GNU/Linux `admission()` opens the pinned path
(`O_NOFOLLOW`, regular file, owned by the service user or root, not writable by group
or others), copies it into a sealed execute-only memory file while hashing it, and
compares the digest (a development build records it; a release without a digest
refuses with `ImageRefusal::Unpinned`). `spawn` starts the child from that file with
`clone(CLONE_PIDFD | SIGCHLD)` and `execveat(AT_EMPTY_PATH)`; `verify_reader` checks
`SO_PEERCRED` and the pidfd. `Peer` carries the Linux credentials.
`testing::admission::path_admission` starts a program by path for the script-worker
tests, under `test-support` only.

What the kernel taught the worker, different from the design text (the design is
updated when the slice's review is closed):

- `pidfd_send_signal(pidfd, 0)` succeeds on a zombie, so "still alive" also polls the
  pidfd for readability.
- `clone` (the raw system call), not `clone3`: x86-64 and aarch64 only, every other
  architecture refuses.
- A close-on-exec report pipe on descriptor 4 tells the parent which call failed in
  the child, so `spawn` returns only after `exec` succeeded (end of file on the pipe)
  and a failure names the call and the error; `close_range(5, ~0)` follows from it.
- There is nothing to hold: `Held::release` does nothing on Linux, the child already
  runs the verified bytes.

Lead's checks at integration: the three lint targets; `platform_conditionals` and
`libc_only`; unprivileged tests of `org-platform`, `org-platform-host` and `org-host`
(644 passed in `org-host`, the failing set identical to the baseline: the host does
not compose the adapter yet); the refusing target's tests. One host test now accepts
`NotFound` as well as `Unsupported` for an admission that cannot be built, because
the Linux adapter reads the image when the admission is built.

**Review of slice 1: ACCEPT** (fresh read-only reviewer, same model as the lead, on
`a83c4fad..00b4b61c`; it attacked the adapter on this kernel as uid 1000 and ran its
tests ten times). Verified by probe: after `exec` a same-user process is denied the
child's `mem`, `exe`, `environ`, `maps`, `fd` and `PTRACE_ATTACH`. Verified by
reading: no host descriptor can reach the worker whatever other threads do
(`close_range(5, ~0)` after the `dup3`s); the child between `clone` and `execveat` is
async-signal-safe and can only `_exit`; the report pipe cannot be forged by the
worker; the child is reaped once and its PID is not reused while the pidfd is held;
`test-support` is enabled by no production composition.

| Finding | Severity | What was done |
| --- | --- | --- |
| L1 Both guarantees depended on the host being non-dumpable, which nothing set: before `exec` the child is a dumpable copy of the host's memory, and before the seals a same-user process could write to the memory file through `/proc/<pid>/fd` (probe: 8 bytes written) | MAJOR, carried to the host slice | Closed in the adapter instead (L2, L3). The host slice still sets it at start, before a key is read. |
| L2 The digest was of the bytes written, not of the sealed object | MINOR | The adapter seals, then reads the object back through its descriptor and judges that digest; it must equal the digest of the bytes read. |
| L3 Nothing enforced non-dumpability | MINOR | `admission()` sets `PR_SET_DUMPABLE` 0 before the memory file is made and refuses if it does not hold; every start sets it again. Test `building_an_admission_closes_this_process_to_other_processes`. |
| L4 `GCONV_PATH` loads shared objects and was not refused | NIT | Refused with the loader variables (ADR decision 7 updated). |
| L5 The design said digest and size are compared | NIT | Design 3.1 says the digest covers every byte. |

Also recorded in design 3.1 from the review: `SO_PEERCRED` names who connected and
never changes, so the worker must never pass its rendezvous connection to a process
it starts. Not verified: kernels before 6.3 and containers that refuse one of the
calls (the decisions are unit-tested, the kernels were not available).

The repairs were made by the lead (the slice's author session ended with the usage
limit, so there is no original-author validation for this slice; the repair diff is
read by X2's fresh final review). Checks after the repairs: the adapter's tests ten
times unprivileged (82 tests, no failure) and once as root; lint for Linux, macOS and
the refusing target; the refusing target's tests. From the X1c admission review, also
done: the readmission comparison is `decision::readmitted`, tested on every target;
the presence adapter reads the peer through `admission::peer` and the admission
adapter module is private again. The error class of a check that may not start a
server moves with the host slice (it is host code).

**Slice 2, the host composition: integrated** (branch `x/x2-host`, eight commits ending
at `83e2ab02`; its production-path test, a real host bringing a real worker to
`ready`, first passed at its first commit). What it built:

- One admission per host lifetime, built on its own thread when the supervisor is made
  (a debug build's hash of the image took over a second and three supervision tests
  assert that a start does not wait); a start waits for a build under way; a failed
  build is logged and asked again by the next start; `host.sock` serves meanwhile.
- Both image identities from the verified manifest (`Pin { sha256, macos_cdhash }`),
  the host's own image by its digest, and the Linux static rule (ELF header and the
  signed digest), chosen by the manifest's platform, with no predicate.
- Native clients on Linux admitted by their user (`app_auth`, one path; the platform
  marks such an identity with `Peer::uid`), and a command accepting a host of its own
  user (`human::verify_host`, not in the brief: without it every Linux command refuses
  the host).
- `Config::refuse_root`, called by `serve`; `org_platform::process::harden_self` first
  in every host start (no core dumps; on Linux non-dumpable, read back).
- Script workers through `ServerCheck::ByPath` in unit tests only; integration tests
  start only compiled programs (`wrapper()` is gone); a compiled recording worker for
  unit tests that need an admitted child.
- `apps/agentic-org/tests/lnx1.rs` (LNX-1, on both systems) and the two refusal tests
  through the real adapter and the host's real supervisor.
- CI: no package excluded on Linux. The list of platform predicates: 18 files, 109
  predicates (from 35 and 182).

Lead's decisions on what the worker raised:

| Question | Decision |
| --- | --- |
| `agent-os` cannot reach the socket of a registered instance on any system (it looks in `<office home>/run`, the registered host serves in the instance's temporary folder); the LNX-1 test hard-links the socket into an office home of its own | The bridge stays, stated in the test and in FC-164. The product fix belongs to the agent launch binding (Track A, C increments), not to Track X. Reported to the owner. |
| A group-writable image is refused, so a development host on a stock Ubuntu account (umask `002`) refuses its own cargo-built worker | The private-group exception of the credential store applies to the image file as well; built with X3, where the rule is written. Until then `DEVELOPMENT.md` says what to do. |
| A debug build hashes slowly | `[profile.dev.package.sha2] opt-level = 3` in the workspace manifest. The background build stays. |
| Root for `admin reset` is not admitted | X5, with the container's `docker exec -u 0` path. |
| Two host tests are macOS adapter tests by nature and keep their predicate | They move into `org-platform-host`'s macOS tests in X6. |
| The setup tests fail when the whole suite is run as root | Accepted: the host refuses root; tests run as an ordinary user, as in CI. |

**Lead's verification of slice 2 at `e8cf07fb`:** lint for Linux (whole workspace),
macOS and the refusing target; `platform_conditionals`, `libc_only`; the whole
workspace unprivileged: 69 test binaries, 3,443 tests passed, 4 failed, none of them
this slice's (`dependency_closure` needs the network; two `org-socket` tests need
IPv6; one `web::tests` test whose premise X4 ended and whose rewrite is on X4's
branch). `lnx1` passed.

**Review of slice 2: CHANGES_REQUIRED** (fresh read-only reviewer, same model as the
lead, on `cbb9b515..e8cf07fb` and the lead's adapter repair `db6b3bfe`). It ran the
host's and the app's suites (957 passed, the one known failure), LNX-1 ten times
under load, the refusal and admission tests ten times each, and probed the repair:
with the process non-dumpable a same-user process is denied the unsealed memory
file and the memory of a child before `exec`; without it both are open. It found no
path by which a process that is not the verified image gets a secret, no release
image admitted without its publisher identity, and no macOS regression by reading.
LNX-1 it judged honestly proved on Linux, with the parts that are not the product
route named: a debug build with a development key folder, an instance from the
fixture instead of `setup`, the office stored in-process, `agent-os` run by hand,
and the linked socket.

| Finding | Severity | State |
| --- | --- | --- |
| H1 Two host tests lost a macOS assertion ("either outcome passes") and two more hang on an unnamed platform fact | MAJOR by the rule that a test is never weakened | With the author: one expected outcome per system, chosen by a named testing fact |
| H2 `admin.link.initial` needs no session: on Linux any process of the service user that names the setup operation gets the one initial link | MINOR, inside the accepted threat model | Stated in ADR-0017 decision 11 and design section 5; the host's log line is corrected by the author |
| H3 A command would send the administrator's password to a host it knows only by its user, once Linux presence exists | MINOR, latent | With the author: the command refuses to send a secret to such a host |
| H4 The Linux CI test job is red on one web test whose premise X4 ended | MINOR | Ends with the merge of X4's host branch |
| H5 The host is open between `exec` and its hardening call | MINOR | With the author: first act of `serve`; the residual is in design section 5 |
| H6 A Linux release host needs the macOS team identifier to start anything | MINOR | With the author: the admission contract says whether the system checks a code signature; where it does not, a release is checked by its pin alone |
| H7 `setup` does not refuse root | MINOR | With the author |
| H8 The macOS adapter treats a digest-only pin as no pin | NIT, unreachable | With the author: refused |
| H9 Stale texts | NIT | Fixed by the lead and the author |
| H10 Tests that skip as root pass silently; the root refusal test never runs in CI | NIT | With the author |

**Repairs of slice 2: integrated** (branch `x/x2-repairs`, three commits ending at
`a494befe`, by the slice's author, who confirmed all ten findings against the source
and rejected none). Every test that differs by system takes its one expected outcome
from a named platform fact and fails on any other
(`admission::image_identity()`: a code signature or verified bytes;
`fs::testing::device_flushes_per_full_sync`; `testing::names::equivalent_spellings_are_one_name`).
`verify_host` says whether it knows the host by its image or by its user alone, and
the recovery command refuses to send a password to the second kind. The executable
hardens itself before it reads its arguments (every command, not only `serve`).
`setup` refuses root. A release build on a system that checks verified bytes is
checked by its pin alone (`ServerCheck::Published`); macOS still refuses a release
without a team identifier, with the same text. The macOS decisions refuse a release
image pinned only by its bytes. Open from the repairs: the web process's check still
applies the signature rule on every system (one line and three tests in files X4's
branch owns; done at X4's merge), and the office worker's own `setrlimit` could use
the platform call.

Lead's checks at the merge: lint for Linux (whole workspace), macOS and the refusing
target; the refusing target's tests; `org-platform`, `org-platform-host`, `org-host`
and `agentic-org` unprivileged: 969 passed, LNX-1 passed. One failure is the known
web test. **A second is new and intermittent:**
`apps/agentic-org` `service::a_list_answer_never_arrives_after_an_event_it_doesnt_reflect`
(twelve offices started at once by an in-process host while a client lists them)
went silent for 30 s in 3 of 6 runs with the machine under load (load average 7 to
8, three other builds) and passed in the others in under 2 s. It ran on Linux for the
first time with slice 2.

**Cause found: a race in the test's own bookkeeping** (branch `x/x2-hang`,
`d9613a56`, merged at `82fbc3d1`). Reproduced 3 of 31 on the loaded machine. The hung
process, read with a debugger: the run was complete (10,675 calls sent and answered,
all twelve offices active), the writer thread was gone, the test thread was parked in
its 30 s receive, every host thread was idle in its normal wait, and all twelve
children ran the sealed image. The reader ended on "the writer has finished and every
call is answered"; it could read the last answer before the writer had stored its
count, and then wait for a message that never comes. Forced with a 200 ms delay: 3 of
3. The writer now names its last call before it makes it; no timeout changed; 30 of
30 under load afterwards. The race is in system-neutral test code, so macOS had it
too. The test is Track A's: what it asserts about ordering is unchanged.

Not verified by the reviewer: anything on macOS at run time (first-run risks it
names: hard-linking a socket in LNX-1, path lengths under `/private/tmp`, the private
program copies, the background admission build), a release-profile build on either
system, kernels before 6.3.

## X4 · The web process confined on Linux (in work)

**Split.** The adapter (`crates/org-platform/src/confinement/linux*`, proved with the
confinement example and with the real web process under a stand-in host), then the
host slice (`org-host/src/web.rs` and its tests, the web process's refusal channel).

**Slice 1, the adapter: integrated at `458ed8b1`** (branch `x/x4-confinement`, six
commits ending at `def7ca7a`). `confine_web` on GNU/Linux, in order: one allocator
arena (`mallopt(M_ARENA_MAX, 1)`); no core dumps and a non-dumpable process; one
thread and exactly descriptors 0 to 2, or it refuses with nothing changed; connect to
the rendezvous; no-new-privileges; a Landlock ruleset that handles every right of the
kernel's ABI (3 or higher) with no rule; a seccomp filter that kills the process on
any call outside its list. A failed step drops the stream and refuses with the call
and the error's name. The host side: `web_confinement_supported()` (an `io::Result`
with the reason), `verify_ready_report` (Landlock ABI 3 or higher), `verify_web`
(`NoNewPrivs: 1`, `Seccomp: 2` and more filters than the host itself has, read from
`/proc/<pid>/status`).

What the kernel and glibc taught the worker, beyond the design text:

- glibc opens `/proc/sys/vm/overcommit_memory` the first time a heap of a non-main
  allocator arena shrinks. Under the filter that is a kill: the real web process died
  in 3 of 3 runs of 224 concurrent requests. Hence the single arena. It holds only
  when `confine_web` runs before the process ever had a second thread (the contract);
  otherwise the result is the kill, never an unconfined process. Cost: one allocator
  lock for all threads; the per-thread caches remain.
- Calls the design's list lacked: `getpeername` (the web process checks its control
  connection), `getpid` (glibc's `abort`), `restart_syscall` (the kernel, after a
  stop and continue during a timed `poll`).
- Three rules stricter than the design, each one table entry: `fcntl` only with
  `F_GETFD`, `F_GETFL` or `F_DUPFD_CLOEXEC` (with `F_SETOWN`, `F_SETSIG` and `O_ASYNC`
  a process makes the kernel signal any process of its user; a probe killed one;
  Landlock closes that only from ABI 6); `clone` only with glibc's exact thread flags;
  no `mmap` or `mprotect` with `PROT_EXEC`.
- `clone3` answers `ENOSYS`, so glibc falls back to `clone`, whose flags can be read.
- Landlock alone does not deny `chmod`, `socket()` or connecting to a named Unix
  socket on the ABI floor: the two layers are needed together.
- `Seccomp_filters` counts inherited filters, so under a container profile "one
  filter" proves nothing; `verify_web` compares with the host's own count.
- `verify_web` keeps a PID (the admission's child type is in the host-only crate);
  the caller holds the child across the call and checks afterwards that it lives.
- aarch64's table is derived and checked against `libc` at build time, never run.

Lead's decisions on the worker's six questions: all accepted as proposed (single
arena by `mallopt`; the reason in an `io::Error`; the three stricter rules; the
example runs in macOS CI through a new `org-socket` test; the PID stays; the filter
count rule).

Lead's checks at integration: lint for Linux (whole workspace), macOS and the
refusing target; `platform_conditionals`, `libc_only`; unprivileged tests of
`org-platform`, `org-socket`, `org-host` and `agentic-org-web`: 892 passed; against
the merged tree's baseline one test now passes
(`web::tests::refused_web_start_never_becomes_ready`) and one now fails
(`the_supervisor_refuses_a_platform_without_a_confinement_adapter`, whose premise
ended; it changes in the host slice). The worker reports the real, unmodified web
process serving HTTPS under the filter with a stand-in host (TLS 1.3 and 1.2, a
3 MiB asset, 224 concurrent requests, timeouts, stop and continue, clean exit), 3 of
3 runs; its harness and logs were in its worktree and were deleted with it by the
lead's cleanup, so that evidence is the worker's receipt only. The reviewer was told
so and re-derives what it can.

**Review of slice 1: CHANGES_REQUIRED** (fresh read-only reviewer, same model as the
lead, on `db6b3bfe..458ed8b1`). It ran the confinement tests ten times (all passed),
rebuilt the real web process and drove it under the filter with a stand-in host
(HTTPS on TLS 1.3 and 1.2, a 3 MiB asset, 224 concurrent requests, about 100,000
keep-alive requests per run, exit 0, only listed calls in the trace), reproduced the
allocator-arena open, and read the filter program instruction by instruction. An
automated safety stop interrupted it when it began to write an attack probe, so its
one MAJOR finding is from reading the kernel's behaviour, not from a probe; the
author checks it on the kernel.

| Finding | Severity | State |
| --- | --- | --- |
| C1 `sendto` and `sendmsg` take any destination and flags: TCP's send-time connect (`MSG_FASTOPEN`) and a datagram send to a named Unix socket are not `connect` calls, so neither the filter nor Landlock sees them | MAJOR | With the author: `sendto` only without a destination and with known flags, `sendmsg` removed, kernel cases added |
| C2 `mremap` is killed, and glibc grows a large block with it: a buffer pushed past about 128 KiB kills a healthy process (probed) | MINOR today, no current path in the web process | With the author: allowed, growth cases added |
| C3 `sched_yield` is killed; a contended standard channel calls it (probed on a copy of the web process's pool, not reproduced in the web process) | MINOR | With the author: allowed, case added |
| C4 The web process prints only "authenticated startup unavailable" | MINOR | The host slice (already its item 1) |
| C5 The example's threaded role no longer isolates the macOS rule "a process that ever had a thread" | MINOR | With the author |
| C6 Design 3.3, the checklist row and this record were behind the code | MINOR | This record was updated after the reviewed commit; design 3.3 and FC-164 are rewritten by the lead when the repairs settle the list |
| C7 Install the filter with thread synchronisation | NIT | With the author |
| C8, C9 Calls a confined process must not make; small information the list still gives | NIT | With the author (documentation, or one entry each) |

Verified by the reviewer and found correct: the architecture and x32 checks, the
default kill, both halves of 64-bit arguments where the kernel reads 64 bits, the
exact thread flags, the Landlock masks and sizes per ABI, the clamp to ABI 7, that no
path returns confined-in-part, that the process name cannot spoof `/proc` fields, and
that the test seams are compiled only for tests. Not verified: C1 on the kernel,
aarch64 at run time, Landlock ABI 3 to 6 on a real kernel, a container runtime's
profile, Docker Desktop's kernel, the example on macOS.

**Repairs of slice 1 and slice 2, the host: integrated at `4823b23a`** (branch
`x/x4-host`, ten commits ending at `eed52d58`, by the adapter's author).

- Review repairs: C1 was real on this kernel: with `tcp_fastopen` 1, a `sendto` with
  `MSG_FASTOPEN` and a destination connected and delivered data on a fresh TCP socket
  and on a listener after `shutdown`, and still did under Landlock ABI 7 with TCP
  connect denied. `sendto` now passes only without a destination and with two known
  flags; `sendmsg` is killed. C2 and C3: `mremap` (two flags) and `sched_yield` are
  allowed. C5: the example proves each thread refusal by itself. C7: the filter is
  installed for every thread. C8, C9: the contract lists what a confined process must
  not call; `clock_gettime` takes only fixed clocks.
- Landlock optional (owner direction below): applied from ABI 1 where the kernel
  offers it; any failure of its three calls is kept as a reason and the filter is
  installed all the same. The forbidden list (42 calls), the working roles and the
  host's check run in four layerings (both layers; Landlock refused at each of its
  three calls). Asked whether any allowed call was stopped by Landlock alone: 22
  candidate operations gave identical results unconfined, under Landlock alone, under
  both and under the filter alone; none found. What is lost without Landlock is the
  backstop for a mistake in the list (shown by adding `connect` to it).
- Host: the refusal's reason reaches the host log through one bounded line on the web
  process's standard error; the host logs once per start what confines the web
  process; the child must be running after `verify_web`; the environment is empty by
  name; a web process started from a sealed image takes its release root from its
  first argument (before, it never reached confinement under the Linux admission).
- Lead's decisions: the reason stays in the log (a status field is Track A's
  contract); the duplicated refusal prefix stays, pinned by a test on each side; the
  rule that the host exits on a descriptor arriving before the handoff
  acknowledgement is unchanged.
- Lead's checks at the merge: lint for the three targets; 1,094 tests of the
  platform, socket, host, web and app crates passed; failing were the offline and
  IPv6 tests, the service test whose race is described under X2, and one setup test
  whose fixed port another agent's run held (8 of 8 alone, three times).

**In work (worktree `x/x4-web`, the same author):** the real-host test of LNX-3 in
its amended meaning; one web admission per host lifetime; the web check following the
system's image identity; the 13 web tests that still start a script through the
macOS admission, on a compiled fake web child. Then one fresh review of all of X4's
host work (`7ffc5e68..4823b23a` and this slice).

**Earlier plan for slice 2, kept for the record:** in worktree `x/x4-host` (the adapter's author): the
refusal's reason from the adapter and from the web process to the host log and the
web service's status (LNX-3 "reports the reason" holds today in the adapter and the
example, not through the host); liveness of the child around `verify_web`; an explicit
environment for the web process; the web tests on both systems. The real-host test
of LNX-3 follows the integration of X2's host slice.

Not run anywhere yet: a release build, the container image's glibc, aarch64.

## X3 · Host secrets on Linux (integrated, under review)

One worker, branch `x/x3-credentials`, seven commits ending at `00d1d70e`, merged at
`d13aa040`. What it built is in design 3.2 ("Built in X3"). Its production-path test
(the store opened through the public contract on this machine's ext4, a key added
through the host's key view and loaded through a second handle) passed first.

Lead's decisions on what the worker raised:

| Question | Decision |
| --- | --- |
| Where `expect_items` is checked, what an item is, and the host's source of "keys exist" (office folders) | Accepted as built, subject to the review (it is asked whether an instance with wrapped records can ever start on an empty key directory and make fresh keys) |
| "No other members" of a private group: the member list is empty or names only the user; an account with that primary group is not looked for | Accepted for now and put to the reviewer, because the same rule now admits a group-writable image |
| The lock is `.lock`; ancestors are opened with `O_PATH`; one error carries a path | Accepted; design 3.2 says so |
| A whole release-profile host on a release directory runs in no test | X5: the container is the first place where it can run |
| The runner hid the from-root acceptance tests (it made the build output writable by everyone, which the ancestor rule refuses on purpose) | The runner now leaves the build output unwritable by others and gives the test user only `target/tmp`; both from-root tests then open the store as uid 1000 (seen by the lead) |
| `mount_point_required` is always false | X5 adds the record's service mode |
| Interactive setup stops on Linux before it changes anything: no service adapter, and no disk-encryption adapter (not in any plan before) | Both are X5's |

Lead's checks at the merge: lint for Linux (whole workspace), macOS and the refusing
target; guard tests; the refusing target's tests; `org-platform`, `org-platform-host`,
`org-host` and `agentic-org` unprivileged: 1,053 passed, none failed. The list of
platform predicates: 17 files, 104 predicates.

**Repairs: integrated at `ce7dd5a1`** (branch `x/x3-repairs`, two commits ending at
`fac893ed`, by the store's author, who confirmed S1 on a real kernel and rejected no
finding; the table below says what each became). Lead's checks: lint for the three
targets, guard tests, 971 tests of the platform and host crates unprivileged with
none failing, and the bind-mount cases as root. From the author's reading of the
source, asked by the reviewer: when the host-secrets directory alone is lost, nothing
is regenerated for an existing office (writer custody and a pending recovery code
refuse), but the start itself does not refuse; a marker in the data root
(`selected-writer.required`) could make `expect_items` exact per item store. Open.

**Defect in the S1 repair, found by the lead's full run at `8ab12056`** (3,563 tests
passed; 28 failed, of them 24 this one): with a Docker daemon running on the machine,
every store is refused with `Undetermined(Malformed)`. Docker mounts its network
namespaces (`57 27 0:4 net:[4026532268] /run/docker/netns/… - nsfs nsfs rw`); the root
of such a mount is not a path, the mount-table reader called the line malformed, and
one unreadable line refused everything. It passed in the author's runs and in the
lead's check of the merge because no daemon was running then. So a release host on
any machine that runs Docker could not open its store. **Fixed, integrated** (branch `x/x3-mounts`, `9d364c66`, by the author): the two
directories are found in the table by the mount id the kernel gives for their held
descriptors; of every other line only the mount id is read, so no other line can
refuse a store, and a line whose id cannot be read still refuses the table. The
fixtures are real tables (this machine with Docker; two containers). With a required
mount point the store is also compared with what the data root's mount shows, so
`-v D:/data -v D/secrets:/secrets` is refused (`InsideDataVolume`); run by the author
inside a container as uid 61000: two volumes accepted, a key added and read back; the
three nested layouts refused. Lead's checks with a container running (the mount
table then has the `nsfs` line): lint for the three targets; 1,003 tests of the
platform and host crates unprivileged and the store's 92 as root, none failing. The other failures of that run: the offline and IPv6
tests, and `supervisor::channels::tests::a_discard_stream_ends_at_a_non_server_event`
(Track A's test; X5's worker saw it fail twice and pass once: intermittent, cause not
yet looked for).

**Review: CHANGES_REQUIRED** (fresh read-only reviewer, same model as the lead, on
`4823b23a..d13aa040`). It built trees as root on ext4, tmpfs and overlay and attacked
them as three other users; ran the store's tests ten times as uid 1000 and as root
with nothing left behind; and read the release-only branches that no test executes.

| Finding | Severity | State |
| --- | --- | --- |
| S1 A store that is a bind mount of a subdirectory of the data root is accepted (and the reverse): the `..` walk from a mount root leaves through the mount point, so the chains never meet, and the mount-point rule passes too. The container mistake `-v ./agentic:/data -v ./agentic/secrets:/secrets` puts every key in the data volume, silently (probed) | MAJOR | With the author: containment decided on paths inside the filesystem, read from the mount table; refused when it cannot be determined |
| S2 The private-group rule admits a group another account has as its primary group; that account can read and plant nothing but can roll the store back to an older copy, and can rewrite a group-writable development image (probed) | MINOR: root must have made such an account | Not closed (it needs every account enumerated, which is unbounded under a directory service); stated in the code and in design section 5 |
| S3 An item store with a wrong mode is reported as "credentials missing", and any file in any `0700` subdirectory counts as an item | MINOR | With the author: the host names its item stores; a wrong mode is its own refusal; only real items count |
| S4 Setup creates the directories by path before any check and can leave them behind on a refusal, and records an unnormalized path | MINOR, latent until setup works on Linux | With X5's worker, who owns `setup_flow.rs` now |
| S5 A POSIX ACL on an ancestor admitted by the group-write exception is not seen (probed) | MINOR: only the owner or root can set it | With the author: such a directory, and such an image, must carry no access ACL |
| S6 Texts and the mode of a lock made under a restrictive umask | NIT | With the author |
| S7 Commands now create the application folder on Linux before they fail on a missing record | NIT | Noted, not changed |
| S8 A macOS record now carries `"credentials": null`, which an older binary would reject | NIT | Rejected: nothing has shipped |
| S9 The account helper has one adapter | NIT | With the author: the crate's three-adapter layout |
| S10 The LNX-4 test is at the level of the two views | NIT | X5: LNX-4 on the instance the container sets up |

Found correct, by probe: every spelling, symbolic link, ancestor, mode, filesystem,
mount-point, empty-store and lock case of design 3.2, each refusal naming the right
directory (`/tmp`, `/var/tmp` and `/dev/shm` are always refused as ancestors, as the
design intends). By reading: every check is made on the descriptor the next `openat`
uses; no office with records ever gets a fresh key (`provisioning.rs`,
`supervisor/office.rs` end with `KeyMissing`); a release host has no fallback to a
development store, to the environment, or to no store; macOS is unchanged in
substance. Not verified: macOS at run time, a release-profile host, XFS/Btrfs/F2FS
and network filesystems live, directory services, an actual race, and whether a lost
host-secrets directory alone is noticed (asked of the author).

## X5 · The service on Linux and the container (built, not integrated)

One worker, branch `x/x5-service`, six commits on `82fbc3d1` ending at `a31873e8`.
It is not merged: it needs the newer `v1-integration` under it (the merge is in work
on `x/merge-a`), its fresh review (in progress on `a31873e8`), and the decisions
below.

What it built: the systemd user-unit adapter of `org_platform::service` and the
foreground mode (nothing registered); a Linux disk-encryption probe
(`org_platform::disk`: on only when the device is a `dm-crypt` target, otherwise off
or not known); non-interactive setup from a strict file and `serve --setup <file>`;
the record's service mode and the mount-point rule from it; root's
`admin reset --user` (`host.sock` admits UID 0 for `hello` and `admin.reset` only;
a same-user peer is refused for it on Linux); the X3 review's S4 (setup creates the
credential directory below judged ancestors and removes what it made on a refusal);
`deploy/container/` (Dockerfile on a Debian base, `setup.json`, optional
`seccomp.json` and AppArmor profile, README, the LNX-2 script).

What the worker measured in Docker (29.10, kernel 6.18, default profiles), 3 of 3
runs of `deploy/container/lnx2.sh`: a plain `docker run -d -p 127.0.0.1:8443:8443
<image>` sets the instance up, starts a worker, `agent-os` sends and reads, root gets
the registration link, `docker stop` returns in under half a second with exit code 0,
and a replaced container on the same two volumes is the same instance. Image 191 MB.
`/data` and `/secrets` are ext4 with named and with anonymous volumes and pass the
mount-point rule; an overlay directory, a plain directory, a root-owned or `0750`
store are each refused by their rule.

**What does not work, from the worker's receipt (not yet verified by the lead):**

| Gap | What it needs |
| --- | --- |
| The browser is not served from the container. The web process serves plain HTTP on loopback only (`web.rs` requires a verified HTTP loopback listener; `tls.fetch` answers only that profile; the `tls_pair` custody is unused), a published port is not loopback, and no workspace dependency can make a certificate | Either HTTPS serving in the host and the web process (a product feature that exists on no system yet: FC-017, Track A's area) with a certificate made at first start, or plain HTTP allowed on the container's interface. **Decided by the owner, 2026-11-18: plain HTTP for now** (a listener mode only the foreground mode may choose; X5's worker is building it), **and HTTPS with the instance's own certificate recorded as pending work** (implementation plan X7, ADR-0017 decision 10). Owed after the merge of `x/merge-a`: the same note in `docs/FEATURES.md` rows FC-164 and FC-017 |
| The container uses the development key store (`/secrets/dev-keys`), because the image holds a debug build; a release directory is used only by a release-profile host | A release image needs the release pipeline (a signed manifest and pinned runtime); until then the release store runs in no host anywhere |
| A new, empty secrets volume does not stop the start in that image: the host serves and reports the office as having no usable key; no key is made | Follows from the development store |
| `-v D:/data -v D/secrets:/secrets` is not refused: the store is beside the instance root (`/data/instance`), not inside it | The store must be compared with the data volume, not only with the instance root: X3's author, after the merge |
| Real systemd never ran (no user bus in the sandbox) | A CI runner or a machine with a user manager; an opt-in test exists |

**Review of X5: CHANGES_REQUIRED** (fresh read-only reviewer, same model as the lead,
on `82fbc3d1..a31873e8`; it built the image and attacked the running container as the
service user, as root and as a third user, with hostile volume layouts and 19
malformed setup files). It confirmed the plain `docker run`, an empty `docker diff`,
the owner-only modes in both volumes, every volume refusal, root's client against a
hostile `/data` (links, forged folders: refused; no root-owned file appears), the
image's content and the unit-file grammar, and each of the worker's stated gaps. It
found no way for a non-root user to gain administrator authority beyond what design
section 5 accepts. All findings are with the author:

| Finding | Severity |
| --- | --- |
| V1 A refused non-interactive setup leaves its frozen record behind, and every later start with a corrected file is refused: the operator must delete the data volume (shown with a typo in the credentials path) | MAJOR |
| V2 Stop is not bounded at 8 s: with a worker that ignores the stop, `docker stop` took 12.3 s and the runtime killed the host; the README says the grace is never needed | MAJOR |
| V3 The README says the store's directory is checked at every start; only first-start setup checks it (a tmpfs `/secrets` on a later start is accepted) | MINOR |
| V4 Root's `admin reset --user` prints whatever link a host of the service user returns: another origin and raw terminal escape sequences reached root's terminal | MINOR (security) |
| V5 In foreground mode the service user can take the first registration link itself, though decision 10 says it comes only from root | MINOR |
| V6 The disk probe says "on" for dm-integrity and dm-verity targets and for a Btrfs with one encrypted member | MINOR |
| V7 The systemd unit carries only `HOME`; with an XDG variable in the shell that ran setup the service looks elsewhere and never becomes ready | MINOR |
| V8 systemd drop-ins that replace `ExecStart` are not seen | MINOR |
| V9 SIGN-33's wording | MINOR |
| V10 After a test run the image can pack a binary built with the test seams | MINOR |
| V11 `https` is accepted in a setup file though no build serves it | MINOR: refused until X7, by the owner's decision |

Also from the review: on macOS `account_folders()` newly refuses a home folder that is
a symbolic link, which no design asks for (to be undone); LNX-2 rests on test
scaffolding (an office planted by an example program and the linked socket), because
no product command creates an office in the container. Not verified by anyone: real
systemd, AppArmor, Docker Desktop on a Mac, a release-build host.

**Plain HTTP and the review's repairs: built on the branch (`7e52887f`), not
integrated.** The author confirmed all eleven findings and rejected none.

- A listener mode of its own, `http_published`: the listen address is not loopback,
  the origin's host is exactly `localhost`, `127.0.0.1` or `[::1]`, and only an
  instance whose service mode is `foreground` may have it; never implied by another
  mode; a record edited by hand does not start a host. It crosses one contract, the
  `tls.fetch` reply between host and web process (a new variant with no pair; the
  vectors gained rows). The web process serves it as it serves loopback HTTP.
- In the container (Debian 13, default profiles): `GET /api/v1/version` answers 200,
  `GET /` serves the UI's index, another `Host` is refused with 400, and the host log
  says `web process started, confined by the seccomp filter and Landlock ABI 7`, with
  no killed system call: the first run of the web process's confinement inside a
  container.
- What the product itself lacks there: `GET /register` is 404 (the registration page
  is not built), so the link root's command prints cannot be used yet.
- V1: everything that can refuse is asked before the record is frozen, and a record
  this run froze is removed when the reservation refuses. V2: the stop grace is 5 s
  (a worker that ignores the stop is killed; exit 0 in about 5.4 s). V3: a foreground
  host judges the recorded directory at every start. V4: only this instance's own
  link is printed. V5: no initial link for a foreground instance. V6 to V11 as the
  review asked; `https` is refused at setup until X7.
- Lead's decisions: the image's origin is `http://127.0.0.1:8443`, not `localhost`
  (a name may resolve to `::1`, where another local process can listen); the limits
  the author names (a registered setup that fails after the registry reserved keeps
  its record; no Docker case for V4 and V5; two XDG names hard-coded beside the
  platform's list) are written down as known limits.
- Next: the branch takes the Track X tip (`daa46cdf`: the other team's changes, the
  mount-table fix, X4's last slice), its checks and the container script run on that,
  then a fresh review of what the first X5 review has not read.

Lead's decisions so far: `root` at `/data/instance` with the service user's home at
`/data` is accepted (the host refuses overlapping instance and locator roots);
`hello` for root is accepted as the handshake of `admin.reset`, subject to the review.

## Merges with v1-integration

**`42dd1322`, 2026-11-18.** Track A had changed 230 files since `831f10ca`, among them
the host files Track X moved behind the platform crates. Resolved by the lead:

- `supervisor/child.rs`, `supervisor/channels.rs`: A1.5 made the rendezvous the only
  launch form and added a discard handoff (`Broker::ready`, `discard`,
  `discard_handoff`). They now take the admission and the child of the admission
  contract instead of a pid, an audit token and the server check. `Broker::accept` and
  `Broker::discard` take `&mut Child` and ask it themselves whether the worker has
  exited while they wait for its connection (Track A passed a closure over the same
  child, which the contract's `try_wait(&mut self)` does not allow next to a shared
  borrow).
- `host_secrets.rs`: the two instance checks of the credential views accept a
  canonical UUIDv7, as Track A's stores do (`org_proto::is_uuid_v7`); the Keychain
  adapter in `org-platform-host` keeps no instance rule (the view owns it).
- `human.rs` `paths()`: Track A's `ORG_DEV_ACCOUNT_BASE` development setting is kept,
  and the system gate after it is `credentials::protected_store()`, not a predicate.
- `bootstrap.rs` and `host_secrets/dev.rs` are gone on both sides.
- Three predicates Track A had added outside the platform crates are removed without
  a new one: `admin.rs` `admit` (the peer check is already system-neutral: a system
  that cannot identify a peer refuses), `lib.rs` (`setup_flow` builds everywhere; its
  refusals come from the service adapter), and a test-only import in `offices.rs`.
- The list of platform predicates fell to 35 files and 182 predicates.
  `apps/agentic-org/tests/supervision.rs` is at 18, one more than before: Track A
  added a macOS-only test of the worker's environment. It falls with the rest of that
  file in X2's host slice.
- The whole workspace lints clean on Linux without any allowance (the three test
  helpers that were dead code on Linux are gone with `bootstrap.rs`), so CI lints the
  host with the rest of the workspace and the separate host lint step is removed.
- `docs/FEATURES.md` and `docs/FEATURE-AREAS.md`: the summary rows are recounted from
  the rows (In progress 64, Planned 53; AREA-02 16 and 3). Track A's summary was one
  behind its own rows (FC-159 had moved to In progress).

**`f7e60098` and `1b260dee`, 2026-11-18.** Track A had 82 more commits (and 12 more,
documents and one command file, while the merge was made). A trial merge conflicted
in 21 files. It was made in its own worktree (`x/merge-a`, `e28aee39`) by a worker
with a written analysis of Track A's changes (the merge guide in the lead's
scratchpad), then brought onto this branch (`5bcbfbfb`) with the 12 newer commits
merged on top. How it was resolved:

- Track X's structure in every host file; Track A's renames (`OfficeSecretsKey`,
  `OfficeSecretsKeys`, `means_no_key`, the launch contract's module path) through all
  of them, including Track X-only files git did not report.
- Track A's three behaviour changes carried over: a start without a usable key still
  spawns the server (`Mode::Office(id, Option<..>)`, `HandoffSecrets`; `key_missing`
  is gone); `web::Running::start` takes a wait; the web process starts without an
  Active HQ, so on Linux it goes through the admission and the filter from host start
  (two LNX-3 assertions follow: the state before an HQ is `Running`, or `Failed`).
- The 13 platform predicates Track A had added in the key store are gone with its
  four files (`keys/{dev,keychain,platform,unsupported}.rs`); the store asks
  `credentials::protected_store()`.
- The one difference in intent, decided by the lead for Track A's behaviour: on a
  system with no protected key store the host starts and each call that needs an
  office key is refused (`KeyStoreError::Unsupported`), chosen before any Keychain
  check; the host-secrets store still refuses the start there, as Track A's did.
- All 102 tests Track A added are in the tree by name. Its script-worker test runs
  through the by-path test admission. Its five ignored real-process tests were run
  once on Linux and pass, its session routes under the seccomp filter.
- A key file that is a FIFO, a symbolic link or has a wrong mode is a storage error
  in Track X's store, not a null-key start as in Track A's old development store.
- The list of platform predicates: 16 files, 78 predicates.
- Feature tables recounted from the rows: 187 rows; Done 9, In progress 66, Planned
  68, Blocked 1, Deferred 43; AREA-02 22 rows.

The worker's checks on `e28aee39`: lint for the three targets, guard tests, the whole
workspace unprivileged (71 binaries, 3,639 passed; failing only the two IPv6 tests),
the app and host loops five times. The lead's checks on the combined tree are in the
commit that merges `1b260dee`.

**Lead's whole-workspace run of `daa46cdf`** (unprivileged; the machine at load average
15 on 4 cores, with three reviewers and a worker building): 71 test binaries, 3,646
tests passed, 4 failed. Two need IPv6. One setup test found its fixed port 7421 held
by another agent's run. One is new:
`web_process_tests::lnx_3_under_a_profile_that_refuses_landlock_the_real_web_process_serves_by_the_filter_alone`
ended with "HQ provisioning deadline": the web process had started (`state before HQ:
Running`), and the real office worker the fixture adds beside it missed the host's
5 s launch deadline again and again until the test's wait ran out. Run alone under
the same load the four `lnx_3_*` tests pass (105 s). The web process no longer needs
an HQ to start; the fixture still makes one. Whether this can fail on a CI runner,
and what to do, is put to the reviewer of that part.

Three fresh reviewers are reading `daa46cdf` in parallel, one part each: the merges
with `v1-integration` and what they change for the other team; the web process's
confinement and its start by the host as they are now; the repairs of X2's host slice
and of X3. The merge into `v1-integration` needs all three to accept.

**The final review of `daa46cdf`, in three parts** (three fresh read-only reviewers,
same model as the lead):

| Part | Verdict | What must precede the merge |
| --- | --- | --- |
| A: the two merges with `v1-integration` and what they change for the other team | ACCEPT_WITH_FOLLOW_UPS: no blocker and no major. 1,562 of their files accounted for (18 moved or replaced, each traced); 3,205 of their test functions accounted for (17 renamed, each with its successor; none newly ignored; 104 lost a platform gate); all 102 tests they added since the earlier merge present by name; their three behaviour changes complete; the Keychain attributes and sequences unchanged; the whole workspace lints for Linux, macOS and musl | Nothing in its part. It keeps the record's own condition: a macOS CI result on the pushed commit |
| B: the web process's confinement and its start by the host | CHANGES_REQUIRED: one MAJOR, test only. Three converted web tests give the real Seatbelt confinement a rendezvous path that is not resolved (`/var/folders/…` on macOS is reached through a link), so the confined connect would be denied on a Mac. The filter itself: 143 instructions decoded and found correct; no next hole of the fast-open kind found by probe; Landlock optional found correct | The one-line test fix; then the macOS job |
| C: the repairs of X2's host slice and of X3 | CHANGES_REQUIRED: one MAJOR, test only. A host test added by X2's repairs expects an image refusal on macOS where the macOS adapter starts nothing and answers a plain error. The store's repairs attacked on ext4, Btrfs, loop volumes and in Docker and found sound; the intermittent channels test traced to a race in the other team's own test, fixed by them in a commit this merge brings | The test fix; then the macOS job |

Both majors are tests that would fail only on macOS, found by reading, in code that
has never run on a Mac. Repaired by the lead in the commit after this record:
`a_release_build_without_a_team_admits_only_the_image_the_release_pins` expects what
the macOS adapter does (nothing started, no image refusal); `confined_config` gives
the resolved runtime folder. From the same reviews, also done: the real-host tests'
HQ step has its own bound of 300 s (reviewer B judged the failure in the lead's run
overload, with a thin margin: one miss recovers, the lead's run needed about ten in
a row); Argon2 is optimised in development builds (one of the other team's sign-in
tests missed its 10 s read timeout at load 15 in reviewer A's run); the documents
reviewer A found stale say what is built; design 3.2 and ADR decision 8 state the
store as repaired, with the stacked-filesystem limit reviewer C probed. By the
owner's instruction the two tests that need IPv6 skip that case where the machine
has no IPv6 loopback.

Follow-ups accepted for after the merge: the development key store is stricter than
the other team's old one (a key file with a wrong mode is a storage error, not a
null-key start); a failed `F_FULLFSYNC` no longer falls back to `fsync` in the agent
spool; the worker launch deadline should be a timing the real-process tests can
lengthen (with the other team); small parser and wording nits. Reviewer A's list
"what the other team must be told at the merge" goes to them with the merge.

**Still required before the merge:** the two reviewers who asked for changes confirm
the repairs on the exact commit, and the macOS CI job runs on it (GitHub's queue for
this branch is hours long; nothing after `e8cf07fb` has executed on macOS).

## macOS results from CI

The macOS job (`macos-15`) runs the workspace's tests on every push. Run 44232051595
on `f2ef6ba6` (X1d integrated; X1c and X1d code executed on macOS for the first
time):

- `org-platform` 94 passed (2 ignored: the real launchd and disk-encryption probes),
  `libc_only` 3, `platform_conditionals` 24; `org-platform-host` 83 passed and the
  presence bridge test 1; `org-socket` 59 + 7; `org-supervise` 96 + 49; `org-office`
  567; every `apps/agentic-org` integration suite passed (app_token 7, binary 8, cli 3,
  contract 12, create 21, host_control 14, instances 1, keys 9, peer_uid_source 6,
  service 8, shutdown_signals 4, supervision 18).
- `org-host` 492 passed, 6 failed, 4 ignored. Every test whose name contains
  admission, credentials, keychain, presence, confine, server_check or custody passed
  (59 tests). The 6 failures are `release::store::tests::*` (`PermissionDenied` at
  `release/store/tests.rs:86`), which fail the same way on `v1-integration` and are
  not in files Track X changed.
- Also failing, on `v1-integration` as well: `agentic-org-web`
  `release::tests::held_release_directory_cannot_switch_to_replaced_path`
  (`PermissionDenied`), and `dependency_closure` on both systems (`cargo metadata
  --offline` needs a crate of the other system that the runner's cache lacks).

Run 44285111130 on `6af7ede0` (code as of X2's host slice, `e8cf07fb`; read
2026-11-18 about 12:40 UTC; the seven newer heads had no Rust job result yet, the
runners' queue being hours long):

- macOS: `lnx1` passed (the hard-linked socket works there), `supervision` 19,
  `service` 7, `setup` 8, `contract` 11, `keys` 3, `binary` 6, `app_token` 5,
  `host_control` 14, `instances` 1; `org-host` 607 passed with only the six known
  `release::store` failures; `org-platform` 99, `org-platform-host` 88 and its
  presence test, `org-socket` 59; 68 tests named admission, 50 credentials, 11
  keychain, 18 server_check, 24 custody and 43 `web::tests` passed, none failed.
- macOS, new: `org-socket` `confinement_example`
  `the_example_passes_where_the_web_process_can_be_confined` failed: the example's
  child answered `refused: gateway confinement unavailable`.
- Linux (`ubuntu-26.04`): `lnx1` passed; every `apps/agentic-org` suite passed;
  `org-host` 593 passed. New failures, all from the runner's environment meeting a
  test's assumption, none seen in the sandbox:
  - all three `confinement_example` tests: `the process must hold exactly descriptors
    0, 1 and 2; it holds [0, 1, 2, 142, 145]`. The runner leaves two descriptors open
    in the test process and the test's children inherit them; the entry check refuses,
    correctly. The macOS failure above is very probably the same (the macOS adapter
    gives no reason). The product's own start closes everything above 2. With X4's
    author: the example starts its children with only descriptors 0 to 2, and a test
    holds a stray descriptor on purpose.
  - `admission::linux::tests::a_program_that_root_owns_is_admitted` and
    `web::tests::running_ended_reads_the_refusal…`: `open` failed with ELOOP. On
    Ubuntu 26.04 `/usr/bin/true` and its like are symbolic links into a multi-call
    program, and the admission refuses a link at the pinned path. The first is fixed
    by the lead (`ce7dd5a1`); the web tests move to compiled programs with X4's
    author.
  - `web::tests::the_supervisor_refuses_a_platform_without_a_confinement_adapter`:
    the known one, rewritten at `4823b23a`.
- The job "Platform conditionals list only shrinks" passed.

This is a debug, unsigned build: the Keychain, the publisher check of a signed image,
Seatbelt on a signed web process and the presence dialog are still owed from a Mac
(the lists under X1c and X1d).

## Owner direction, 2026-11-18: Docker without extra configuration; Landlock optional

The owner: the product must work in Docker ("that's no question") and without extra
configuration; a Debian base image is fine for now; and asked why Landlock is needed
at all. Consequences taken by the lead and recorded here until the ADR and the design
are rewritten with the code:

- **Landlock is used when the kernel offers it and is never required** (amends
  ADR-0017 decision 9 and LNX-3). The seccomp filter is the mandatory layer and still
  fails closed. Reason: in every test the filter kills a forbidden call before
  Landlock is asked; the one hole found in review (a fast-open send that connects)
  went through Landlock and was closed in the filter; and requiring Landlock from
  kernel 6.2 excludes common hosts. X4's author is making the change and measuring
  whether any allowed call is stopped by Landlock alone.
- **The container must start with a plain `docker run`** (amends decision 10 and
  design 3.4): the custom seccomp and AppArmor options exist only for the agents'
  vendor sandboxes and become optional; the operator-supplied certificate becomes
  optional, with a certificate the container makes for itself at first start as the
  lead's recommendation, put to the owner (not yet answered).

Measured on this machine the same day (Docker 29.10 daemon started in the sandbox,
image `debian:stable-slim` = Debian 13.7 with glibc 2.41, host kernel 6.18, Docker's
default seccomp profile, no option beyond a bind mount of the build output):

| What ran in the container | User | Result |
| --- | --- | --- |
| The confinement example (`gateway_confinement`, adapter as of `e8cf07fb`) | 61000 | PASS: Landlock and the filter both apply under Docker's default profile |
| The Linux admission adapter's unit tests (26) | 61000 | 26 passed: `memfd_create`, `clone` with a pidfd, `close_range` and `execveat` are all allowed |
| `lnx1` (real `agentic-org serve`, real worker, real `agent-os`) | 1000 | passed |

Not measured: Docker Desktop on a Mac, an older Docker or host kernel, the product's
own image (X5).

## Merge into v1-integration

Owner's go-ahead (2026-11-18): merge what is ready when the independent reviewer
approves it. What must be true of the one commit that is merged:

1. X3's review repairs and X4's last slice are integrated (both in work); X5 is not
   part of this merge unless it is finished and reviewed by then.
2. `origin/v1-integration` is merged into the Track X branch again. It moved 82
   commits past `42dd1322` (to `f7e60098`): a trial merge conflicts in 20 files, among
   them the host's key stores, supervisor, web process and `serve`, which the running
   workers are editing, so this merge is made once, after their branches are in. A
   read-only analysis of Track A's changes in those files is being written as a merge
   guide.
3. The checks pass on that commit: fmt; lint for Linux, macOS (cross-lint) and the
   refusing target; the guard tests; the whole workspace's tests unprivileged; and the
   macOS CI job on the pushed commit shows no failure that `v1-integration` does not
   already have.
4. One fresh independent review of that exact commit covers what no fresh reviewer
   has read: the two merges with `v1-integration` (the lead's conflict resolutions),
   the repairs of X2's host slice and the test-race fix, all of X4's host work and the
   Landlock change, and X3's repairs. Its verdict must be ACCEPT; a CHANGES_REQUIRED
   is repaired and reviewed again.
5. Then `git merge --no-ff` into `v1-integration` and push, and the Track A lead is
   told what changed under them (the platform crates, the rule against platform
   predicates and its CI check, the admission and credential contracts, the root
   refusal, the runner's test user).
