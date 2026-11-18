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
  A2 part 1, which created `crates/org-platform`) and at `831f10ca` (the store
  architecture, A1's SQLCipher removal). The session that runs this lead may push only
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
| X1a check, list, CI guards, layout | Built and accepted 2026-11-18 | See below |
| X1b `fs`, `dirs`, `process`, `socket` | Built and accepted 2026-11-18 (three slices, each reviewed) | See below |
| X1c `admission`, `confinement`, `presence` | Built and accepted 2026-11-18 (two slices, each reviewed); Mac run owed | See below |
| X1d `credentials` | Built and accepted 2026-11-18; Mac run owed | See below |
| X2 first worker on Linux (LNX-1) | The Linux admission adapter is written on `x/x2-linux-admission` (`17c9ae02`), not yet verified by the lead, reviewed or integrated; the host composition is not started | See below |
| X3 host secrets on Linux (LNX-4) | Not started | |
| X4 web process confined on Linux (LNX-3) | Brief written; no code yet | See below |
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
   another CDHash is refused ("selected release kernel image mismatch"); a debug
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
host's own image, the Linux release rule, the root refusal, the script-worker tests
on the by-path test admission, the host's tests on Linux, the LNX-1 test with
`agent-os`).

**State.** The adapter slice is on branch `x/x2-linux-admission`, three commits on
`61acd21a` ending at `17c9ae02` (the contract's surface for an adapter that executes
verified bytes; the adapter; a test fix). Its worker was stopped by a usage limit
during its final verification and returned no receipt. Before it is integrated: the
lead runs its checks (the adapter's tests 20 times, the host's failing set against
the baseline, the three lint targets), reads its diff, and a fresh reviewer reads
it. Two small code repairs from the X1c admission review are held until then,
because they are in the same files (the pin-after-admit composition as a pure
function tested on every system; the error class of an unreachable guard), with two
nits (`Peer::satisfies` documentation; the presence adapter reading the peer token
through the contract's `peer` instead of `pub(crate) mod macos`).

## X4 · The web process confined on Linux (not started)

The brief for the Linux confinement adapter (Landlock and a seccomp allowlist in
`crates/org-platform/src/confinement/linux.rs`, raw system calls through `libc`
only, proved with the confinement example process) is written; its worker was
stopped by the same limit before writing code. Decision already taken for it: the
ready line carries the Landlock status only, so on Linux the host observes seccomp
on the process (`/proc/<pid>/status`) in `verify_web`, and `verify_ready_report`
requires a Landlock ABI of 3 or higher.
