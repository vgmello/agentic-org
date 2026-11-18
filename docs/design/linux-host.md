# Linux host design

How Agentic Org runs on Linux, including as the main process of a container, under
[ADR-0017](../adr/0017-linux-and-container-hosting.md). It names the platform
contracts, the one crate that implements them for every operating system, the Linux
adapter of each, what each proves, and the order of work. The macOS adapters are the
code that exists today; their requirements stay in
[ADR-0008](../adr/0008-gateway-macos-sandbox.md),
[ADR-0011](../adr/0011-protected-recovery-delivery.md) and the
[host boundary design](office-host-boundary.md).

## 1. The shared platform crate

`crates/org-platform` holds every piece of code that differs by operating system, for
all four executables. After Track X, no other Rust file has a `cfg`, `cfg_attr` or
`cfg!` predicate on `target_os`, `target_vendor`, `target_env` or `target_family`.

- **Contents.** Ten modules, and every operating-system difference in the workspace
  belongs to one of them: the five contracts of ADR-0017 (`admission`, `credentials`,
  `confinement`, `service`, `isolation`; section 3) and five further capabilities
  (`fs`, `dirs`, `process`, `socket`, `presence`; section 4). A difference found later
  becomes a new module under the
  [adapter rule](../architecture-rules.md#planned-variation-goes-behind-an-adapter-from-day-one),
  with its macOS, Linux and refusing adapters, and is added to section 2. Each module
  is `<capability>/mod.rs` with `macos.rs`, `linux.rs` and `refused.rs` beside it
  (declared by explicit `cfg` lines at the top of `mod.rs`, which bind `native` to the
  build target's adapter, so that `cargo fmt` sees every file; code only the two real
  adapters share sits in `shared.rs`), and exports one non-conditional
  constructor that returns the build target's implementation or the refusing one. The
  refusing adapter returns `Refused::Unsupported { capability, os }` and never falls
  back to another system's behaviour. It is compiled and unit-tested on every target,
  and CI also builds and tests the crate for Linux with musl, a target that gets only
  the refusing adapters.
- **Targets.** A non-Unix target is a `compile_error!` in `org-platform`. Every other
  Unix target builds and gets the refusing adapters, including Linux with a C library
  other than glibc: the Linux adapters need glibc 2.34 or later (for example
  `posix_spawn_file_actions_addclosefrom_np`). `org-supervise`'s `compile_error!` for
  anything but macOS and GNU/Linux goes when its system code moves here. The container
  image therefore uses a glibc base (Debian), not musl.
- **Contracts live in `org-platform`.** A contract here is an operating-system
  capability (start this verified image, keep these bytes secret, confine this
  process); four executables and six libraries consume it, so no single consumer can
  own it. This is the exception, recorded in ADR-0017 decision 6, to the rule that the
  consumer owns its contract. Policy stays with the consumer: which image is trusted,
  which secret names exist, when a check is skipped in a development build. The crate
  takes that policy as plain data (paths, digests, signing identifiers) and decides
  nothing about the domain.
- **Two crates.** `org-platform` is linked by all four executables and depends on
  `libc` only and on no workspace crate. `org-socket`, `org-supervise`, `org-client`,
  `org-office`, `org-spool`, `org-host` and the applications depend on it; there is
  no cycle. `crates/org-platform-host` holds the capabilities that only the host
  consumes and whose macOS adapter needs an Apple framework (`admission`,
  `credentials`, `presence`, and `names`, the comparison of file names): its macOS
  dependencies are `security-framework`, `security-framework-sys`, `core-foundation`
  and the `cc` build of the recovery presence bridge, all moved from `org-host`. It
  depends on `org-platform`, and only `org-host` depends on it. The reason is the
  link line: a crate's link directives reach every executable that links it, and the
  web process must load nothing but the C library before it confines itself
  (ADR-0008); `crates/org-platform/tests/libc_only.rs` holds both rules. Wherever
  this document says `org-platform` for `admission`, `credentials` or `presence`,
  the code is in `org-platform-host`; the running image's identity and the release
  platform, which the web process also needs and which are libc-only, are
  `org_platform::image`. The LNX-5 check exempts both crates. `org-proto` does not depend on it:
  the macOS values in `org-proto/src/location.rs` move into `org_platform::dirs`, and
  `org-proto` keeps only path composition that takes the base and the limit as
  arguments.
- **What moves in, and what stays.** `org-socket` keeps its portable socket code (the
  ticked reader, descriptor-passing framing); its `confinement`, `image` and the
  platform parts of `peer` and `rights` move into `org-platform`. `org-supervise`
  keeps process ownership policy; `src/platform.rs`, `src/recovery/native/` and the
  conditionals in `lib.rs`, `spawn.rs`, `child.rs`, `recovery.rs` and
  `recovery/scan.rs` move into `org_platform::process`. `org-host` keeps the
  admission and custody policy; its Security-framework, Keychain, audit-token and SDK
  code moves.
- **Tests.** A test of an adapter lives in `org-platform`. A consumer test that
  depends on operating-system behaviour it does not control (an errno, a buffer-full
  error, a linker section name) reads the expected value from
  `org_platform::testing` (a `test-support` feature: for example
  `EAFNOSUPPORT_ON_UNIX_BIND`, `SEND_BUFFER_FULL`, `INIT_ARRAY_SECTION`), or uses a
  portable `libc` constant, instead of a `cfg`. Consumer tests otherwise run on both
  systems against the real adapter or a test adapter built from the contract.
- **The check (LNX-5).** `crates/org-platform/tests/platform_conditionals.rs` reads
  every `.rs` file outside `crates/org-platform/`, requires that `syn` parses it,
  and counts in its tokens each `cfg` or `cfg_attr` attribute and each `cfg!` call that
  names `target_os`, `target_vendor`, `target_env` or `target_family`, wherever it is
  written (macro calls and definitions, attribute arguments, `extern` blocks, raw
  identifiers), and each other use of one of those four names as an identifier, such
  as a key of a `cfg_select!` arm: the names are reserved for `org-platform`. It is a test of the workspace, so `cargo test --workspace` runs it
  on every system, and CI also runs it as its own step on macOS and Linux. String
  literals and fixtures (for example the TOML and `syn` fixtures in
  `org-office/src/store/tests/opener_policy.rs`) are not predicates and do not
  match. A conditional nested in another is counted by itself. It fails on a file
  that does not parse; on `include!` or a `path` attribute of anything but a literal
  `.rs` path, and on `include` imported under another name, because it could not read
  that source; and on a `.rs` file that git tracks in a directory it does not read
  (hidden directories, `node_modules`, and build output, which is a directory holding
  `CACHEDIR.TAG`).
  Its allowlist, `.github/platform-conditionals.allow`, was written in X1a from the
  check's own findings: 79 files with 495 predicates, one `path count` line each (a
  text search finds 81 files, two of them only through fixtures). The check fails on
  a predicate in a file that is not listed, on a listed file with more predicates
  than its count, and on a listed file with fewer than its count or none, so the
  list always equals the sources. A second CI job
  (`.github/scripts/platform_allowlist_shrinks.py`) compares the list with the commit
  where the branch left `v1-integration` and fails on a raised count or a raised
  total, so the list only shrinks (a renamed or split file may appear under new paths
  while the total does not rise). X1b to X1d remove the source files, X2 and the `testing`
  module remove the test files, and the list is empty at X6.
  Outside the check: `cfg(unix)`, harmless because a non-Unix target cannot build
  `org-platform` at all, so such a branch never selects between supported systems;
  and what this scan does not see, which review covers: a `--cfg` flag, a comparison
  with `std::env::consts::OS` at run time, code in a documentation test, a Cargo
  target whose `path` is not a `.rs` file, a `[target.'cfg(..)']` table in a manifest,
  `cfg(windows)` and `cfg(target_feature = "crt-static")`, an `include!` or `path`
  attribute that a macro assembles from its arguments, and a build script that emits
  its own `cfg` after reading `CARGO_CFG_TARGET_OS`. Only `org-platform-host/build.rs` reads that variable, to
  compile the presence bridge (moved there from `org-host` in X1c).

Rejected: renaming `org-socket` to `org-platform`, because the rename ripples through
files the macOS track is editing (`apps/agentic-org-office/src/serve.rs`, the host
supervisor); and one contract per consumer crate, because the same capabilities
would be declared four times.

## 2. What each executable needs from the platform

