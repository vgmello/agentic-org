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
  has `macos.rs`, `linux.rs` and `refused.rs`, and exports one non-conditional
  constructor that returns the build target's implementation or the refusing one. The
  refusing adapter returns `Refused::Unsupported { capability, os }` and never falls
  back to another system's behaviour.
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
- **Dependencies.** `libc` and `sha2`; on macOS also `security-framework`,
  `security-framework-sys`, `core-foundation` and the `cc` build of the recovery
  presence bridge, all moved from `org-host`. It depends on no workspace crate.
  `org-socket`, `org-supervise`, `org-client`, `org-office`, `org-host` and the
  applications depend on it; there is no cycle. `org-proto` does not depend on it:
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
- **The check (LNX-5).** `crates/org-platform/tests/platform_conditionals.rs` parses
  every `.rs` file outside `crates/org-platform/` with `syn` and counts each `cfg`,
  `cfg_attr` and `cfg!` predicate that names `target_os`, `target_vendor`,
  `target_env` or `target_family`, including those inside macro calls and
  definitions. It is a test of the workspace, so `cargo test --workspace` runs it on
  every system, and CI also runs it as its own step on macOS and Linux. String
  literals and fixtures (for example the TOML and `syn` fixtures in
  `org-office/src/store/tests/opener_policy.rs`) are not predicates and do not
  match; a file that does not parse fails the check. Its allowlist,
  `.github/platform-conditionals.allow`, was written in X1a from the check's own
  findings: 79 files with 495 predicates, one `path count` line each (a text search
  finds 81 files, two of them only through fixtures). The check fails on a predicate
  in a file that is not listed, on a listed file with more predicates than its
  count, and on a listed file with fewer than its count or none, so no file gains a
  predicate and the list only shrinks. X1b to X1d remove the source files, X2
  and the `testing` module remove the test files, and the list is empty at X6.
  Outside the check, and harmless: `cfg(unix)`, because a non-Unix target cannot
  build `org-platform` at all, so such a branch never selects between supported
  systems; and build scripts, of which only `org-host/build.rs` reads
  `CARGO_CFG_TARGET_OS` today, to compile the presence bridge, which moves with the
  bridge into `org-platform/build.rs` in X1c.

Rejected: renaming `org-socket` to `org-platform`, because the rename ripples through
files the macOS track is editing (`apps/agentic-org-office/src/serve.rs`, the host
supervisor); and one contract per consumer crate, because the same capabilities
would be declared four times.

## 2. What each executable needs from the platform

| Executable | Platform concern | Where it is today | Behind |
| --- | --- | --- | --- |
| `agentic-org` (`serve`) | Start an office worker and admit it | `org-host/src/spawn.rs`, `server_check.rs`, `code_signing.rs`, `peer.rs`, `supervisor/child.rs`, `supervisor/channels.rs` (`reader_is_child`), `supervisor/mod.rs` (macOS-only tests and scripted checks), `lib.rs` (`startup_server_check`) | `admission` |
| `agentic-org` (`serve`) | Start the web process and verify its confinement | Not wired (A3); host side is `org-socket/src/confinement.rs` (`verify_gateway`) | `admission`, `confinement` |
| `agentic-org` (`serve`) | Admit native clients on `host.sock`, including the recovery peer | `org-host/src/app_auth.rs`, `bootstrap.rs`, `human.rs` (`verify_host`), `recovery.rs` (`native()` admits only on macOS), `peer.rs` | `admission` (peer) |
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
| `agentic-org-web` | Confine itself before input | `org-socket/src/confinement.rs`, `confinement/macos.rs` | `confinement` |
| `agentic-org-web` | Check the inherited listener and received descriptors | `org-socket/src/rights/stream.rs` (`listening`), `rights.rs` | `socket` |
| `agent-os` | Find and connect to the office socket | `org-proto/src/location.rs` through `apps/agent-os/src/office.rs` and `org-client`; the ticked reader is portable | `dirs` |
| `agent-os` | Durable queue writes and safe reads | `org-client/src/agent.rs` (`rename_new`, `full_sync`), `org-spool/src/safe_read.rs` (deleted with B6) | `fs` |

Portable already, and staying where they are: core-dump limits in each `main.rs`,
signal handlers in `apps/agentic-org-office/src/signals.rs`, `apps/agent-os/src/main.rs`
and `org-host/src/signals.rs`, and the descriptor swap in
`apps/agentic-org-office/src/fdswap.rs`. No executable's own source has a platform
conditional outside its tests.

