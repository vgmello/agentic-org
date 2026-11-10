# org-supervise

Synchronous process primitives with private macOS/GNU-Linux implementations. The
only dependencies are `libc` and the `org-runtime` contract. The generic API knows
no office, employee, protocol, storage or adapter implementation.

Piped spawning, generated files, owned graceful/fast stopping, socket cleanup and
probe preparation are implemented. Recovery policy over fake operations is implemented;
native orphan discovery adapters and PTYs follow in separate deliveries.
Do not run real Claude until native orphan discovery is available: its tools
can leave its process group.

The required platforms are macOS 13+, Linux kernel 5.10+/glibc 2.34+. Root and
setuid/setgid executable launches are refused. Missing/restricted process APIs
fail the launch; there is no fallback to a weaker descriptor or identity policy.

The caller supplies captured environment data, a sanitized `SearchPath` (pin the
verified release helper directory first, exclude workspace and private-home roots),
its context values and a random session mark. Nothing is inherited from the
embedding server's environment. Captured/context/runtime-switch names have separate
allowlists; duplicate names and unreviewed adapter switches are errors. Debug
output shows environment names, never values. Paths/executables must have passed
the caller's preflight and reside in trusted locations: this is not code-signature
admission or protection against a hostile same-user replacement of the executable.

Ownership sequence:

1. Build `PreparedSpawn`, then `record_mark` using a callback that commits the mark
   durably. A failed callback cannot spawn.
2. `spawn` creates a new session, resets the signal mask/dispositions, closes every
   descriptor except the three explicitly duplicated pipes, and captures process
   start/boot identity. On macOS the child starts suspended, so a fast exit cannot
   remove its process-table identity before capture. It returns `PendingChild`,
   without access to stdin.
3. `record_identity` commits that identity. Only then does `Spawned` release stdin
   and the two readers to the pump. A failed callback kills/reaps the child.
   On macOS success also resumes the child. Suspension is a lifecycle ordering
   mechanism, not executable admission: another same-user process can send CONT.
4. The pump owns stdout/stderr for their entire lifetime. The supervisor never
   reads them. Its shared stdin is nonblocking; retry `WouldBlock` while draining
   output, and use idempotent `close` to end input.
5. `try_exit`/`wait_until` observe exit using `WNOWAIT`, preserving PID ownership.
   A deadline does not reap. After protocol close or its deadline, call `stop` on
   a worker while the pump continues draining. Its `StopPolicy` preserves each
   step's monotonic grace, injects leader TERM before the first requested signal,
   then finishes with group KILL. Fast mode uses leader TERM/five seconds/group
   KILL without protocol or stdin waits.
6. After natural or forced exit, `cleanup_and_reap` kills the residual group,
   removes validated sockets and the probe cwd, then reaps only this leader.
   `StopFailure` and `CleanupFailure` retain partial evidence and the owner for
   recovery. Keep the run lock and admit no replacement on unresolved cleanup.
   Dropping the owner is an emergency best-effort group-kill/reap guard. General
   detached descendant discovery is still required before real harness use.

The embedding process must never reap arbitrary children or configure automatic
child reaping (`SIGCHLD=SIG_IGN`/`SA_NOCLDWAIT`). Preparation rejects automatic
reaping; an observed `ECHILD` disarms the owner before any further signals. The
crate-local Clippy configuration forbids common accidental reaping/spawning paths.

The embedder must ignore or safely handle/block `SIGPIPE` on threads writing
`ChildStdin`, so a closed pipe produces `BrokenPipe` instead of terminating the
worker. Rust's default ignored disposition satisfies this; children still reset
it to the default. On macOS, every other child-creation path in the embedder must
also close descriptors by default, or serialize its spawn with the whole
`RecordedSpawn::spawn` call: Darwin has no atomic `pipe2(O_CLOEXEC)`. Linux uses
`pipe2(O_CLOEXEC)` to avoid this creation window.

Credential checks reject real/effective/saved root IDs on Linux and real/effective
root or `issetugid()` provenance on macOS. Credential policy is unit tested;
running the real privileged-credential queries under root requires a privileged
harness. Core-dump limits are inherited unchanged, and preparation/spawn refuse either
limit being nonzero. Ada's revised D-B32 assigns `RLIMIT_CORE` soft/hard0 to
host/server startup H-1, before concurrency. Tests set both to0 before their
harness starts; a private policy seam covers nonzero values/query errors even
when a developer already inherited hard0 (which fixtures cannot raise).

`runtime::prepare` maps a piped `LaunchPlan`, creates its directories and atomically
replaces planned files without following symlinks. It returns the plan's stop,
cleanup, redaction and conversation metadata explicitly for the lifecycle layer;
it does not execute a stop. PTYs and `git_exclude` fail before any file writes.
`git_exclude` requires the caller-owned repository capability being settled in X-09.
`runtime::stop_policy` and `runtime::socket_cleanup` translate those retained
instructions into generic core types. `runtime::prepare_probe` uses the same
mark/identity persistence callbacks and refuses all context-only environment names
(`ORG_*`, `GIT_CONFIG_*`, `GH_CONFIG_DIR`) before
creating files. The caller supplies an existing private session temp root; the
mapper allocates a fresh empty 0700 cwd inside it and returns initial stdin lines,
mode and timeout. The caller owns all ProbeSession I/O. The cwd is removed after
exit before reaping. Preparation/spawn/persistence failures use ownership guards;
emergency Drop cleanup is best effort and may leave a cwd for recovery.
Relative dotfiles stay inside that fresh directory.

No changes to `org-runtime` or adoption by the host are part of this delivery.