| Executable | Platform concern | Where it is today | Behind |
| --- | --- | --- | --- |
| `agentic-org` (`serve`) | Start an office worker and admit it | `org-host/src/spawn.rs`, `server_check.rs`, `code_signing.rs`, `peer.rs`, `supervisor/child.rs`, `supervisor/channels.rs` (`reader_is_child`), `supervisor/mod.rs` (macOS-only tests and scripted checks), `lib.rs` (`startup_server_check`) | `admission` |
| `agentic-org` (`serve`) | Start the web process and verify its confinement | Wired by F4 in `org-host/src/web.rs`; the host side is `org_platform::confinement` (`verify_web`) since X1c | `admission`, `confinement` |
| `agentic-org` (`serve`) | Admit native clients on `host.sock`, including the recovery peer | `org-host/src/app_auth.rs`, `human.rs` (`verify_host`), `recovery.rs` (`native()` admits only on macOS), `peer.rs` | `admission` (peer) |
| `agentic-org` (`serve`) | Office secrets keys and host secrets | `org-host/src/keys/` (`mod.rs`, `keychain.rs`, `dev.rs`), `host_secrets.rs`, `host_secrets/` (`keychain.rs`, `dev.rs`) | `credentials` |
| `agentic-org` (`serve`, `setup`) | Register, start and stop the service; stop on a signal | Not built (A2); `org-host/src/signals.rs` is portable | `service` |
| `agentic-org` (`serve`, `setup`) | Release images: which platform's release, static check and the host's own image | `org-host/src/release.rs` (`cfg!` picks the release platform), `release/platform.rs`, `release/runtime.rs` (`check_self`), `org-socket/src/image.rs` | `admission` |
| `agentic-org` (`setup`, `serve`) | Instance, temp and socket locations; filesystem checks | `org-host/src/home.rs`, `instance/directory.rs`, `instance/mod.rs`, `instance/temp.rs`, `instance/setup.rs`, `instance/registry.rs`, `human.rs` (`paths`, `user_temp`); `org-proto/src/location.rs` (`Library/Application Support/agentic.org`, `MAX_SOCKET_PATH_BYTES = 103`, no conditional) | `dirs`, `fs` |
| `agentic-org` (`serve`) | Durable flush and exclusive rename | `org-host/src/writer.rs`, `hostdb.rs`, `release/files.rs`, `release/files/output.rs`, `human/recovery.rs`, `host_secrets/dev.rs` | `fs` |
| `agentic-org` (`serve`) | Office catalog tests gated by system | `org-host/src/offices.rs` | `testing` support |
| `agentic-org` (`recovery`) | Local presence and confirmation for the recovery code (ADR-0011) | `org-host/src/recovery/presence.rs`, the bridge in `org-host/build.rs` | `presence` |
| `agentic-org` (`admin reset`) | Local authority to reset the administrator: the OS dialog at the Mac, root on Linux (SIGN-33) | Not built (A3) | `presence` |
| `agentic-org-office` | Socket locations | `org-proto/src/location.rs` through `apps/agentic-org-office/src/lib.rs` | `dirs` |
| `agentic-org-office` | Its end of the rendezvous: receive descriptors | `org-socket/src/rights.rs`, `rights/send.rs`, `rights/stream.rs` | `socket` |
| `agentic-org-office` | Admit same-user peers on its local socket | `org-socket/src/peer.rs` (`peer_allowed`), called from `apps/agentic-org-office/src/serve.rs` | `socket` |
| `agentic-org-office` | Start, watch, stop and recover agent processes | `org-supervise/src/lib.rs`, `platform.rs`, `spawn.rs`, `child.rs`, `recovery.rs`, `recovery/scan.rs`, `recovery/native/` | `process`, `isolation` |
| `agentic-org-office` | Durable flush of accepted work | `org-office/src/store/acceptance/durable.rs` | `fs` |
| `agentic-org-web` | Confine itself before input | `org-platform/src/confinement/` since X1c (was `org-socket/src/confinement.rs`, `confinement/macos.rs`) | `confinement` |
| `agentic-org-web` | Which platform's release it is, when it authenticates its own image | `apps/agentic-org-web/src/release.rs` (`policy`, a `cfg!` on the system) | `admission` (X1c, with `org-host/src/release.rs`) |
| `agentic-org-web` | Check the inherited listener and received descriptors | `org-socket/src/rights/stream.rs` (`listening`), `rights.rs` | `socket` |
| `agent-os` | Find and connect to the office socket | `org-proto/src/location.rs` through `apps/agent-os/src/office.rs` and `org-client`; the ticked reader is portable | `dirs` |
| `agent-os` | Durable queue writes and safe reads | `org-client/src/agent.rs` (`rename_new`, `full_sync`), `org-spool/src/safe_read.rs` (deleted with B6) | `fs` |

Portable already, and staying where they are: core-dump limits in each `main.rs`,
signal handlers in `apps/agentic-org-office/src/signals.rs`, `apps/agent-os/src/main.rs`
and `org-host/src/signals.rs`, and the descriptor swap in
`apps/agentic-org-office/src/fdswap.rs`. One executable's own source has a platform
conditional outside its tests: `apps/agentic-org-web/src/release.rs`, in the table
above.

Count when the check was written (X1a): 79 `.rs` files outside `org-platform` have a
platform predicate. 45 are source files: 29 in `org-host`, 6 in `org-socket`, 6 in
`org-supervise`, and `org-office/src/store/acceptance/durable.rs`,
`org-client/src/agent.rs`, `org-spool/src/safe_read.rs` and
`apps/agentic-org-web/src/release.rs`; each is in a row above. One is an example
(`org-socket/examples/gateway_confinement.rs`; it stayed in `org-socket` in X1c, because it needs that crate's descriptor passing, and lost its predicates),
and 33 are test files. The list is at 78 files and 491 predicates since A1 removed
SQLCipher from the office store. Milestone-one work in these files adds no new
predicate; a new need goes into `org-platform`.

## 3. The five contracts

Linux hosts need kernel 6.2 or later with Landlock enabled, and glibc 2.34 or later.
Section 3.3 sets the kernel floor; admission alone needs 5.9 (`close_range`).

The service account is an ordinary user on every system: `agentic-org serve`
refuses to run as root. That refusal is a check in the `serve` composition
(`Config::refuse_root`, which `agentic-org serve` calls before anything is opened or
made; built in X2), not in an adapter, so tests can run the adapters as any user.

### 3.1 Process admission

**Contract** (`org_platform::admission`):

```rust
pub struct ExpectedImage {          // policy, supplied by org-host
    pub program: PathBuf,           // the pinned path
    pub sha256: Option<[u8; 32]>,   // from the verified release manifest
    pub macos_cdhash: Option<[u8; 20]>,
    pub macos_requirement: Option<String>, // Developer ID requirement text
    pub development: bool,          // a debug build's own build output
}
pub fn admission(expected: ExpectedImage) -> Result<Admission, Refused>;
impl Admission {
    pub fn spawn(&self, args: &[&OsStr], env: &ChildEnv) -> io::Result<Started>;
    pub fn verify_reader(&self, child: &Child, stream: &UnixStream) -> Result<(), Refused>;
}
pub struct ChildEnv(/* name/value pairs; refuses loader variables, see below */);
pub struct Started { pub child: Child, pub stdin: File, pub stdout: File, pub stderr: File }
impl Child { fn id(&self) -> u32; fn try_wait(&mut self) -> io::Result<bool>; fn kill(&mut self); } // Drop kills and reaps
pub fn peer(stream: &UnixStream) -> io::Result<Peer>;   // native clients on host.sock
pub fn own_image(expected: &ExpectedImage) -> Result<(), Refused>; // the host's own image
```

Built in X1c in `org_platform_host::admission` (the contract's documentation there is
the full text). It is the shape above with what today's macOS order needs:
`spawn(args, env, fence)` takes the host's spawn lock, so that it covers only pipe and
process creation and never the image check; `start` returns a `Held` child and
`Held::release` resumes it, for the web path, which re-checks before resuming;
`recheck` and `readmit` are the two later checks (`ServerCheck::still`, and the web
path's re-admission that must equal the first); refusals are typed (`ImageRefusal`,
`SpawnRefused`, `NotTheChild`), so the host keeps its log texts and its error
classes; `ExpectedImage::macos_signing` (a `SigningRule`: required flags, rejected
and absent entitlements) carries the rest of the host's signing policy as data; and
`native_peers`, `static_images`, `own_signing` and, in a debug build,
`peer_executable` replace the remaining macOS-only calls of the host. `verify_reader`
and `own_image` return errors the host can tell apart, not only `Refused`.

`spawn` returns only a child that has passed the platform's check; `verify_reader`
is called on the accepted rendezvous before and after `ReadyForSecrets`, exactly where
`reader_is_child` and `ServerCheck::still` are called today. The same `Admission`
type starts the office worker and the web process, each with its own
`ExpectedImage`.

Built in X2 on the host's side. The office worker's admission is built once per host
lifetime, on its own thread when the supervisor is made, so that a host start does
not wait for the hash of the image; a start that comes first waits for it, a failed
build is logged ("no office server can be started") and asked again by the next
start, and `host.sock` serves meanwhile. `ServerCheck::Pinned` carries both identities
of the image from the same verified manifest (`Pin { sha256, macos_cdhash }`), and
each system's adapter uses its own. A development host logs the digest of the image it
admitted, once. A check that may not start a server, an image the platform refuses
when the admission is built, and a refused loader variable are refusals
(`SpawnError::Untrusted`) with the check's wording, as a refused child is. The web
process's admission is still built at each start (X4's host slice).

`ChildEnv` refuses, on every system, a variable that changes how the loader runs the
image: any name beginning `LD_` or `DYLD_`, and `GLIBC_TUNABLES`. Today's host passes
only an allowlist (`PASSED_ENV` in `supervisor/child.rs`); the contract makes the rule
hold for every caller. A probe showed why it matters: an execute-only image is
non-dumpable, yet `LD_PRELOAD` still ran injected code inside it.