Count today: 79 `.rs` files outside any adapter name a platform target in their text. 44 are
source files: 29 in `org-host`, 6 in `org-socket`, 6 in `org-supervise`, and
`org-office/src/store/acceptance/durable.rs`, `org-client/src/agent.rs` and
`org-spool/src/safe_read.rs`; each is in a row above. 35 are test files. Milestone-one
work in these files adds no new predicate; a new need goes into `org-platform`.

## 3. The five contracts

Linux hosts need kernel 6.2 or later with Landlock enabled, and glibc 2.34 or later.
Section 3.3 sets the kernel floor; admission alone needs 5.9 (`close_range`).

The service account is an ordinary user on every system: `agentic-org serve`
refuses to run as root. That refusal is a check in the `serve` composition
(`Config::refuse_root`, on in every production entry), not in an adapter, so tests can
run the adapters as any user.

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

`spawn` returns only a child that has passed the platform's check; `verify_reader`
is called on the accepted rendezvous before and after `ReadyForSecrets`, exactly where
`reader_is_child` and `ServerCheck::still` are called today. The same `Admission`
type starts the office worker and the web process, each with its own
`ExpectedImage`.

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

1. When the host builds the `Admission`, once per host lifetime, it opens the pinned
   path with `O_RDONLY | O_NOFOLLOW | O_CLOEXEC`, requires a regular file owned by the
   service user or root and not writable by group or others, and copies it into
   `memfd_create(name, MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_EXEC)` while computing
   SHA-256. On `EINVAL` (a kernel before 6.3, which has no `MFD_EXEC` and makes every
   memory file executable) it retries once without `MFD_EXEC`. It seals the copy with
   `F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL` and sets its mode to
   `0111` with `fchmod`.
2. A release build compares the digest and size with the manifest entry
   (`FileEntry::sha256`, already signed and verified by `org-release`). A development
   build records the digest of its own build output at that moment and logs that the
   image has no publisher check, as macOS logs today for an unsigned build. A release
   build without a runtime release pin refuses: on Linux there is no other publisher
   identity.
3. `spawn` uses `clone(CLONE_PIDFD | SIGCHLD)` without a shared address space. The
   child runs only raw system calls: reset the signal mask and `SIGPIPE`, `dup2` the
   three pipes to 0 to 2, `chdir("/")`, duplicate the sealed image to descriptor 3
   with close-on-exec set, `close_range(4, ~0)`, then
   `execveat(3, "", argv, envp, AT_EMPTY_PATH)`; on failure it exits with 127. The
   kernel opens the image before it closes descriptor 3, so the worker starts with
   exactly descriptors 0 to 2. An interpreter script (`#!`) cannot start this way: the
   kernel fails with `ENOENT`, because the interpreter would open the descriptor
   after it is closed. Only compiled executables are admitted.
4. `Child` holds the pidfd. Exit is observed by polling it, signals use
   `pidfd_send_signal`, and the child is never reaped elsewhere, so its PID is not
   reused while the host holds it.
5. `verify_reader` reads `SO_PEERCRED` on the accepted rendezvous: the PID must equal
   the child's and the UID the host's, and `pidfd_send_signal(pidfd, 0)` must succeed.

There is no start-suspended step, and none is needed. macOS checks the image after
`exec`, so it must stop the child until the check is done. Linux checks the bytes
before `exec` and executes exactly those bytes from a sealed descriptor that nobody
can change. Because the image is not readable (`0111`), the kernel starts a child of
an unprivileged user non-dumpable: from its first instruction no same-user process
can `ptrace` it, read or write `/proc/<pid>/mem`, or follow `/proc/<pid>/exe`. (Root
can; for a root-owned process the property does not hold, which is one reason the
service refuses root.) The worker never calls `exec` on itself, so the PID that
connects is still the verified image. The host also sets `PR_SET_DUMPABLE` to 0 at
startup, so its own memory is equally closed. `/proc/<pid>/cmdline` and
`/proc/<pid>/status` stay readable to the same user, so no secret is ever in an
argument, as the launch contract already requires.

