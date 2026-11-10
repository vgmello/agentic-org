# org-supervise

Synchronous process primitives with private macOS/GNU-Linux implementations. The
only dependencies are `libc` and the `org-runtime` contract. The generic API knows
no office, employee, protocol, storage or adapter implementation.

This first delivery implements piped spawning and generated files. It is not a
complete agent runner: graceful stop/probes/socket cleanup, orphan discovery and
PTYs follow in separate deliveries. In particular, do not run real Claude until
orphan discovery is available: its tools can leave its process group.

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
   A deadline does not reap. Dropping the owner kills its group and reaps only
   this child. Full graceful lifecycle and detached descendants are not covered
   by this minimal guard. The integration layer must later complete cleanup
   before releasing an employee lock or starting a replacement.

The embedding process must never reap arbitrary children or configure automatic
child reaping (`SIGCHLD=SIG_IGN`/`SA_NOCLDWAIT`). Preparation rejects automatic
reaping; an observed `ECHILD` disarms the owner before any further signals. The
crate-local Clippy configuration forbids common accidental reaping/spawning paths.

`runtime::prepare` maps a piped `LaunchPlan`, creates its directories and atomically
replaces planned files without following symlinks. It returns the plan's stop,
cleanup, redaction and conversation metadata explicitly for the lifecycle layer;
it does not execute a stop. PTYs and `git_exclude` fail before any file writes.
`git_exclude` requires the caller-owned repository capability being settled in X-09.
No changes to `org-runtime` or adoption by the host are part of this delivery.

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