**macOS adapter.** Today's code, unchanged in behaviour: `spawn_suspended`, the audit
token from `task_name_for_pid`, the Developer ID requirement and hardened-runtime
check, the pinned CDHash, `SIGCONT`, and the token comparison in `verify_reader`. The
signing identifiers (`OFFICE_SIGNING_ID`, `com.vgmello.agentic-org.office`, for the
worker; `WEB_SIGNING_ID`, `com.vgmello.agentic-org.web`, for the web process;
`HOST_SIGNING_ID` for the host) and the team ID are this adapter's data, passed in
`macos_requirement`; they keep their names and values. The Linux adapter has no
equivalent identifier: it uses the release manifest digest. `org-host/src/spawn.rs`
and the Security-framework half of `code_signing.rs` move; `ServerCheck` in
`server_check.rs` becomes the policy that builds `ExpectedImage` (today: a debug build
skips the check, a release build without `ORG_APP_TEAM_ID` refuses, a release with a
runtime pin adds the manifest CDHash; `org-host/src/lib.rs`, `startup_server_check`).

**Linux adapter.** The worker runs from bytes the host verified, not from a path.

1. When the host builds the `Admission`, once per host lifetime, the adapter first
   makes the process non-dumpable (`PR_SET_DUMPABLE` 0, and refuses if that does not
   hold), so that no other process of the user can open the host's descriptors through
   `/proc/<pid>/fd`. It opens the pinned path with
   `O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK` (a FIFO at the path must not block
   the host), requires a regular file owned by the service user or root and not
   writable by group or others, and copies it into
   `memfd_create(name, MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_EXEC)`. On `EINVAL` (a
   kernel before 6.3, which has no `MFD_EXEC` and makes every memory file executable)
   it retries once without `MFD_EXEC`. It seals the copy with
   `F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL`, sets its mode to `0111`
   with `fchmod`, and then computes SHA-256 by reading the sealed object back through
   the descriptor it holds. That digest must equal the digest of the bytes it read
   from the pinned path, and it is the one judged: nothing written before the seals
   can differ from what was judged.
2. A release build compares the digest with the manifest entry
   (`FileEntry::sha256`, already signed and verified by `org-release`; the digest
   covers every byte, so the size is not compared separately). A development
   build records the digest of its own build output at that moment and logs that the
   image has no publisher check, as macOS logs today for an unsigned build. A release
   build without a runtime release pin refuses: on Linux there is no other publisher
   identity.
3. `spawn` makes the process non-dumpable again (the child is a copy of the host's
   memory until it executes the image), blocks signals, and uses the raw `clone`
   system call with `CLONE_PIDFD | SIGCHLD` and no shared address space (`clone3` is
   not needed; the argument order of `clone` differs by architecture, so x86-64 and
   aarch64 are built and every other architecture refuses). The child runs only raw
   system calls: reset `SIGPIPE`, `dup3` the three pipes to 0 to 2, `chdir("/")`,
   duplicate the sealed image to descriptor 3 and a report pipe to descriptor 4, both
   close-on-exec, `close_range(5, ~0)`, unblock signals, then
   `execveat(3, "", argv, envp, AT_EMPTY_PATH)`. A step that fails writes the step and
   the error to the report pipe and the child exits with 127; `spawn` returns only
   when the pipe has closed without a report, which is the `exec`, so a refused start
   names the call that failed. The kernel opens the image before it closes
   descriptors 3 and 4, so the worker starts with exactly descriptors 0 to 2. An interpreter script (`#!`) cannot start this way: the
   kernel fails with `ENOENT`, because the interpreter would open the descriptor
   after it is closed. Only compiled executables are admitted.
4. `Child` holds the pidfd. Exit is observed by polling it, signals use
   `pidfd_send_signal`, and the child is never reaped elsewhere, so its PID is not
   reused while the host holds it.
5. `verify_reader` reads `SO_PEERCRED` on the accepted rendezvous: the PID must equal
   the child's and the UID the host's, the pidfd must not be readable (a zombie
   answers `pidfd_send_signal(pidfd, 0)` with success, so the signal alone would not
   notice an exited child), and `pidfd_send_signal(pidfd, 0)` must succeed.
   `SO_PEERCRED` names who connected and does not change afterwards, unlike the macOS
   audit token: the check proves that the child connected and still lives. The worker
   therefore never passes its rendezvous connection to a process it starts, the same
   rule as never calling `exec` on itself.

There is no start-suspended step, and none is needed. macOS checks the image after
`exec`, so it must stop the child until the check is done. Linux checks the bytes
before `exec` and executes exactly those bytes from a sealed descriptor that nobody
can change. Because the image is not readable (`0111`), the kernel starts a child of
an unprivileged user non-dumpable: from its first instruction no same-user process
can `ptrace` it, read or write `/proc/<pid>/mem`, or follow `/proc/<pid>/exe`. (Root
can; for a root-owned process the property does not hold, which is one reason the
service refuses root.) The worker never calls `exec` on itself, so the PID that
connects is still the verified image. The host also sets `PR_SET_DUMPABLE` to 0 at
startup, before it reads a key, so its own memory is equally closed; the admission
adapter does not rely on that call and sets it itself before it makes the memory file
and before every start. `/proc/<pid>/cmdline` and
`/proc/<pid>/status` stay readable to the same user, so no secret is ever in an
argument, as the launch contract already requires.

`peer` on Linux returns the UID and PID from `SO_PEERCRED`. A native client is
admitted on `host.sock` by UID alone (the service user's, or root for `admin reset`
only, section 4): a command started by the person has no code identity the kernel
can vouch for, so its authority is only the session the person approved in the
browser (A5). `own_image` hashes `/proc/self/exe` and compares it with the manifest
entry for `agentic-org`; `release/platform.rs` accepts a Linux executable on the
signed manifest digest alone, after its ELF header check. Built in X2: the host's
peer policy has one path (`app_auth`), in which a peer the platform names by its user
alone is admitted when that user is the host's own, and a command accepts a host of
its own user the same way (`human::verify_host`); the platform says which kind of
system it is with each identity (`Peer::uid`). Root for `admin reset` is not built:
the listener refuses every other user when it accepts (X5, with the container's
`docker exec -u 0` path).

The worker's `current_exe()` is `/memfd:… (deleted)`, so a worker cannot find sibling
executables (for example `agent-os`) next to itself. The host passes the release's
program paths to the worker (a note for C3).

**What it proves.** The worker's and the web process's own executable code is the
exact byte sequence whose SHA-256 the signed release manifest names (in a development
build, the bytes the host read at startup), it was started without loader variables,
and the rendezvous peer is that process. **What it does not prove.** The dynamic
loader and the shared libraries it maps (glibc, and the few system libraries the
executables link) are not hashed; they are trusted because root owns them, in the
system or in the container image. There is no kernel-enforced code signature: the
proof rests on the manifest signature checked when the host starts and on the host
itself being the intended binary. A same-user attacker who replaces the
`agentic-org` executable before the host next starts runs a host that skips every
check; macOS binds the Keychain to the signed host and Linux has nothing equivalent
(section 5). A release installed under a root-owned path, as in the container image,
closes that. A native client's image is not identified.

**Failing closed.** Any failed step refuses the spawn, kills and reaps a started
child, and counts as today's `storage_error`; nothing is sent. A refused loader
variable, `vm.memfd_noexec = 2`, `EINVAL` on both `memfd_create` attempts, or a
container profile that refuses `memfd_create`, `clone` with `CLONE_PIDFD`,
`close_range` or `execveat` refuses the start and names the call and the error.

**Tests.** The host's supervisor tests start shell-script workers today
(`script()` in `org-host/src/supervisor/mod.rs`, ten uses; `wrapper()` in
`apps/agentic-org/tests/support/mod.rs`), and a script cannot be admitted. X2 added
`org_platform_host::testing::admission::path_admission()`, a test adapter built from
the same contract that starts a program by path and checks the reader by PID,
available only under the `test-support` feature and never in a production
composition; the host's unit tests that need a script reach it through
`ServerCheck::ByPath`, which exists only in test builds. The integration tests start
only compiled programs: `wrapper()` is gone, the fake office server reads what a
wrapper did from its script, and a compiled recording worker
(`crates/org-host/examples/recording_worker.rs`) stands in where a unit test needs a
real admitted child. The real Linux adapter is tested with a compiled worker fixture. The non-dumpable test needs an unprivileged child: when the
test runs as root (a development container), it drops the child to UID 65534 with
`setresuid` before `spawn`, so it runs, and passes or fails, the same everywhere.

Rejected: executing the verified file by path or by an `O_PATH` descriptor, because
the file can change between hashing and loading; fs-verity, because it depends on the
filesystem and the release store's volume.

### 3.2 Credential provider

**Contract** (`org_platform::credentials`): one item store per instance.

```rust
pub struct CredentialLocation<'a> {
    pub instance_id: &'a str,
    pub keychain_group: Option<&'a str>, // macOS release
    pub directory: Option<&'a Path>,     // Linux, and development builds everywhere
    pub data_root: &'a File,             // held descriptor; must stay disjoint from the store
    pub expect_items: bool,              // the host's records say keys exist
}
pub fn credentials(location: CredentialLocation) -> Result<Credentials, Refused>;
impl Credentials {
    pub fn add(&self, name: &ItemName, value: &[u8]) -> Result<(), CredentialError>; // add-only
    pub fn replace(&self, name: &ItemName, value: &[u8]) -> Result<(), CredentialError>;
    pub fn load(&self, name: &ItemName) -> Result<Option<Secret>, CredentialError>;
    pub fn delete(&self, name: &ItemName) -> Result<(), CredentialError>;
    pub fn list(&self) -> Result<Vec<ItemName>, CredentialError>;
}
```