## Recovery policy (B3-02.4a)

`recovery::recover_startup` reads a consumer-owned `RecoveryRecords` port and uses
`RecoveryCheckpoint` for durable target inventory. It never retires a storage row,
writes SQL or releases a session lock. The caller excludes
concurrent spawning with the same mark. A mark-only record covers a crash before
identity persistence. A matching recorded leader receives TERM and up to five
seconds to exit; captured surviving descendants are checked/killed individually.
Reused PIDs, process groups and session hints cannot authorize a signal. A reboot
invalidates recorded identity/group hints but does not skip the full current mark scan.

Before TERM or KILL, the caller's checkpoint store durably appends the identities
and ancestry of every captured target to the matching row's inventory. The union
never shrinks and is capped at 256 identities; write failure or overflow permits
no signal or completion. Retry reloads this inventory, checks exact PID, start
and boot identity against a complete current snapshot, and still runs the full
mark scan. A reparented child stays a target after its parent exits. Resolved
entries remain until the consumer retires the row. The public store port carries
plain data; the scan journal and proof issuance stay private to the crate.

One scan pass captures all marked roots and descendants before killing them. It
always includes the full user-process enumeration, regardless of tool-session hints.
Zombies and an exact optional owned probe leader are skipped before environment
reads. S0 unreadable environments are counted and skipped, without failing the scan;
the caller surfaces these counts as warnings. A known live saved target skipped
under S0 prevents an empty pass, without making the pass itself fail. A known
target whose identity cannot be verified leaves recovery unresolved. No process
environment bodies are retained.

A shared crate-internal coordinator runs at most three passes, with a fresh
getentropy/getrandom nonce each time. It validates the echoed mark and nonce, waits
up to one second between nonempty passes, and requires a validated empty final
pass before issuing `RecoveryGone`. That sealed proof is keyed by mark; the consumer
binds it to its owned-process reference and commits its probe outcome before retiring
the reference. A signal/wait/deadline/scan failure retains incomplete evidence.

Process and scan traits are sealed. Startup uses only the internal scanner; callers
cannot supply an empty fake snapshot to obtain a real proof. This slice provides
only crate-local fake process operations and fake persistence tests. Slice 4b supplies
Linux pidfd + /proc and macOS verified identity + KERN_PROCARGS2 implementations and
real synthetic-process tests. Linux must refuse unavailable/restricted pidfds rather
than fall back to raw PIDs; macOS's residual non-child PID race remains explicit.
Processes clearing their environment and double-forking remain a containment limit.

Existing owned stop/cleanup is unchanged and does not call this recovery policy yet.
The later probe cleanup integration uses the shared coordinator after successful
owned group kill, then reaps last; no new runtime may start before cleanup completes.
No SQL, host/server wiring, or real-runtime admission is added here. Crate dependencies
are unchanged: first-party Ce=1 (`org-runtime`), Ce=2 including `libc`. Policy and its
sealed capability contracts remain in `org-supervise`; future storage adapters depend
on this consumer-owned read port, without adding a dependency from process policy to SQL.

Socket cleanup walks real paths from `/` using pinned no-follow dirfds. The final
parent must already be a current-UID directory with mode exactly 0700; nothing
repairs permissions. It checks the leaf with `fstatat(AT_SYMLINK_NOFOLLOW)` and
unlinks only current-UID sockets. The private parent narrows the window, not
atomic: deliberate same-UID replacement is outside v1's threat model (D-B22/D-B33).
A missing/unsafe parent or unsafe leaf leaves cleanup unresolved and the leader
unreaped. Missing socket leaves are recorded as absent.

On macOS a zombie-only group can make `killpg` return EPERM: XNU's
[killpg1](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c#L1612-L1621)
filters zombies before deciding whether any member was signalled. EPERM is
accepted only after WNOWAIT proves the owned leader exited, a complete libproc
snapshot contains exactly that leader, and ownership is rechecked (D-B34). An
extra member, unavailable enumeration or lost ownership fails closed. The query
sizes the buffer first, adds headroom, and refuses a full/truncated result.
Linux handling is unchanged.

The fresh probe cwd is removed only after its pinned private parent and allocated
inode identity are verified. It uses the standard library's
[remove_dir_all](https://doc.rust-lang.org/std/fs/fn.remove_dir_all.html), which
protects against symlink traversal on supported macOS/Linux systems. Renaming by
a deliberate same-UID actor is outside v1's threat model. An ownership violation
leaves the cwd for recovery rather than deleting it while child control is lost.

Directories are never chmodded: `mkdir` does not return an inode handle, so even
an opened directory may have been substituted after creation. A created-path inode
is accepted only when it is a no-follow directory owned by the current user with
mode exactly `0700`. Common umasks (`002`, `022`, `027`, `077`) already produce that
mode. Owner-bit-masking umasks such as `0100` or `0777` are explicitly refused;
universal umask support is not required for v1.0 (ada's decision). Existing planned
directories are validated for ownership and keep their modes. No helper or
process-wide umask mutation is used. Preparation may leave a partial directory on
failure; the caller/operator must resolve its permissions before retrying. No child
is spawned by a failed preparation.

Verification uses temporary roots and fake child processes only. The regression
suite covers received inheritable fds held by another thread at spawn, environment
and signal isolation, identity ordering, non-reaping waits, drop cleanup and
symlink/hardlink file protection. Linux tests must run as a non-root user; in a
container allow the required process syscalls in its seccomp policy. macOS process
tests need an environment that permits `setsid`/`posix_spawn` and process identity
queries (the developer tool sandbox denies these).