`peer` on Linux returns the UID and PID from `SO_PEERCRED`. A native client is
admitted on `host.sock` by UID alone (the service user's, or root for `admin reset`
only, section 4): a command started by the person has no code identity the kernel
can vouch for, so its authority is only the session the person approved in the
browser (A5). `own_image` hashes `/proc/self/exe` and compares it with the manifest
entry for `agentic-org`; `release/platform.rs` accepts a Linux executable on the
signed manifest digest alone, after its ELF header check.

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
`apps/agentic-org/tests/support/mod.rs`), and a script cannot be admitted. X2 adds
`org_platform::testing::path_admission()`, a test adapter built from the same
contract that starts a program by path with `posix_spawn` and checks the reader by
PID, available only under the `test-support` feature and never in a production
composition; those tests move onto it. The real Linux adapter is tested with a
compiled worker fixture. The non-dumpable test needs an unprivileged child: when the
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
pub fn confine_web(rendezvous: &Path) -> Result<(UnixStream, Enforcement), Refused>; // in the web process
pub fn verify_web(child: &Child, enforcement: &Enforcement, critical: &[&Path]) -> Result<(), Refused>; // in the host
```

`confine_web` returns the connected rendezvous stream only after confinement is in
force, so the web process cannot use the stream unconfined on any system. The order
inside differs by system (macOS confines, then connects; Linux connects, then
confines); either way nothing reads from the stream or reports ready before
confinement holds. `Enforcement` is the evidence the web process reports in
`ReadyForSecrets`.

**macOS adapter.** ADR-0008: the thread and descriptor preconditions, the Seatbelt
profile, then the connection; the host's `sandbox_check` probes
(`org-socket/src/confinement/macos.rs`).

**Linux adapter.** Called on the main thread before any runtime thread. The host
starts the web process without `RUST_BACKTRACE` (a backtrace opens files, which the
filter forbids), and `ChildEnv` refuses it for the web image.

1. Disable core dumps, set `PR_SET_DUMPABLE` to 0, and require exactly one thread
   (`/proc/self/status`, `Threads: 1`) and exactly descriptors 0 to 2
   (`/proc/self/fd`). Static assets are already in memory.
2. Connect to the rendezvous. This is the last new socket the process creates.
3. `PR_SET_NO_NEW_PRIVS`.
4. Landlock. Query the ABI; require 3 or higher. Create a ruleset that handles every
   filesystem right the running ABI knows, add no rule, and restrict the process: no
   path can be read, written, executed, created, renamed, linked or truncated. With
   ABI 4 or higher it also handles TCP bind and connect with no rule; with ABI 6 or
   higher it also scopes abstract Unix sockets and signals to the process.
5. seccomp. A filter that kills the process on a foreign architecture (including x32
   on x86-64) and allows a fixed list per architecture. The list, from a trace of the
   HTTPS path: memory (`mmap`, `munmap`, `mprotect`, `madvise`, `brk`), `futex`,
   `epoll_*`, `eventfd2`, `read`, `write`, `writev`, `recvfrom`, `recvmsg`,
   `sendmsg`, `sendto`, `accept4`, `getsockname`, `getsockopt`, `setsockopt`,
   `shutdown`, `close`, `fcntl`, `poll`, clocks, `getrandom`, `rt_sig*`,
   `sigaltstack`, `rseq`, `set_robust_list`, `gettid`, `sched_getaffinity`, `exit`,
   `exit_group`, signals to itself, thread `clone` with `CLONE_THREAD` checked in its
   flags, `prctl` limited by its first argument to `PR_SET_NAME` and `PR_GET_*`, and
   `ioctl` limited by request to `FIONBIO` and `FIONREAD`. It returns `ENOSYS` for
   `clone3`, so the C library falls back to `clone`, and kills the process on every
   other call, among them `socket`, `connect`, `bind`, `listen`, every `open`,
   `execve`, `execveat`, `ptrace`, `process_vm_*`, `unshare`, `setns`, `mount`,
   `bpf`, `io_uring_setup` and `memfd_create`. The list is proved by running the web
   process's complete HTTP and TLS test path under the filter, on the development
   system and in the container image, whose glibc may make other calls.
6. Report `Enforcement { landlock_abi, seccomp: true }` and send `ReadyForSecrets`.

On the host, `verify_web` reads `/proc/<pid>/status` of the admitted child, then
checks the child is still alive through its pidfd, and requires `NoNewPrivs: 1`,
`Seccomp: 2` and at least one filter, and a reported Landlock ABI of 3 or higher.
Any failure kills and reaps the web process before any descriptor or secret is sent.

**Minimum kernel.** Landlock ABI 3 is Linux 6.2; the kill-process action is 4.14. The
floor is 6.2 because Landlock before ABI 3 does not cover truncating a file by path,
and each layer alone must deny every file change.

**What it proves.** Once ready, the web process cannot open any file, create or
connect a socket, start a program or trace another process; it can only serve the
listener and descriptors it holds. **What it does not prove.** The host cannot
observe another process's Landlock domain: it trusts the report of an image it
admitted by digest, and observes seccomp and no-new-privileges itself. The macOS
host's path probes have no Linux equivalent.

**Failing closed, and in a container.** Landlock missing from the kernel's security
modules (`EOPNOTSUPP`), disabled, an ABI below 3, or a container seccomp profile that
refuses `landlock_create_ruleset` or `seccomp` (`EPERM`, `ENOSYS`) makes the web
process exit before readiness with the reason; the host reports the web service
unavailable and keeps native control (LNX-3). A container's own seccomp profile does
not replace this filter. Whether Docker Desktop's kernel on a Mac has Landlock decides
whether the web process can run in a container on a Mac at all; it is probed on the
first day of Track X, before this adapter's details are fixed (implementation plan,
X4).

Rejected: an allowlist that returns an error instead of killing, because a forbidden
call that fails quietly hides a broken assumption; connecting after confinement as
macOS does, because Landlock does not control connecting to a named Unix socket on
the 6.2 floor, and denying `connect` outright is simpler.

### 3.4 Service lifecycle

**Contract** (`org_platform::service`):

```rust
pub enum ServiceMode { Registered, Foreground }
pub struct Unit { pub instance_id: String, pub program: PathBuf, pub args: Vec<OsString>, pub env: Vec<(OsString, OsString)> }
impl Unit { pub fn check(&self) -> Result<(), ServiceError>; }
pub fn service(mode: ServiceMode) -> Result<Box<dyn ServiceManager>, Refused>;
pub trait ServiceManager: Send + Sync {
    fn register(&self, unit: &Unit) -> Result<(), ServiceError>;   // idempotent, never takes over another unit
    fn start(&self, instance_id: &str) -> Result<(), ServiceError>;
    fn stop(&self, instance_id: &str) -> Result<(), ServiceError>;
    fn status(&self, instance_id: &str) -> Result<ServiceStatus, ServiceError>;
    fn unregister(&self, instance_id: &str) -> Result<(), ServiceError>;
}
pub struct ServiceStatus { pub state: ServiceState, pub availability: Availability }
pub enum ServiceState { NotRegistered, Stopped, Running }
pub enum Availability { Always, WhileLoggedIn { remedy: Option<String> } }
pub enum ServiceError { InvalidUnit { reason }, NotRegistered, Conflict, Manager { action, detail } }
```

The rules every adapter keeps (the contract's documentation in
`crates/org-platform/src/service.rs` is the full text):

- `register` records the unit and does not start it. The same unit again succeeds and
  changes nothing. A different registration already under the instance's name is
  `Conflict` and nothing changes; a unit is replaced by `unregister`, then `register`.
- `start` and `stop` succeed when the instance is already in that state, and are
  `NotRegistered` for an instance without a registration. `status` reports
  `ServiceState::NotRegistered` instead of failing. `unregister` stops the instance,
  removes the registration, and succeeds when there is none.
- `Availability` is the login requirement: `Always`, or `WhileLoggedIn` with the
  command that lifts the limit when the system has one. Setup prints that command and
  never runs it unasked.
- `Unit::check` is applied by every adapter before it writes anything: an instance id
  of 1 to 64 ASCII letters, digits, `-` or `_` (it becomes part of a unit or label
  name), an absolute program path, no NUL or line break in any value, well-formed
  environment names, and no loader variable (`LD_*`, `DYLD_*`, `GLIBC_TUNABLES`), the
  rule `ChildEnv` applies to children (section 3.1).

The contract landed in X1a with the refusing adapter for every target, so A2 writes
the launchd adapter in `service/macos.rs` behind it and X5 the Linux adapters in
`service/linux.rs`; each adds its target's branch to the constructor. The Track A lead
reviews the contract when A2 starts; a change A2 needs is made in this section and in
the contract together.

`agentic-org serve` is the same foreground process in every mode; it stops on
`SIGTERM` or `SIGINT` (`org-host/src/signals.rs`): it stops accepting, stops the web
process and the workers (`SIGTERM`, then `SIGKILL` after 5 s), flushes its logs and
exits 0 within 8 s, inside a container runtime's default 10-second grace.

**macOS adapter.** A per-user launchd agent (written by A2 behind this contract).

**Linux adapter, `Registered`.** A systemd user unit,
`~/.config/systemd/user/agentic-org-<instance-id>.service`, with
`ExecStart=<pinned agentic-org> serve --instance <id>`, `Type=exec`,
`KillMode=mixed`, `TimeoutStopSec=15`, `Restart=on-failure`, `UMask=0077` and
`NoNewPrivileges=yes`, registered with `systemctl --user daemon-reload` and
`enable --now`. Without a user manager (no `XDG_RUNTIME_DIR` or user bus) it refuses
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

- **`fs`.** `full_sync(&File)`: `fsync`, and on macOS also `F_FULLFSYNC`, with no
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
  the runtime base for sockets, with the socket path limit. macOS:
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
  The command then talks to the impostor, which can show it anything and capture what
  the person types, but receives no credential the real host would accept, because
  sign-in is approved in the browser.
- **Trace or alter what the person runs**: the `agentic-org` command, and agent
  processes, are ordinary dumpable processes. The host, the workers and the web
  process are not (section 3.1).
- **Connect to `host.sock`.** Admission is by UID only; nothing is granted without a
  session the person approved in the browser.
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
| X1a crate and check | new `crates/org-platform/` (skeleton, `service` contract); the workspace `Cargo.toml`; `.github/workflows/ci.yml`, the check script and its allowlist | A2 writes the launchd adapter in `org-platform/src/service/macos.rs` |
| X1b `fs`, `dirs`, `process`, `socket` | `org-platform/src/{fs,dirs,process,socket}/`; `org-proto/src/location.rs`; `org-socket/src/{peer.rs, rights.rs, rights/}`; `org-supervise/src/{lib.rs, platform.rs, spawn.rs, child.rs, recovery.rs, recovery/scan.rs, recovery/native/}`; `org-host/src/{home.rs, writer.rs, hostdb.rs, release/files.rs, release/files/output.rs, human/recovery.rs, instance/directory.rs, instance/mod.rs, instance/temp.rs, instance/setup.rs, instance/registry.rs, human.rs (paths)}`; `org-office/src/store/acceptance/durable.rs`; `org-client/src/agent.rs`; `org-spool/src/safe_read.rs`; `apps/agent-os/src/office.rs`, `apps/agentic-org-office/src/lib.rs`, `apps/agentic-org/src/serve.rs` (location call sites) | A2: `instance/`, `human.rs`, `home.rs`, `hostdb`, `apps/agentic-org/src/serve.rs`. A3: `hostdb`. A5: `org-client` |
| X1c `admission`, `confinement`, `presence` | `org-platform/src/{admission,confinement,presence}/`, `org-platform/build.rs`; `org-socket/src/{confinement.rs, confinement/, image.rs}`; `org-host/src/{spawn.rs, server_check.rs, code_signing.rs, peer.rs, app_auth.rs, bootstrap.rs, human.rs (verify_host), recovery.rs, recovery/presence.rs, release.rs, release/platform.rs, release/runtime.rs, supervisor/child.rs, supervisor/channels.rs, supervisor/mod.rs, offices.rs, lib.rs}`, `org-host/build.rs`, `org-host/Cargo.toml` | A1: `supervisor/child.rs`, `supervisor/channels.rs`, `supervisor/mod.rs`. A2 and A3: `org-host/src/lib.rs`, `bootstrap.rs`. A3: the web process's start and its confinement call. A4: `offices.rs` |
| X1d `credentials` | `org-platform/src/credentials/`; `org-host/src/{keys/, host_secrets.rs, host_secrets/}` | A1 (`keys/`, the office secrets key) |
| X2 first worker | `org-platform/src/{admission,fs,dirs,process,socket}/linux.rs`, `org-platform/src/testing.rs`; the tests in `org-host/src/supervisor/` and `apps/agentic-org/tests/`; `ci.yml` | the tests in `apps/agentic-org/tests/` that A1 and A2 also edit |
| X3 credentials on Linux | `org-platform/src/credentials/linux.rs`; the `credentials` setup property | A2 (setup) |
| X4 web confinement | `org-platform/src/confinement/linux.rs` and its tests | A3 composes `confine_web` and `verify_web`; X4 does not edit the web process or the host's web start |
| X5 service and container | `org-platform/src/service/linux.rs`; the `service` setup property; `serve --setup`; the supplied-certificate verifier unless A3 has built it; `deploy/container/`, `deploy/apparmor/`, `.dockerignore` | A2 (setup), A3 (listener and TLS) |
| X6 close-out | `ci.yml`, the allowlist, `docs/` | none |

How the tracks avoid each other:

- **Land the crate and the check on the first day.** X1a creates `org-platform`, the
  CI check with today's files as a shrinking allowlist, and the `service` contract,
  agreed with the A2 lead, so that A2 writes launchd code behind it from the start.
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
  renames the database key), `org-host/src/lib.rs` and `bootstrap.rs` (A2, A3),
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