Built in X1d in `org_platform_host::credentials`, with the location as an enum
(`CredentialLocation::Keychain { access_group, service }` or `Directory { anchor,
below, create, sharing, suffix }`), because exactly one store is chosen and the
Keychain service and the directory's layout are host policy passed as data.
`data_root` and `expect_items` arrive with X3's release store. `add` and `replace`
report whether a temporary file was left; `list` takes the consumer's name rule and
never opens a file that rule refuses. The directory engine's lock file is `.lock`; an
add publishes by exclusive rename; the office-keys view holds a shared lock and opens
at first use, the host-secrets view an exclusive one; the two development layouts on
disk are the ones the two old development stores had.

`org-host` keeps its two typed views, `keys::KeyStore` (office secrets keys) and
`host_secrets::HostSecrets` (writer custody, TLS, pending recovery code), with their
name rules and add-only rules, as wrappers over one `Credentials`. Errors carry kinds
only, never bytes or paths.

**macOS adapter.** Today's Data Protection Keychain items, in the compiled access
group, after the host's own entitlement and hardened-runtime check
(`keys/keychain.rs`, `host_secrets/keychain.rs`).

**Linux adapter, and every development build.** An owner-only directory outside the
data root. One engine replaces today's two development stores (`keys/dev.rs`,
`host_secrets/dev.rs`), which already use `openat`, `O_NOFOLLOW`, owner and mode
checks and a lock.

- **Where.** On a Linux host,
  `${XDG_STATE_HOME:-~/.local/state}/agentic-org/credentials/<instance-id>/`, which
  setup creates with mode `0700` together with its `agentic-org/credentials` parents.
  In a container, a mounted secrets volume (`/secrets` in the container file). Setup
  records the absolute path in the setup record (the `credentials` property of
  non-interactive setup); the host never derives it from its environment at start.
