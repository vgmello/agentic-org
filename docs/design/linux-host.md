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
all four executables. Nothing else in the workspace contains `target_os`,
`target_vendor` or `target_family`, in source or in tests.

- **Contents.** One module per contract (`admission`, `credentials`, `confinement`,
  `service`, `isolation`) and per supporting capability (`fs`, `dirs`, `process`,
  `socket`, `presence`; section 4). Each module has `macos.rs`, `linux.rs` and
  `refused.rs`, and exports one non-conditional constructor that returns the build
  target's implementation or the refusing one. The refusing adapter returns
  `Refused::Unsupported { capability, os }` and never falls back to another system's
  behaviour. A non-Unix target is a `compile_error!`; every other Unix target gets the
  refusing adapters.
- **Contracts live in `org-platform`.** A contract here is an operating-system
  capability (start this verified image, keep these bytes secret, confine this
  process), and four executables and five libraries consume it, so no single consumer
  can own it. Policy stays with the consumer: which image is trusted, which secret
  names exist, when a check is skipped in a development build. The crate takes that
  policy as plain data (paths, digests, signing identifiers) and decides nothing about
  the domain.
- **Dependencies.** `libc` and `sha2`; on macOS also `security-framework`,
  `security-framework-sys`, `core-foundation` and the `cc` build of the recovery
  presence bridge, all moved from `org-host`. It depends on no workspace crate.
  `org-socket`, `org-supervise`, `org-client`, `org-office`, `org-host` and the
  applications depend on it; there is no cycle.
- **What moves in, and what stays.** `org-socket` keeps its portable socket code (the
  ticked reader, descriptor-passing framing); its `confinement`, `image` and the
  platform parts of `peer` and `rights` move into `org-platform`. `org-supervise`
  keeps process ownership policy; `src/platform.rs`, `src/recovery/native/` and the
  conditionals in `spawn.rs` and `child.rs` move into `org_platform::process`.
  `org-host` keeps the admission and custody policy; its Security-framework,
  Keychain, audit-token and SDK code moves.
- **Tests.** A test that needs a platform feature is an adapter test inside
  `org-platform`. Consumer tests are platform-neutral and run on both systems against
  the real adapter, or against a test adapter built from the contract.
- **The check (LNX-5).** CI runs one script that fails when any `.rs` file outside
  `crates/org-platform/` contains `target_os`, `target_vendor` or `target_family`. Its
  allowlist, `.github/platform-conditionals.allow`, names only test files gated today
  because Linux cannot yet start a worker; the script also fails when a listed file no
  longer has a conditional, so the list only shrinks, and it is empty when X2 is done.

Rejected: renaming `org-socket` to `org-platform`, because the rename ripples through
files the macOS track is editing (`apps/agentic-org-office/src/serve.rs`, the host
supervisor); and one contract per consumer crate, because the same five capabilities
would be declared four times.

## 2. What each executable needs from the platform

| Executable | Platform concern | Where it is today | Behind |
| --- | --- | --- | --- |
| `agentic-org` (`serve`) | Start an office worker and admit it | `org-host/src/spawn.rs`, `server_check.rs`, `code_signing.rs`, `peer.rs`, `supervisor/child.rs`, `supervisor/channels.rs` (`reader_is_child`) | `admission` |
| `agentic-org` (`serve`) | Start the web process and verify its confinement | Not wired (A3); host side is `org-socket/src/confinement.rs` (`verify_gateway`) | `admission`, `confinement` |
| `agentic-org` (`serve`) | Admit native clients on `host.sock` | `org-host/src/app_auth.rs`, `bootstrap.rs`, `human.rs` (`verify_host`), `peer.rs` | `admission` (peer) |
| `agentic-org` (`serve`) | Office secrets keys and host secrets | `org-host/src/keys/` (`mod.rs`, `keychain.rs`, `dev.rs`), `host_secrets.rs`, `host_secrets/` (`keychain.rs`, `dev.rs`) | `credentials` |
| `agentic-org` (`serve`, `setup`) | Register, start and stop the service; stop on a signal | Not built (A2); `org-host/src/signals.rs` is portable | `service` |
| `agentic-org` (`serve`, `setup`) | Release images: static check and the host's own image | `org-host/src/release/platform.rs`, `release/runtime.rs` (`check_self`), `org-socket/src/image.rs` | `admission` |
| `agentic-org` (`setup`, `serve`) | Instance, temp and socket locations; filesystem checks | `org-host/src/home.rs`, `instance/directory.rs`, `instance/mod.rs`, `instance/temp.rs`, `instance/setup.rs`, `human.rs` (`paths`, `user_temp`) | `dirs`, `fs` |
| `agentic-org` (`serve`) | Durable flush and exclusive rename | `org-host/src/writer.rs`, `hostdb.rs`, `release/files.rs`, `release/files/output.rs`, `human/recovery.rs`, `host_secrets/dev.rs` | `fs` |
| `agentic-org` (`recovery`) | Local presence and confirmation for the recovery code | `org-host/src/recovery/presence.rs`, the bridge in `org-host/build.rs` | `presence` |
| `agentic-org-office` | Its end of the rendezvous: receive descriptors | `org-socket/src/rights.rs`, `rights/send.rs`, `rights/stream.rs` | `socket` |
| `agentic-org-office` | Admit same-user peers on its local socket | `org-socket/src/peer.rs` (`peer_allowed`), called from `apps/agentic-org-office/src/serve.rs` | `socket` |
| `agentic-org-office` | Start, watch, stop and recover agent processes | `org-supervise/src/platform.rs`, `spawn.rs`, `child.rs`, `recovery/native/` | `process`, `isolation` |
| `agentic-org-office` | Durable flush of accepted work | `org-office/src/store/acceptance/durable.rs` | `fs` |
| `agentic-org-web` | Confine itself before input | `org-socket/src/confinement.rs`, `confinement/macos.rs` | `confinement` |
| `agentic-org-web` | Check the inherited listener and received descriptors | `org-socket/src/rights/stream.rs` (`listening`), `rights.rs` | `socket` |
| `agent-os` | Connect to the office socket | `org-client/src/lib.rs` through `org-socket`'s ticked reader; portable, no peer check today | `socket` (unchanged) |
| `agent-os` | Durable queue writes and safe reads | `org-client/src/agent.rs` (`rename_new`, `full_sync`), `org-spool/src/safe_read.rs` (deleted with B6) | `fs` |

Portable already, and staying where they are: core-dump limits in each `main.rs`,
signal handlers in `apps/agentic-org-office/src/signals.rs`, `apps/agent-os/src/main.rs`
and `org-host/src/signals.rs`, and the descriptor swap in
`apps/agentic-org-office/src/fdswap.rs`. No executable's own source has a platform
conditional outside its tests; the tests that have one are listed in the allowlist of
section 1 or move into `org-platform`.

## 3. The five contracts

Linux hosts need kernel 6.2 or later with Landlock enabled (section 3.3 sets the
floor; admission alone needs 5.2). The host refuses to run as root on every system:
the service account is an ordinary user, and the container file creates one.

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
    pub fn spawn(&self, args: &[&OsStr], env: &[(OsString, OsString)]) -> io::Result<Started>;
    pub fn verify_reader(&self, child: &Child, stream: &UnixStream) -> Result<(), Refused>;
}
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

**macOS adapter.** Today's code, unchanged in behaviour: `spawn_suspended`, the audit
token from `task_name_for_pid`, the Developer ID requirement and hardened-runtime
check, the pinned CDHash, `SIGCONT`, and the token comparison in `verify_reader`.
`org-host/src/spawn.rs` and the Security-framework half of `code_signing.rs` move;
`ServerCheck` in `server_check.rs` becomes the policy that builds `ExpectedImage`
(today: a debug build skips the check, a release build without `ORG_APP_TEAM_ID`
refuses, a release with a runtime pin adds the manifest CDHash,
`org-host/src/lib.rs` `startup_server_check`).

**Linux adapter.** The worker runs from bytes the host verified, not from a path.

1. When the host builds the `Admission`, once per host lifetime, it opens the pinned
   path with `O_RDONLY | O_NOFOLLOW | O_CLOEXEC`, requires a regular file owned by the
   service user or root and not writable by group or others, and copies it into
   `memfd_create(name, MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_EXEC)` while computing
   SHA-256. It seals the copy with `F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK |
   F_SEAL_SEAL` and sets its mode to `0111` with `fchmod`.
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
   exactly descriptors 0 to 2.
4. `Child` holds the pidfd. Exit is observed by polling it, signals use
   `pidfd_send_signal`, and the child is never reaped elsewhere, so its PID is not
   reused while the host holds it.
5. `verify_reader` reads `SO_PEERCRED` on the accepted rendezvous: the PID must equal
   the child's and the UID the host's, and `pidfd_send_signal(pidfd, 0)` must succeed.