- **Checks when the store opens.**
  - The path is absolute with no `..`, and is walked from `/` with
    `openat(O_DIRECTORY | O_NOFOLLOW)` one component at a time.
  - Every ancestor is owned by root or the service user. It is not writable by others.
    It is not writable by its group, with two exceptions: a root-owned sticky
    directory, and a directory owned by the service user whose group is the user's
    own private group (the user's primary group, named as the user, with no other
    members), as on a default Ubuntu account whose umask is `0002`.
  - The store directory is owned by the service user with mode exactly `0700`.
  - Its filesystem (`fstatfs`) is one of ext4, XFS, Btrfs or F2FS. Network and user-
    space filesystems (NFS, CIFS, sshfs, Docker Desktop's virtiofs and gRPC FUSE) and
    memory filesystems are refused: a lost or shared office secrets key loses or
    exposes that office's private keys. Docker and Kubernetes secrets are read-only
    `tmpfs` mounts and are not a store.
  - Store and data root are compared by the `(device, inode)` of held directory
    descriptors, walking `..` from each up to `/`: neither may contain the other.
  - In a container (setup's `service` is `foreground`) the store directory must be a
    mount point (`statx` `STATX_ATTR_MOUNT_ROOT`).
  - When `expect_items` is set and the store holds no item, the store refuses with
    "credentials missing", and the host does not start: it never creates fresh keys
    for offices that already have wrapped records.
  - An exclusive `flock` on `lock` admits one host per instance.
- **Checks on every item.** A regular file opened with `openat` and `O_NOFOLLOW`,
  owned by the service user, mode exactly `0600`, one link, at most 16 KiB. A write
  goes to a new temporary file, `fsync`, then `renameat2` with `RENAME_NOREPLACE`
  for an add or a plain rename for a replace, then `fsync` of the directory.
- **Failing closed.** A wrong owner, mode, filesystem, location, mount, empty store or
  lock refuses the host's start with the exact path and rule broken. Nothing repairs
  permissions.
- **In a container.** The container file declares `/data` and `/secrets` owned by the
  service user with mode `0700`. The operator uses two named volumes; the
  documentation says never to rely on anonymous volumes (a replaced container gets
  new, empty ones, and the empty-store check then refuses rather than silently losing
  the keys) and never to bind-mount a macOS folder (refused by the filesystem rule).
  Without a mounted secrets volume, setup refuses.

**Built in X3** (`org_platform_host::credentials`: `protected_store()` answers
`Keychain` or `ReleaseDirectory`; `release_directory(ReleaseDirectory { path,
data_root, mount_point_required, expect_items })` returns a `ReleaseRoot` whose
`store(ReleaseItems { directory, suffix, sharing })` opens an item store below the
checked root with the X1d engine; `org-host/src/release_store.rs` is the host's use of
it). As built, where the text above is not exact:

- The checks run in this order and the first failure refuses with the rule and the
  offending directory (`CredentialError::Root(RootRefusal { rule, path })`, the one
  error of this capability that carries a path: a directory path is configuration,
  not a secret): path syntax; the walk, ancestors opened with `O_PATH`, each judged
  before anything below it is opened; the store's owner and mode; the filesystem
  (ext2 and ext3 share ext4's number and pass); disjointness from the data root; the
  mount point when asked; the empty store; then the lock, which is the first thing
  made.
- Disjointness is decided twice (X3's review, S1). First by the walk up from each
  directory by device and inode. Then each directory is placed in its filesystem from
  the mount table: its mount by the mount id the kernel gives for the held descriptor
  (`statx`), that mount's root from `/proc/self/mountinfo`, and the path below the
  mount point; equal or nested places refuse. This catches a store that is a bind
  mount of a directory inside the data root, which the walk cannot see because it
  leaves a mount through its mount point. Of the table only the two mounts looked for
  are read in full; a machine that runs Docker has mounts whose root is not a path,
  and they refuse nothing. Any doubt refuses: no `/proc`, a kernel that reports no
  mount id (before 5.8), a table that cannot be read, a deleted mount root.
- With a required mount point (the foreground mode) the store is also compared with
  the whole of what the data root's mount shows, so a secrets volume that is a
  directory of the data volume is refused even when the instance root is a folder
  inside that volume (`InsideDataVolume`). On Btrfs the filesystem is the mount
  table's device, so subvolumes are placed by their path from the top.
- **Not seen:** a stacked filesystem. A data root on an overlay and a store in a
  directory of that overlay's upper layer are two filesystems in the mount table
  (probed in the final review: such a store opens, and is visible inside the data
  root through the overlay). It needs a data root on an overlay whose upper layer
  the service user can name; inside a container the upper layer is not reachable. A
  backup of such a data root holds the keys. Do not put the data root on an overlay.
- A directory admitted by a group-write exception (the private group, or a root-owned
  sticky directory) must carry no access ACL, and neither may an image file admitted
  that way: an ACL entry for another user shows in the mode only as group bits.
- The host declares its item stores (`office-keys`, `host-secrets`). A declared store
  that is not the service user's directory with mode `0700` refuses by its own rule,
  naming it, before the store is called empty; only what the engine would hand the
  host counts as an item; when the records say keys exist, a file named like an item
  that is not one refuses the store (`ItemUnsafe`), and otherwise the engine refuses
  that item when it is used; entries the host did not declare are ignored and never
  opened (a root-owned `lost+found` in a volume root is tolerated).
- Whatever an open makes gets its mode set, whatever the process's mask is (the lock
  and item files on their descriptor; an item store's directory by name in its parent,
  which is the checked owner-only directory); nothing that exists is changed.
- The lock file is `.lock`, the engine's name. The layout is `<dir>/.lock`,
  `<dir>/office-keys/<office id>.key` (the raw 32 bytes, as in the Keychain) and
  `<dir>/host-secrets/<name>`.
- The empty-store rule is checked over the whole store before any view exists. The
  host says keys exist when the office home holds an office folder (a folder is made
  only after its key was stored). A store that holds host secrets but no office key
  passes the rule; an office whose key is gone is started without a key (its handoff
  carries none) and never gets a fresh one.
- The private group is the user's primary group whose member list is empty or names
  only the user. An account that has that group as its primary group is not in the
  member list and is not looked for (it cannot be found without enumerating
  accounts). The same rule admits an image file of the admission adapter (section
  3.1) that the service user owns and its group can write, so a development host on
  an account with umask `002` starts its own build.
- The record's `credentials` path is byte-exact normalized and frozen; setup creates
  the directory, opens it once as the host will, and prints `Secrets: <path>`.
- Open: `mount_point_required` is not asked for until the record has a service mode
  (X5); a whole release-profile host on a release directory runs in no test; setup
  itself still stops on Linux until X5's service and disk-encryption adapters.

**What it proves.** Only the service user and root can read the items, none is in
the data volume, a wrong setup refuses, and a lost store is noticed (LNX-4). **What it
does not prove.** Any process running as the service user reads every item; macOS
limits Keychain items to the signed host. Agents in `same-user` mode run as that user,
so their protection from these files is the vendor tool's sandbox (section 5).

Rejected: a Secret Service or kernel keyring, because neither exists in a container or
a headless session and the keyring does not survive a reboot; systemd credentials,
because they are read-only and the host creates keys.

### 3.3 Confinement of the web process

**Contract** (`org_platform::confinement`):

```rust
pub fn confine_web(rendezvous: &Path) -> io::Result<Confined>;   // in the web process
pub struct Confined { pub stream: UnixStream, pub enforcement: Enforcement, pub landlock_not_applied: Option<String> }
pub fn web_confinement_supported() -> io::Result<()>;            // in the host, before any bind, child or secret
pub fn verify_ready_report(enforcement: &Enforcement) -> io::Result<()>; // in the host, when the ready line arrives
pub fn verify_web(pid: u32, enforcement: &Enforcement, critical: &[&Path]) -> io::Result<()>; // in the host, before the handoff
pub fn layers(enforcement: &Enforcement) -> io::Result<Layers>;  // what the process is confined by, for the host's log
```

Built in X1c (macOS) and X4 (Linux). The functions return `io::Result`, because they
fail for reasons other than a missing adapter; a refusal's reason is in the error.
`verify_web` takes the process id: the admission's child type is in the host-only
crate, so the host holds the child (unreaped) across the call and checks afterwards
that it has not exited. The host makes two checks, where one function would reorder
macOS's steps: the report when the ready line arrives, and the process itself after
the reader and image re-checks.

`confine_web` returns the connected rendezvous stream only after confinement is in
force, so the web process cannot use the stream unconfined on any system. The order
inside differs by system (macOS confines, then connects; Linux connects, then
confines); either way nothing reads from the stream or reports ready before
confinement holds. `Enforcement` is the evidence the web process reports in
`ReadyForSecrets`.

**macOS adapter.** ADR-0008: the thread and descriptor preconditions, the Seatbelt
profile, then the connection; the host's `sandbox_check` probes
(`org-platform/src/confinement/macos.rs`, moved from `org-socket` in X1c).

**Linux adapter: the seccomp filter, and Landlock where the kernel has it.** The
filter is the layer that must hold; without it the web process does not start.
Landlock is a second layer over the same ground: it is applied on every kernel that
offers it and is never a reason to refuse (owner direction, 2026-11-18: the product
runs on ordinary Linux hosts and in Docker without extra configuration; requiring
Landlock from Linux 6.2 excluded common hosts). Nothing chooses the layers but the
kernel: there is no setting. `confine_web` is called on the main thread before any
runtime thread. The host starts the web process with an empty environment (so no
`RUST_BACKTRACE`: a backtrace opens files, which the filter kills).

0. `mallopt(M_ARENA_MAX, 1)`. glibc opens `/proc/sys/vm/overcommit_memory` the first
   time a heap of a non-main allocator arena shrinks, which the filter kills: the
   real web process died that way under 224 concurrent requests. With one arena there
   is no such heap. It holds only when the process never had a second thread before
   this call (the contract); otherwise the result is the kill, never an unconfined
   process. The cost is one allocator lock for all threads; per-thread caches remain.
1. Disable core dumps, set `PR_SET_DUMPABLE` to 0, and require exactly one thread
   (`/proc/self/status`, `Threads: 1`) and exactly descriptors 0 to 2
   (`/proc/self/fd`). Static assets are already in memory.
2. Connect to the rendezvous. This is the last new socket the process creates.
3. `PR_SET_NO_NEW_PRIVS`.
4. Landlock, if it can be applied. Query the ABI; from ABI 1 up, create a ruleset that
   handles every right that ABI knows (filesystem rights; from ABI 4 TCP bind and
   connect; from ABI 6 the scopes for abstract Unix sockets and signals), add no rule,
   and restrict the process. If the query, the ruleset or the restriction fails for
   any reason (no Landlock in the kernel, a container profile that refuses the call),
   nothing is restricted by Landlock, the reason is kept, and the next step runs.
5. seccomp, installed for every thread (`SECCOMP_FILTER_FLAG_TSYNC`). The filter kills
   the process on a foreign architecture (including x32 on x86-64) and on every call
   outside a fixed list per architecture (x86-64 and aarch64; any other architecture
   refuses). A failure to install it refuses: the stream is dropped unused and the
   error names the call and the error.
   - Allowed with any arguments: `read`, `write`, `writev`, `close`, `poll` (`ppoll`
     on aarch64), `recvfrom`, `recvmsg`, `shutdown`, `getsockname`, `getpeername`,
     `setsockopt`, `getsockopt`, `accept4`, `epoll_create1`, `epoll_ctl`,
     `epoll_wait` (`epoll_pwait` on aarch64), `eventfd2`, `munmap`, `brk`, `madvise`,
     `futex`, `sched_getaffinity`, `sched_yield`, `set_robust_list`, `rseq`, `gettid`,
     `getpid`, `exit`, `exit_group`, `rt_sigaction`, `rt_sigprocmask`, `rt_sigreturn`,
     `sigaltstack`, `restart_syscall`, `getrandom`.
   - Allowed by argument: `sendto` only without a destination and with flags within
     `MSG_DONTWAIT | MSG_NOSIGNAL`; `clone` only with glibc's exact thread flags;
     `prctl` only `PR_SET_NAME`; `ioctl` only `FIONBIO` and `FIONREAD`; `fcntl` only
     `F_GETFD`, `F_GETFL` and `F_DUPFD_CLOEXEC`; `mmap` and `mprotect` without
     `PROT_EXEC`; `mremap` with no flag or `MREMAP_MAYMOVE`; `tgkill` only to the
     process itself; `clock_gettime` only with a non-negative clock id (the fixed clocks; a
     process's or thread's CPU clock has a negative id).
   - `clone3` answers `ENOSYS`, so glibc falls back to `clone`, whose flags the filter
     can read. `sendmsg` is killed: the web process sends no descriptor.
   - Everything else kills, among them `socket`, `connect`, `bind`, `listen`, every
     `open`, `execve`, `execveat`, `ptrace`, `process_vm_*`, `kill`, `unshare`,
     `setns`, `mount`, `bpf`, `io_uring_setup` and `memfd_create`.
6. Report `Enforcement { landlock_abi, seccomp: true }` and send `ReadyForSecrets`.
   When Landlock was not applied, the web process first writes one line on standard
   error, `agentic-org-web: confinement: landlock not applied: <reason>` (the reason
   is 1 to 240 printable ASCII characters).

Why each entry that is not obvious is there, all measured on Linux 6.18 with glibc
2.39: `getpeername` (the web process checks its control connection), `getpid`
(glibc's `abort`), `restart_syscall` (the kernel, after a stop and continue during a
timed `poll`), `mremap` (glibc grows a large block with it: a buffer pushed past
about 128 KiB killed a healthy process), `sched_yield` (a contended standard channel).
Why the argument rules: a `sendto` with `MSG_FASTOPEN` and a destination CONNECTS a
TCP socket, and did so in a probe under Landlock with TCP connect denied (Landlock
checks the `connect` call only), and a datagram send with a destination reaches any
named Unix socket of the user; `fcntl` with `F_SETOWN`, `F_SETSIG` and `O_ASYNC` makes
the kernel signal any process of the user (a probe killed one; Landlock stops that
only from ABI 6).

On the host: `web_confinement_supported()` asks, without changing anything, whether
this architecture has a list, whether the kernel knows the kill-process action, and
whether a filter can be installed (an install with a null filter must answer
`EFAULT`); Landlock is not asked. `verify_ready_report` accepts a report with or
without Landlock. `verify_web` reads `/proc/<pid>/status` of the admitted child and
requires `NoNewPrivs: 1`, `Seccomp: 2` and more filters than the host itself has
(`Seccomp_filters` counts inherited ones, so under a container's own profile "one
filter" would prove nothing); the host then checks the child is still running. Any
failure kills and reaps the web process before any descriptor or secret is sent. Once
per start the host logs what the web process is confined by: `the seccomp filter and
Landlock ABI n`, or `the seccomp filter alone: Landlock was not applied: <reason>`.

**Minimum kernel.** The kill-process action is Linux 4.14; the admission of section
3.1 needs 5.9. Landlock adds itself from 5.13.

**What it proves.** Once ready, the web process cannot open any file, create or
connect a socket, start a program, or trace or signal another process; it can only
serve the listener and descriptors it holds. The whole list of forbidden operations
is proved under the filter alone as well as under both layers, and no operation the
list allows was found that only Landlock stopped (X4, 21 operations in six
layerings). **What it does not prove.** The host cannot observe another process's
Landlock domain, nor which filter a process installed: it trusts the report of an
image it admitted by digest, and observes that a filter beyond its own, and
no-new-privileges, are in force. The macOS host's path probes have no Linux
equivalent. **What Landlock adds where it is present:** a backstop for a mistake in
the list. With `connect` added to the list as an experiment, the call was refused by
Landlock and succeeded under the filter alone.

A confined process must not make a call the list lacks: it is killed. Known such
calls a web process could be tempted to make: `std::thread::sleep`
(`clock_nanosleep`), `available_parallelism` (opens files), `geteuid`, `localtime`
(opens a file), anything that opens a file. Left open and judged harmless:
`sched_getaffinity` of another process, futex priority-inheritance operations that
name another task, `getsockopt(SO_PEERPIDFD)`.

**Failing closed, and in a container.** A kernel or a container profile that refuses
`seccomp` makes the host refuse to start the web process, or the web process exit
before readiness, with the reason in the host log (the web process writes
`agentic-org-web: startup refused: confinement: <reason>` on standard error, and the
host keeps the first 1024 bytes of a child that ends before its ready line); the host
reports the web service unavailable and keeps native control (LNX-3). A container's
own seccomp profile does not replace this filter. Under Docker's default profile on
Linux 6.18 both layers apply (measured, X4). The reason reaches the host's log, not
yet its status output (the status contract is Track A's).

Rejected: an allowlist that returns an error instead of killing, because a forbidden
call that fails quietly hides a broken assumption; connecting after confinement as
macOS does, because Landlock does not control connecting to a named Unix socket and
denying `connect` outright is simpler; requiring Landlock, because it excluded hosts
without adding a guarantee the filter does not give.

### 3.4 Service lifecycle

**Contract** (`org_platform::service`):

```rust
pub enum ServiceMode { Registered, Foreground }
pub struct Unit { pub instance_id: String, pub program: PathBuf, pub args: Vec<OsString>, pub env: Vec<(OsString, OsString)> }
pub struct Placement { pub prefix: String, pub directory: PathBuf } // Placement::user(home); tests use a temp one
pub fn service(mode: ServiceMode, home: &Path) -> Result<Box<dyn ServiceManager>, Refused>;
pub fn service_in(mode: ServiceMode, placement: Placement) -> Result<Box<dyn ServiceManager>, Refused>;
pub trait ServiceManager {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError>; // Unchanged | Loaded; idempotent, never takes over another unit
    fn start(&self, instance_id: &str) -> Result<(), ServiceError>;
    fn stop(&self, instance_id: &str) -> Result<(), ServiceError>;
    fn status(&self, instance_id: &str) -> Result<ServiceStatus, ServiceError>; // the login and linger requirement is added in X5
    fn unregister(&self, instance_id: &str) -> Result<(), ServiceError>;
}
pub enum Registration { Unchanged, Loaded }
pub enum ServiceStatus { NotRegistered, Unloaded, Stopped, Running { pid: u32 } }
pub enum ServiceError { Refused(Refused), Invalid(&'static str), Conflict(String), System(String) }
```

A2 writes the launchd registration behind this contract. A2 B0 built it as shown
(`crates/org-platform/src/service/`), before X1a: `ServiceStatus` is `NotRegistered`,
`Unloaded`, `Stopped` or `Running { pid }`; every adapter refuses a relative program,
path syntax in the instance ID and loader variables (`DYLD_*`, `LD_*`,
`GLIBC_TUNABLES`) in the unit's environment. `Foreground` refuses everywhere until X5.

`agentic-org serve` is the same foreground process in every mode; it stops on
`SIGTERM` or `SIGINT` (`org-host/src/signals.rs`): it stops accepting, stops the web
process and the workers (`SIGTERM`, then `SIGKILL` after 1.5 s), flushes its logs and
exits 0 within 2.5 s, inside the shortest default grace of a container runtime (Docker
Desktop's 3 s, `default-stop-timeout`; Docker Engine's is 10 s). A worker killed at the
deadline loses nothing: the office store recovers from a kill at any point. Every wait of
the stop ends by one deadline fixed when the signal is taken: a web process start under
way ends at once, and what still runs 0.5 s after the workers' grace (a request or a
create waiting for a lock of the registry, a worker still in its launch handshake) is
logged and left to end with the process.

**macOS adapter.** A per-user launchd agent (written by A2 behind this contract):
`~/Library/LaunchAgents/com.ellosoft.agentic-org.instance.<instance-id>.plist` with
`RunAtLoad`, `KeepAlive { SuccessfulExit = false }`, `Umask` 0077 and an explicit
environment, loaded with `launchctl bootstrap gui/<uid>`. An identical loaded job is
left alone; a different file or a job loaded from elsewhere under the label is a
conflict. "Elsewhere" is decided by the top-level `path` of `launchctl print`, which
must name our definition file; `register`, `start`, `stop`, `status` and `unregister`
all refuse such a job. The definition is published by hard-linking a complete temporary
file, so a file that appears meanwhile is never replaced (a conflict). `start` never
loads a definition: on `Unloaded` it refuses, and the caller runs `register` again,
which loads the file only when its bytes equal the rendered unit. `unregister` waits
up to 15 s for launchd to drop the job after `bootout`. The plist has no
`ProcessType`, so the host is not throttled as a background job. Program, arguments
and environment must be valid XML text (no NUL or other C0 control except tab, line
feed and carriage return). `stop` sends `SIGTERM`, so the clean exit is not restarted.
Disk encryption reads `diskutil info -plist` for the mount point (`statfs`) of the
nearest existing ancestor of the root.

**Linux adapter, `Registered`.** A systemd user unit,
`~/.config/systemd/user/agentic-org-<instance-id>.service`, with
`ExecStart=<pinned agentic-org> serve --instance <id>`, `Type=exec`,
`KillMode=mixed`, `TimeoutStopSec=15`, `Restart=on-failure`, `UMask=0077` and
`NoNewPrivileges=yes`, registered with `systemctl --user daemon-reload` and
`enable --now`, as the launchd agent starts when it is loaded. Without a user manager (no `XDG_RUNTIME_DIR` or user bus) it refuses
and names `Foreground`. When lingering is off, `status` and setup report that the
instance runs only while the user is logged in and print
`loginctl enable-linger <user>`, which polkit normally lets a local, active user run
for themselves; setup does not run it without being asked. The unit file is writable
by the same user, like the executable path it names (section 5).

**`Foreground`, the container mode.** Nothing is registered: the container runtime
is the supervisor. Setup is told the mode explicitly; nothing detects a container.
The container's command is `agentic-org serve --setup /etc/agentic-org/setup.json`:
when that instance is not set up yet, `serve` first runs the same non-interactive,
offline setup (`setup --non-interactive --config`), which repeats as a no-op later,
then serves. The image's entry point is `tini`, so orphaned agent processes are
reaped and the stop signal reaches `serve`.

**Amended by the owner's direction of 2026-11-18, to be rewritten with X5:** the
container starts with a plain `docker run` that publishes the port. The image declares
its volumes; the seccomp and AppArmor options below are optional and only let agents'
vendor sandboxes create user namespaces. The listener paragraph below (HTTPS with an
operator's certificate) is NOT what is built: for now the container serves plain HTTP
through a listener mode of its own that only the foreground mode may choose, and the
run command publishes it on `127.0.0.1` only. HTTPS with a certificate the instance
makes for itself, or one the operator supplies, is pending (implementation plan, X7):
the product serves no HTTPS on any system yet.

**The container file** (`deploy/container/Dockerfile`):

- A Debian (glibc) base; the release installed under root-owned `/opt/agentic-org`;
  a service user with a dedicated high UID and GID, 61000, so that it is unlikely to
  match an account on the Docker host; `USER 61000:61000`.
- Volumes `/data` and `/secrets`, owned by 61000 with mode `0700`.
- `/etc/agentic-org/setup.json`: `service: foreground`, `root: /data`,
  `credentials: /secrets`, and the listener below.
- **The listener.** A published port arrives on the container's network interface,
  not its loopback, and the web process refuses plain HTTP off loopback. So the
  container listens with HTTPS on `0.0.0.0:8443`; the configured Origin is
  `https://localhost:8443`; the operator publishes it as
  `-p 127.0.0.1:8443:8443`. The certificate and key are a pair the operator places
  in the secrets volume (made with a local CA the Mac's browser trusts, for example
  `mkcert localhost`); setup imports them into the credential store. This needs the
  host's verifier for a supplied certificate pair, which X5 builds unless A3 has.
- **The run command**, documented and used by LNX-2:
  `docker run -d --name agentic-org -v agentic-org-data:/data -v agentic-org-secrets:/secrets -p 127.0.0.1:8443:8443 --security-opt seccomp=deploy/container/seccomp.json --security-opt apparmor=agentic-org agentic-org`.
  `seccomp.json` is Docker's default profile plus unprivileged user-namespace
  creation (`unshare` and `clone` with `CLONE_NEWUSER`), which the vendor tools'
  sandboxes need (section 3.5). The `agentic-org` AppArmor profile is
  `docker-default` plus `userns`, for Docker hosts that restrict unprivileged user
  namespaces through AppArmor (Ubuntu 24.04 and later); on a Docker host without
  AppArmor that option is left out. `apparmor=unconfined` and `--privileged` are
  never used.
- **The first registration link.** Non-interactive setup prints none, and nothing is
  written to the container log. The administrator runs
  `docker exec -u 0 agentic-org agentic-org admin reset --user agentic` (root, naming
  the service user, as SIGN-33 requires), which prints the link on that terminal.

**Without a terminal.** `setup --non-interactive --config <file>` gains two
properties: `service` (`registered` or `foreground`) and `credentials` (the absolute
store directory of section 3.2, required on Linux). The listener's certificate pair
is named by a path in the secrets volume, never carried in the configuration.

### 3.5 Agent isolation

`same-user` needs nothing new on Linux. `org-supervise` already starts agents with
`posix_spawn`, a new session, `closefrom`, a pidfd for exit and the start time from
`/proc/<pid>/stat` for identity; that code moves into `org_platform::process`
unchanged, and the private home of ADR-0015 decision 10 is portable. The contract
(`org_platform::isolation`) carries the mode into the spawn and refuses any mode
other than `same-user` on every system until L6.

One Linux fact binds the vendor runtime adapters (C3): the vendor tools' own sandboxes
on Linux create unprivileged user namespaces. Docker's default seccomp profile refuses
that, and Ubuntu 24.04 and later restrict it through AppArmor
(`kernel.apparmor_restrict_unprivileged_userns = 1`) for any program without a profile
that grants `userns`. A runtime adapter reports whether its sandbox is available, and
the office does not start an agent whose sandbox is unavailable; it is never started
without one. The container file's seccomp and AppArmor profiles (section 3.4) allow
it in a container. On a Linux host with the AppArmor restriction, setup reports it and
ships `deploy/apparmor/agentic-org-agents`, a profile granting `userns` to the vendor
sandbox helper, which an administrator installs; until then no agent starts there.

`dedicated-user` (L6) uses an account created in the image or by setup and a small
launcher; it is designed then.

## 4. The other platform capabilities

ADR-0017's table names five contracts; these five capabilities are the rest of the
operating-system differences in the workspace. They live in `org-platform`, have the
same three adapters, and LNX-5's check covers them.

- **`fs`.** Built in X1b as `full_sync`, `rename_noreplace_at`, `rename_noreplace`,
  `set_errno`, `device_and_inode`, `check_instance_root`, `require_disk` and the
  `open(2)` flag and error constants; the file name comparison is
  `org_platform_host::names::paths_may_alias`, because its macOS adapter needs
  CoreFoundation. What each does: `full_sync(&File)`: `fsync`, and on macOS also `F_FULLFSYNC`, with no
  downgrade when it fails. `rename_noreplace_at(dir, from, dir, to)`: `renameatx_np`
  with `RENAME_EXCL` on macOS, `renameat2` with `RENAME_NOREPLACE` on Linux; a
  filesystem without it refuses. `set_errno`. `check_instance_root(dir)`: Linux
  refuses a case-folding directory and a filesystem other than ext4, XFS without
  case-insensitive names, Btrfs, F2FS or overlayfs (the checks in
  `instance/directory.rs` and `instance/registry.rs` today); macOS keeps its rules.
  `require_disk(dir)`: refuses `tmpfs`, `ramfs` and `hugetlbfs` where data must
  persist. These replace the copies in `org-host` (`writer.rs`, `hostdb.rs`,
  `release/files.rs`, `release/files/output.rs`, `human/recovery.rs`,
  `instance/directory.rs`), `org-office/src/store/acceptance/durable.rs`,
  `org-client/src/agent.rs` and `org-spool/src/safe_read.rs`.
- **`dirs`.** The application base, the default office home, the user temp base and
  the runtime base for sockets, with the socket path limit. Built in X1b:
  `application_base` and, with the environment left out, `account_application_base`
  (a release host is pinned to the second and refuses when the first differs, so
  `XDG_DATA_HOME` cannot move it), `user_temp_base`, `environment_spelling`,
  `socket_path_limit`. The runtime base for sockets is built when X2 needs it. macOS:
  `~/Library/Application Support/agentic.org`, `_CS_DARWIN_USER_TEMP_DIR`, 103 bytes.
  Linux: `${XDG_DATA_HOME:-~/.local/share}/agentic-org/`,
  `${XDG_CACHE_HOME:-~/.cache}/agentic-org/tmp/` (on disk; `require_disk` applies)
  and `$XDG_RUNTIME_DIR/agentic-org/` (or `/run/agentic-org/` in the container), 107
  bytes. These replace the hard-coded macOS values in `org-proto/src/location.rs`
  (`default_office_home`, `MAX_SOCKET_PATH_BYTES`), used by `agent-os`,
  `agentic-org-office`, `agentic-org serve` and `supervisor/child.rs`;
  `OFFICE_HOME_IN_ACCOUNT` in `org-host/src/home.rs`; the path in
  `instance/setup.rs`; the `/var/tmp` and `/private/tmp` choice in `instance/mod.rs`;
  `instance/temp.rs`; and `human.rs` (`user_temp`).
- **`process`.** Everything `org-supervise` asks of the kernel: credential checks,
  exit notification (kqueue or pidfd), process start identity (`proc_pidinfo` or
  `/proc/<pid>/stat`), process-group membership, the native recovery scan, and spawn
  flags (`POSIX_SPAWN_SETSID`, `closefrom`, `pipe2`).
- **`socket`.** Peer UID (`getpeereid` or `SO_PEERCRED`), the receive flags for
  descriptors (`MSG_CMSG_CLOEXEC` on Linux, a fenced `FD_CLOEXEC` on macOS), and
  whether a descriptor is a listening socket. `org-socket` calls these.
- **`presence`.** The recovery release's local-context query and confirmation
  (ADR-0011). macOS: today's bridge, returning `Confirmation::OsDialog`. Linux:
  `local` requires that the peer has a controlling terminal and is its foreground
  process group, read from `/proc/<pid>/stat`. The peer is identified by
  `SO_PEERPIDFD` on kernels 6.5 and later, so the PID cannot be reused under the
  check, and by the `SO_PEERCRED` PID before that; neither is a security control
  against a same-user process, which can satisfy the check itself. `confirm` returns
  `Confirmation::BrowserApproval`, and `org-host` then requires the signed-in
  administrator's approval of this run in the browser as the one confirmation. A
  remote shell is a local context on Linux; a run without a terminal receives no code.
  The host branches on the returned confirmation kind, never on the system.
  `local_authority` decides who may reset the administrator: on macOS the OS dialog
  at the console, as for the recovery code; on Linux a peer whose `SO_PEERCRED` UID
  is 0, acting on the instance of the user that `sudo` names (`SUDO_UID`) or that
  `--user` names (SIGN-33). Root is admitted on `host.sock` for this method only.

## 5. What a local attacker can still do on Linux

A process of another ordinary user can do nothing beyond what file permissions allow:
the data root, the credential store, `host.sock` and the office sockets are owner-only,
and every socket checks the peer's UID. Root and the container runtime can do
everything. The cases that matter run as the service user:

- **Read every host secret,** because the credential store is files the service user
  can read. On macOS the Keychain gives them only to the signed host. This is the
  largest difference, and it includes a `same-user` agent that escapes its vendor
  sandbox. What each item allows:
  - an office secrets key unwraps that office's private keys, among them the
    organization's CA key: the attacker can mint office certificates and enrol an
    office of their own;
  - the web TLS key lets the attacker impersonate HQ to browsers that trust it;
  - a pending recovery code is readable directly. On Linux the browser approval and
    the terminal checks of the recovery release are therefore a gate against
    mistakes, not a security control against a same-user process.
  `dedicated-user` (L6) closes this for agents; a separate service account closes it
  for the person's other programs.
- **Read and change the organization's data** directly, as on macOS (ADR-0016).
- **Replace the host's own executable, or edit the systemd unit that names it,**
  before the host next starts. The replacement then skips every check. Replacing the
  worker or the web executable is detected, because their digest is checked against
  the signed manifest, but only by an honest host. A release installed under a
  root-owned path closes this; the container image already does.
- **Impersonate the host to a command.** On Linux the `agentic-org` command cannot
  authenticate `host.sock`: a same-user process can bind its own socket at that path.
  The command then talks to the impostor, which can show it anything (a false
  registration link, for example) and capture what the person types. It receives no
  credential the real host would accept, because sign-in is approved in the browser:
  a command never sends a password or a recovery secret to a host it knows only by
  its user (enforced in the command since X2's review; before, only the missing Linux
  presence adapter kept the recovery command from sending the administrator's
  password).
- **Trace or alter what the person runs**: agent processes are ordinary dumpable
  processes. The host, the workers and the web process are not (section 3.1), and
  since X2's review neither is any other `agentic-org` command on Linux: the
  executable hardens itself before it reads its arguments. That protects a command's
  memory, not what the person types into a terminal the attacker can also read. The executable makes itself non-dumpable as its first act; a process that opened the host's `/proc/<pid>/mem` in the instant after
  the `exec` and before that call keeps its descriptor (measured), which adds nothing
  to what this user can already read.
- **Connect to `host.sock`.** Admission is by UID only. With one exception, nothing is
  granted without a session the person approved in the browser. The exception is the
  one initial registration link (`admin.link.initial`,
  [ADR-0022](../adr/0022-host-creates-its-registry-no-reverse-bootstrap.md)): setup
  asks for it before any account exists, so on Linux any process of the service user
  that names the setup operation, which the setup record holds, can take it, once.
  That user can also write the host's registry directly, so the link gives it nothing
  more.
- **Another account that shares the service user's group.** The credential store and
  the admission of a development image accept a directory or file that the service
  user's group can write when that group is the user's private group: its primary
  group, named as the user, with a member list that is empty or names only the user.
  An account that root created with that group as its own primary group is not in the
  member list and is not detected (finding it means enumerating every account). Such
  an account can read no item and plant none, but it can rename an ancestor of the
  store and put an older copy the service user owns in its place, and it can rewrite a
  development build's group-writable executable before the host hashes it. A release
  image is covered by its pinned digest. Do not give another account the service
  user's group.
- **Stop the service**: kill or stop the host or its children. This denies service and
  exposes nothing; recovery is as after a crash.
- **Make the web process refuse to start** on a kernel without Landlock or under a
  restrictive profile. That is lost availability, never a web process without
  confinement.

**A container on a Linux Docker host.** Without user-namespace remapping, the
container's UID is the same number on the host. Any host process with that UID can
reach `/data` and `/secrets` through `/proc/<pid>/root` of a container process, and
can `ptrace` and signal the container's dumpable processes. The container file's UID
61000 is chosen so that no ordinary host account has it; the documentation tells
operators who share the host to enable Docker's `userns-remap` or rootless mode, which
map the container's users to unprivileged host IDs. Docker Desktop on a Mac runs
containers inside its own virtual machine, where no Mac process has that UID.

What the Linux design does not lose against macOS: a worker or web process that is not
the verified image never receives a secret, and nothing resumes a child before its
check, because there is no suspended child to resume.

## 6. What this design was checked against

The kernel facilities were probed in this repository's Linux development container
(x86-64, kernel 6.18, no Yama, as root and, for the dumpable checks, as an
unprivileged user), and again by an independent review:

| Facility | Result |
| --- | --- |
| Landlock | ABI 7 |
| seccomp filters, `SECCOMP_RET_KILL_PROCESS` | available; the container itself runs without a filter |
| `pidfd_open`, `clone3` with `CLONE_PIDFD`, `pidfd_send_signal` | available |
| `memfd_create` with `MFD_EXEC`, sealing with `F_SEAL_WRITE \| GROW \| SHRINK \| SEAL` | available on 6.18; `MFD_EXEC` is `EINVAL` before 6.3; a write after sealing fails with `EPERM`; `vm.memfd_noexec` is 0 |
| `execveat(AT_EMPTY_PATH)` of a sealed image set to `0111` | the child of an unprivileged user runs non-dumpable: `/proc/<pid>/mem` is root-owned and `/proc/<pid>/exe` is refused to the same user; without `0111` the child is dumpable |
| `LD_PRELOAD` into that child | the injected code ran: loader variables must be refused (section 3.1) |
| `execveat` of a `#!` script from a memory file | `ENOENT` |
| The child's `current_exe()` | `/memfd:<name> (deleted)` |
| System calls of an HTTPS server under load | the list in section 3.3, including `clone3`, `rseq`, `sigaltstack` and `prctl` |
| Writing an executing file | refused with `ETXTBSY`; the design does not rely on it |
| Docker | CLI 29.9.2, `dockerd`, `containerd` and `runc` installed; no daemon running |

Not probed: Docker Desktop's kernel on a Mac (X4's first step) and a stock Ubuntu
24.04 host's AppArmor restriction (X5).

Code today, checked against the source: the host refuses to start a worker on Linux
(`spawn.rs`, "office servers run only on macOS"), `reader_is_child` refuses on every
system but macOS and six tests in `supervisor/channels/tests.rs` fail on Linux because
of it, and `cargo clippy -p org-host -p agentic-org --all-targets -- -D warnings`
fails on Linux with unused items that only macOS code reaches (the reason CI allows
`dead_code` and `unused_imports` for those two crates on Linux).

## 7. Working in parallel with the macOS track

Track X (Linux) runs beside Track A (getting in) and Track B (the store). The
increments are in the [implementation plan](../implementation-plan.md#milestone-two--linux-in-parallel-track-x).

| Increment | Files it changes | Also changed by |
| --- | --- | --- |
| X1a check and layout | `crates/org-platform/tests/platform_conditionals.rs`, `.github/platform-conditionals.allow`, `.github/scripts/platform_allowlist_shrinks.py`, `.github/workflows/ci.yml`; the layout text and `From<Refused> for io::Error` in `crates/org-platform/src/lib.rs` | A2 B0 created the crate, the `service` contract and its launchd adapter in `org-platform/src/service/` |
| X1b `fs`, `dirs`, `process`, `socket` | `org-platform/src/{fs,dirs,process,socket}/`; `org-proto/src/location.rs`; `org-socket/src/{peer.rs, rights.rs, rights/}`; `org-supervise/src/{lib.rs, platform.rs, spawn.rs, child.rs, recovery.rs, recovery/scan.rs, recovery/native/}`; `org-host/src/{home.rs, writer.rs, hostdb.rs, release/files.rs, release/files/output.rs, human/recovery.rs, instance/directory.rs, instance/mod.rs, instance/temp.rs, instance/setup.rs, instance/registry.rs, human.rs (paths)}`; `org-office/src/store/acceptance/durable.rs` (or `org-store/src/files.rs`, where B1a carries its flush); `org-client/src/agent.rs`; `org-spool/src/safe_read.rs`; `apps/agent-os/src/office.rs`, `apps/agentic-org-office/src/lib.rs`, `apps/agentic-org/src/serve.rs` (location call sites) | A2: `instance/`, `human.rs`, `home.rs`, `hostdb`, `apps/agentic-org/src/serve.rs`. A3: `hostdb`. A5: `org-client` |
| X1c `admission`, `confinement`, `presence` | `org-platform/src/{admission,confinement,presence}/`, `org-platform/build.rs`; `org-socket/src/{confinement.rs, confinement/, image.rs}`; `org-host/src/{spawn.rs, server_check.rs, code_signing.rs, peer.rs, app_auth.rs, human.rs (verify_host), recovery.rs, recovery/presence.rs, release.rs, release/platform.rs, release/runtime.rs, supervisor/child.rs, supervisor/channels.rs, supervisor/mod.rs, offices.rs, lib.rs}`, `org-host/build.rs`, `org-host/Cargo.toml` | A1: `supervisor/child.rs`, `supervisor/channels.rs`, `supervisor/mod.rs`. A2 and A3: `org-host/src/lib.rs`, `instance/`, `hostdb`. A3: the web process's start and its confinement call. A4: `offices.rs` |
| X1d `credentials` | `org-platform/src/credentials/`; `org-host/src/{keys/, host_secrets.rs, host_secrets/}` | A1 (`keys/`, the office secrets key) |
| X2 first worker | `org-platform/src/{admission,fs,dirs,process,socket}/linux.rs`, `org-platform/src/testing.rs`; the tests in `org-host/src/supervisor/` and `apps/agentic-org/tests/`; `ci.yml` | the tests in `apps/agentic-org/tests/` that A1 and A2 also edit |
| X3 credentials on Linux | `org-platform/src/credentials/linux.rs`; the `credentials` setup property | A2 (setup) |
| X4 web confinement | `org-platform/src/confinement/linux.rs` and its tests | A3 composes `confine_web` and `verify_web`; X4 does not edit the web process or the host's web start |
| X5 service and container | `org-platform/src/service/linux.rs`; the `service` setup property; `serve --setup`; the supplied-certificate verifier unless A3 has built it; `deploy/container/`, `deploy/apparmor/`, `.dockerignore` | A2 (setup), A3 (listener and TLS) |
| X6 close-out | `ci.yml`, the allowlist, `docs/` | none |

How the tracks avoid each other:

- **Land the crate and the check on the first day.** A2 B0 created `org-platform`; X1a adds the
  CI check with today's files as a shrinking allowlist; the A2 lead wrote the crate's
  first modules and the `service` contract with its launchd adapter.
  From then on the check refuses any new platform predicate outside `org-platform`.
- **Then carve the seams in three merges.** X1b moves the capabilities no Track A
  increment is redesigning; X1c moves admission, confinement and presence, ordered
  with A1 and landed before A3 starts the web process; X1d moves credentials after
  A1's key change. Each is behaviour-preserving on macOS and merged the day it is
  reviewed.
- **macOS verification.** The Track X engineer works on Linux, and macOS CI does not
  exercise the Keychain entitlement, Seatbelt, the presence dialog or Developer ID
  signing. Before X1c and X1d merge, the Track A engineer runs their signed-build and
  dialog checks on a Mac and records the result in the merge; X1b needs only macOS CI.
- **Agree the shared hot spots.** `supervisor/child.rs`, `supervisor/channels.rs`
  and `supervisor/mod.rs` (A1 removes the older launch form there), `keys/` (A1
  replaced the database key with the office secrets key), `org-host/src/lib.rs` and `instance/` (A2, A3),
  `hostdb` (A2, A3) and `offices.rs` (A4) are edited by both tracks. The Track X lead
  asks the other lead which lands first; the second rebases. X1c's change in those files
  is limited to call sites: `spawn_suspended`, `ServerCheck::admit`, `still` and
  `reader_is_child` become `Admission::spawn` and `verify_reader`; flush, rename and
  location helpers become `org_platform` calls.
- **After X1d, the rule is mechanical.** A Track A or B change that needs something
  new from the system adds it to `org-platform`: the macOS adapter, the refusing
  adapter, and a note for Track X, which writes the Linux adapter.
- **After X1d, Linux work stays inside `org-platform`** and the composition in
  `org-host`, so X2 to X4 do not touch the files A1 to A5 edit, except the shared tests
  of X2 and the setup properties of X3 and X5.