There is no start-suspended step, and none is needed. macOS checks the image after
`exec`, so it must stop the child until the check is done. Linux checks the bytes
before `exec` and executes exactly those bytes from a sealed descriptor that nobody
can change. Because the image is not readable (`0111`), the kernel starts the child
non-dumpable: from its first instruction no same-user process can `ptrace` it, read
or write `/proc/<pid>/mem`, or follow `/proc/<pid>/exe`. The worker never calls
`exec` on itself, so the PID that connects is still the verified image. The host also
sets `PR_SET_DUMPABLE` to 0 at startup, so its own memory is equally closed.

`peer` on Linux returns the UID and PID from `SO_PEERCRED`. A native client is
admitted on `host.sock` by UID alone: a command started by the person has no code
identity the kernel can vouch for, so its authority is only the session the person
approved in the browser (A5). `own_image` hashes `/proc/self/exe` and compares it
with the manifest entry for `agentic-org`; `release/platform.rs` accepts a Linux
executable on the signed manifest digest alone, after its ELF header check.

**What it proves.** The worker's and the web process's running code is the exact
byte sequence whose SHA-256 the signed release manifest names (in a development
build, the bytes the host read at startup), and the rendezvous peer is that process.
**What it does not prove.** There is no kernel-enforced code signature: the proof
rests on the manifest signature checked when the host starts and on the host itself
being the intended binary. A same-user attacker who replaces the `agentic-org`
executable before the host next starts runs a host that skips every check; macOS
binds the Keychain to the signed host and Linux has nothing equivalent (section 5).
A native client's image is not identified.

**Failing closed.** Any failed step refuses the spawn, kills and reaps a started
child, and counts as today's `storage_error`; nothing is sent. `vm.memfd_noexec = 2`,
a missing `MFD_EXEC` on a kernel with `vm.memfd_noexec = 1`, or a container profile
that refuses `memfd_create`, `clone` with `CLONE_PIDFD` or `execveat` refuses the
start and names the call and the error.

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
    pub data_root: &'a Path,             // must stay disjoint from the store
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
  `${XDG_STATE_HOME:-~/.local/state}/agentic-org/credentials/<instance-id>/`. In a
  container, a separately mounted secrets volume (`/secrets` in the container file).
  Setup records the absolute path in the setup record; the host never derives it from
  its environment at start.
- **Checks when the store opens.** The process is not root. The path is absolute
  with no `..`, and is walked from `/` with `openat(O_DIRECTORY | O_NOFOLLOW)` one
  component at a time. Every ancestor is owned by root or the service user and is not
  writable by group or others, unless it is root-owned and sticky. The store
  directory is owned by the service user with mode exactly `0700`. Its filesystem is
  not `tmpfs` or `ramfs` (`fstatfs`), because a lost office secrets key loses that
  office's private keys. Its canonical path and the data root's are disjoint in both
  directions. An exclusive `flock` on `lock` admits one host per instance.
- **Checks on every item.** A regular file opened with `openat` and `O_NOFOLLOW`,
  owned by the service user, mode exactly `0600`, one link, at most 16 KiB. A write
  goes to a new temporary file, `fsync`, then `renameat2` with `RENAME_NOREPLACE`
  for an add or a plain rename for a replace, then `fsync` of the directory.
- **Failing closed.** A wrong owner, mode, filesystem, location or lock refuses the
  host's start with the exact path and rule broken. Nothing repairs permissions.
- **In a container.** The secrets volume is created by the container file with the
  service user as owner and mode `0700`; a bind mount the operator prepared wrongly is
  refused as above. Without a secrets volume, setup refuses: the container's writable
  layer is not a store.

**What it proves.** Only the service user and root can read the items, none is in
the data volume, and a wrong setup refuses (LNX-4). **What it does not prove.** Any
process running as the service user reads every item; macOS limits Keychain items to
the signed host. Agents in `same-user` mode run as that user, so their protection from
these files is the vendor tool's sandbox (section 5).

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
force, so the web process cannot use the stream unconfined on any system.
`Enforcement` is the evidence the web process reports in `ReadyForSecrets`.

**macOS adapter.** ADR-0008: the thread and descriptor preconditions, the Seatbelt
profile, then the connection; the host's `sandbox_check` probes
(`org-socket/src/confinement/macos.rs`).

**Linux adapter.** Called on the main thread before any runtime thread.

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
   on x86-64), allows a fixed list of system calls per architecture (memory, futex,
   epoll, read and write, `accept4`, `recvmsg` and `sendmsg`, `close`, clocks,
   `getrandom`, signals to itself, thread `clone` with `CLONE_THREAD` checked in the
   flags), returns `ENOSYS` for `clone3` so the C library falls back to `clone`, and
   kills the process on every other call, among them `socket`, `connect`, `bind`,
   `listen`, every `open`, `execve`, `execveat`, `ptrace`, `process_vm_*`, `unshare`,
   `setns`, `mount`, `bpf`, `io_uring_setup` and `memfd_create`. The list is proved by
   running the web process's complete HTTP and TLS test path under the filter.
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
not replace this filter.

Rejected: an allowlist that returns an error instead of killing, because a forbidden
call that fails quietly hides a broken assumption; connecting after confinement as
macOS does, because Landlock does not control connecting to a named Unix socket on
the 6.2 floor, and denying `connect` outright is simpler.

### 3.4 Service lifecycle

**Contract** (`org_platform::service`):

```rust
pub enum ServiceMode { Registered, Foreground }
pub struct Unit { pub instance_id: String, pub program: PathBuf, pub args: Vec<OsString>, pub env: Vec<(OsString, OsString)> }
pub fn service(mode: ServiceMode) -> Result<Box<dyn ServiceManager>, Refused>;
pub trait ServiceManager {
    fn register(&self, unit: &Unit) -> Result<(), ServiceError>;   // idempotent, never takes over another unit
    fn start(&self, instance_id: &str) -> Result<(), ServiceError>;
    fn stop(&self, instance_id: &str) -> Result<(), ServiceError>;
    fn status(&self, instance_id: &str) -> Result<ServiceStatus, ServiceError>; // includes login and linger requirements
    fn unregister(&self, instance_id: &str) -> Result<(), ServiceError>;
}
```

`agentic-org serve` is the same foreground process in every mode; it stops on
`SIGTERM` or `SIGINT` (`org-host/src/signals.rs`): it stops accepting, stops the web
process and the workers (`SIGTERM`, then `SIGKILL` after 5 s), flushes its logs and
exits 0 within 8 s, inside a container runtime's default 10-second grace.

**macOS adapter.** A per-user launchd agent (built in A2).

**Linux adapter, `Registered`.** A systemd user unit,
`~/.config/systemd/user/agentic-org-<instance-id>.service`, with
`ExecStart=<pinned agentic-org> serve --instance <id>`, `Type=exec`,
`KillMode=mixed`, `TimeoutStopSec=15`, `Restart=on-failure`, `UMask=0077` and
`NoNewPrivileges=yes`, registered with `systemctl --user daemon-reload` and
`enable --now`. Without a user manager (no `XDG_RUNTIME_DIR` or user bus) it refuses
and names `Foreground`. When lingering is off, `status` and setup report that the
instance runs only while the user is logged in; setup never enables lingering, which
needs an administrator.

**`Foreground`, the container mode.** Nothing is registered: the container runtime
is the supervisor. Setup is told the mode explicitly; nothing detects a container.
The container's command is `agentic-org serve --setup /etc/agentic-org/setup.json`:
when that instance is not set up yet, `serve` first runs the same non-interactive,
offline setup (`setup --non-interactive --config`), which repeats as a no-op later,
then serves. Setup prints no link; `docker exec <container> agentic-org admin reset`
prints one. The image's entry point is `tini`, so orphaned agent processes are reaped
and the stop signal reaches `serve`.

**Without a terminal.** `setup --non-interactive --config <file>` gains two
properties: `service` (`registered` or `foreground`) and `credentials` (the absolute
store directory of section 3.2, required on Linux).

### 3.5 Agent isolation

`same-user` needs nothing new on Linux. `org-supervise` already starts agents with
`posix_spawn`, a new session, `closefrom`, a pidfd for exit and the start time from
`/proc/<pid>/stat` for identity; that code moves into `org_platform::process`
unchanged, and the private home of ADR-0015 decision 10 is portable. The contract
(`org_platform::isolation`) carries the mode into the spawn and refuses any mode
other than `same-user` on every system until L6.

One Linux fact binds the vendor runtime adapters (C3): the vendor tools' own sandboxes
on Linux use unprivileged user namespaces, which a container's default seccomp
profile often refuses. A runtime adapter reports whether its sandbox is available,
and the office does not start an agent whose sandbox is unavailable; it is never
started without one.

`dedicated-user` (L6) uses an account created in the image or by setup and a small
launcher; it is designed then.

## 4. Supporting capabilities

## 5. What a local attacker can still do on Linux

## 6. What this design was checked against

## 7. Working in parallel with the macOS track
