//! What process supervision asks of the kernel.
//!
//! Linux host design section 4, `process`: credential checks, exit notification
//! (kqueue or pidfd), process start identity (`proc_pidinfo` or `/proc/<pid>/stat`),
//! process-group membership, spawn flags (`POSIX_SPAWN_SETSID`, `closefrom`, `pipe2`)
//! and the native recovery scan.
//!
//! `org-supervise` is the consumer. It keeps the ownership policy: who owns a child,
//! when a signal may be sent, what proves that a process belongs to a session, and
//! what a session mark may look like. This module only asks the kernel, and takes what
//! it needs as plain data (a PID, argument and environment strings, the name of the
//! mark variable).
//!
//! # Systems
//!
//! - macOS 13 or later: `kqueue`, `libproc`, `sysctl`, a suspended `posix_spawn`.
//! - GNU/Linux with Linux 5.10 and glibc 2.34 or later: pidfds, `/proc`, `openat2`,
//!   `posix_spawn` with `closefrom`.
//! - Every other Unix target: each function returns the `process` refusal
//!   ([`crate::Refused`] as an [`io::ErrorKind::Unsupported`] error).
//!
//! # Where an unsupported target refuses first
//!
//! [`validate`] is the first function `org-supervise` calls on every path: in
//! `PreparedSpawn::new`, again in `RecordedSpawn::spawn`, in the runtime probe before
//! it creates its directory, and (through [`RecoveryScan::new`]) in
//! `NativeRecovery::new`. On a target without an adapter its first step, reading the
//! process's user IDs, returns the refusal. Nothing has been created, started, watched
//! or signalled at that point, and no child or scan exists afterwards for any other
//! function to act on. The other functions refuse as well, so a caller that skips
//! `validate` still starts nothing.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

use std::ffi::{CStr, CString};
use std::fmt;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::Instant;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

mod limits;
#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
mod shared;

pub use limits::{disable_core_dumps, owner_only_umask};
#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
use shared::{check_time, native_mark, no_core_dumps, valid_pid};

#[cfg(all(
    test,
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))
))]
mod adapter_tests;

#[cfg(test)]
mod tests;

/// Close this process to the other processes of its user, before it holds a secret.
///
/// A process that will hold keys calls this first, before it reads one and before it
/// starts a thread or a child. It cannot be undone.
///
/// - Every system with an adapter: core dumps are disabled (`RLIMIT_CORE` soft and hard
///   limit 0), so a crash writes no memory to disk.
/// - GNU/Linux: the process is also made non-dumpable (`PR_SET_DUMPABLE` 0, then read
///   back). From then on no process of the same user can attach to it, read or write
///   its memory, follow its executable link or open its descriptors through
///   `/proc/<pid>`; root still can. The attribute is inherited by a child until that
///   child executes a program (a readable program image makes the child dumpable
///   again), so a child that must stay closed executes an image nobody can read.
/// - macOS: nothing more. What keeps another process out of a release image there is
///   its code signature (the hardened runtime, no `get-task-allow`), which the
///   consumer checks; an unsigned development image is not protected.
/// - Every other Unix target: the `process` refusal; nothing is changed.
///
/// An error is the system's, naming the call that failed: the caller does not go on to
/// read a secret.
pub fn harden_self() -> io::Result<()> {
    native::harden_self()
}

/// The parent PID of `pid`, or `None` if the process no longer exists.
/// Fails closed with the `process` refusal on targets without an adapter.
pub fn parent_pid(pid: libc::pid_t) -> io::Result<Option<libc::pid_t>> {
    if pid <= 0 {
        return Ok(None);
    }
    native::parent_pid(pid)
}

/// Whether `pid` is `ancestor` or a live descendant of `ancestor` by walking parent PIDs
/// through the kernel (`proc_pidinfo` on macOS, `/proc/<pid>/stat` on Linux).
/// Probes `pid` first so dead PIDs return `Ok(false)` and targets without an adapter fail closed.
pub fn is_descendant_of(pid: libc::pid_t, ancestor: libc::pid_t) -> io::Result<bool> {
    if pid <= 0 || ancestor <= 0 {
        return Ok(false);
    }
    let Some(first_ppid) = native::parent_pid(pid)? else {
        return Ok(false);
    };
    if pid == ancestor {
        return Ok(true);
    }
    const MAX_DEPTH: usize = 128;
    let mut current = pid;
    let mut ppid = first_ppid;
    for _ in 0..MAX_DEPTH {
        if ppid == ancestor {
            return Ok(true);
        }
        if ppid <= 1 || ppid == current {
            return Ok(false);
        }
        current = ppid;
        let Some(next) = native::parent_pid(current)? else {
            return Ok(false);
        };
        ppid = next;
    }
    Ok(false)
}

/// Whether [`harden_self`] holds for this process now: core dumps are disabled, and
/// where the system has such an attribute the process is not dumpable. For a
/// consumer's test of its own start path (`test-support`).
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

/// `run_before_main!(unsafe NAME = function)`: a static `NAME` that has the system's
/// loader call `function` (an `unsafe extern "C" fn()`) before `main`, and so before a
/// test harness starts any thread (`test-support`). macOS: the `__DATA,__mod_init_func`
/// section; GNU/Linux: `.init_array`. A target without an adapter does not build it.
///
/// It is unsafe, and the call says so: `function` runs before the Rust runtime is set
/// up, so it may use only what needs none (raw C library calls), and must not unwind.
#[cfg(any(test, feature = "test-support"))]
pub use native::run_before_main;

/// Whether a signal to a whole process group whose members have all exited, but are
/// not yet reaped, is refused (macOS: `EPERM`, which [`is_exited_group_denial`]
/// recognises), rather than delivered or answered `ESRCH` (GNU/Linux).
pub const EXITED_GROUP_SIGNAL_IS_DENIED: bool = native::EXITED_GROUP_SIGNAL_IS_DENIED;

/// Whether [`spawn_piped`] starts the child suspended, until [`release_spawned`]
/// continues it (macOS), rather than running (GNU/Linux).
pub const SPAWNS_SUSPENDED: bool = native::SPAWNS_SUSPENDED;

// What a [`RecoveryScan`] meets, one fact per behaviour a consumer's tests depend on.
// On today's systems the first three hold together on GNU/Linux and the last two on
// macOS, because each follows from how that system lets one process read another; each
// is still stated by itself, so that a test names the behaviour it needs and a later
// adapter can differ in one without the others.

/// Whether a [`RecoveryScan`] holds one descriptor for each process it observes until
/// the next snapshot (GNU/Linux: a pidfd pins each process), so a scan of many
/// processes is bounded by `RLIMIT_NOFILE` and its error names that limit.
pub const SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS: bool = native::SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS;

/// Whether the system's read of a process's environment, which a [`RecoveryScan`]
/// makes, returns the part before a page the process made unreadable (GNU/Linux:
/// `/proc/<pid>/environ`, copied from the process's memory between the addresses in
/// fields 50 and 51 of proc(5)'s `/proc/<pid>/stat`), rather than failing as a whole
/// (macOS: `KERN_PROCARGS2`). Such a prefix can end at a complete entry, so the scan
/// must tell it from a whole environment. On every system the scan reports that
/// environment as unreadable.
pub const SCAN_ENVIRONMENT_READ_STOPS_AT_AN_UNREADABLE_PAGE: bool =
    native::SCAN_ENVIRONMENT_READ_STOPS_AT_AN_UNREADABLE_PAGE;

/// Whether a process whose main thread has ended while another thread runs is a zombie
/// leader to a [`RecoveryScan`] (GNU/Linux: state `Z` in `/proc/<pid>/stat`, while the
/// process lives on). The scan must not take such a leader for an ended process.
pub const AN_ENDED_MAIN_THREAD_LEAVES_A_ZOMBIE_LEADER: bool =
    native::AN_ENDED_MAIN_THREAD_LEAVES_A_ZOMBIE_LEADER;

/// Whether the environment a [`RecoveryScan`] reads is what follows the arguments
/// counted when the process started (macOS: `KERN_PROCARGS2`), so a process that
/// rewrote its arguments can move text into what the scan takes for its environment,
/// and an empty `argv[0]` makes the layout ambiguous.
pub const SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS: bool =
    native::SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS;

/// Whether the system withholds a restricted process's environment from a
/// [`RecoveryScan`] (macOS: `CS_RESTRICT`, as for its platform programs such as
/// `/bin/sleep`), so the scan reports its mark unreadable.
pub const SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS: bool =
    native::SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS;

fn check(rc: libc::c_int) -> io::Result<()> {
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(rc))
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn unsupported(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

/// Reads the child's PID and its exit status or signal from a `siginfo_t`.
///
/// # Safety
///
/// The record was written by a successful `waitid`.
type ExitFields = unsafe fn(&libc::siginfo_t) -> (libc::pid_t, libc::c_int);

/// The user IDs of this process, as far as the system reports them.
struct Uids {
    real: libc::uid_t,
    effective: libc::uid_t,
    saved: libc::uid_t,
    /// The process is, or was, running with changed IDs (a set-id image).
    tainted: bool,
}

/// Whether this process may supervise children: not root by any user ID and not a
/// set-id image, `SIGCHLD` left so that a child stays unreaped until its owner reaps
/// it, core dumps disabled, and a system version the adapter supports.
///
/// It changes nothing. Call it before preparing or starting a child. Opening a
/// [`RecoveryScan`] calls it.
pub fn validate() -> io::Result<()> {
    let uids = native::uids()?;
    // Actual root/set-id integration needs a privileged harness; the complete
    // credential-policy combinations are tested without changing process IDs.
    if privileged_credentials(uids.real, uids.effective, uids.saved, uids.tainted) {
        return Err(unsupported("refusing root process supervision"));
    }
    // SAFETY: querying the signal disposition, without changing it.
    unsafe {
        let mut action = std::mem::zeroed::<libc::sigaction>();
        if libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) != 0 {
            return Err(io::Error::last_os_error());
        }
        if action.sa_sigaction == libc::SIG_IGN || action.sa_flags & libc::SA_NOCLDWAIT != 0 {
            return Err(unsupported(
                "SIGCHLD must preserve unreaped child ownership",
            ));
        }
    }
    check_core_limit(core_limit())?;
    native::check_version()
}

fn core_limit() -> io::Result<libc::rlimit> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: querying a process limit with a correctly sized output slot.
    if unsafe { libc::getrlimit(libc::RLIMIT_CORE, limit.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful getrlimit initialized the complete record.
    Ok(unsafe { limit.assume_init() })
}

// This policy seam covers nonzero values even when the test harness already inherited
// hard0, which an unprivileged fixture cannot raise again. Query errors fail closed too.
fn check_core_limit(limit: io::Result<libc::rlimit>) -> io::Result<()> {
    let limit = limit?;
    if limit.rlim_cur != 0 || limit.rlim_max != 0 {
        return Err(unsupported("RLIMIT_CORE soft and hard limits must be 0; disable core dumps at host/server startup before supervising children"));
    }
    Ok(())
}

fn privileged_credentials(
    real: libc::uid_t,
    effective: libc::uid_t,
    saved: libc::uid_t,
    tainted: bool,
) -> bool {
    real == 0 || effective == 0 || saved == 0 || tainted
}

/// Exit notification for one process: a kqueue registration on macOS, a pidfd on
/// Linux. It never reaps and never signals.
pub struct Watch(native::Watch);

impl Watch {
    /// Start watching `pid`. For the caller's own unreaped child the PID cannot have
    /// been reused. A child that has already exited is not an error: the caller's
    /// non-reaping exit check is authoritative before it waits.
    pub fn new(pid: libc::pid_t) -> io::Result<Self> {
        native::Watch::open(pid).map(Self)
    }

    /// Block until the process exits or the monotonic deadline passes, whichever is
    /// first. Returning says nothing about which: the caller checks the exit itself.
    pub fn wait(&self, deadline: Instant) -> io::Result<()> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let rc = self.0.poll(remaining);
            if rc >= 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}

/// The process group, the start identity and the boot identity of `pid`, in that
/// order. The start and boot identities are opaque values of this system: compare
/// them, never decode them. A numeric PID alone identifies nothing after a restart.
pub fn identity(pid: libc::pid_t) -> io::Result<(u32, String, String)> {
    native::identity(pid)
}

/// One non-blocking look at whether the caller's own child `pid` has exited. It never
/// reaps: the zombie and its PID stay reserved for the owner (`WNOWAIT`).
///
/// `None` means still running, or stopped. An interrupted call and `ECHILD` (some
/// other code reaped the child) are returned as errors for the owner to handle.
pub fn try_exit(pid: libc::pid_t) -> io::Result<Option<ExitStatus>> {
    let exit_fields = native::exit_fields()?;
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: only the caller's own child's pid, valid siginfo slot. WNOWAIT is mandatory.
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as _,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: waitid initialized the zeroed siginfo slot.
    let info = unsafe { info.assume_init() };
    // SAFETY: waitid returns the SIGCHLD variant of this record.
    let (child, status) = unsafe { exit_fields(&info) };
    // Darwin reports START_SUSPENDED as CLD_STOPPED even with WEXITED.
    // Only terminal codes are exits; a stop must still be resumed/reaped.
    let exited = matches!(
        info.si_code,
        libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
    );
    Ok((child != 0 && exited).then(|| {
        ExitStatus::from_raw(if info.si_code == libc::CLD_EXITED {
            status << 8
        } else {
            (status & 0x7f)
                | if info.si_code == libc::CLD_DUMPED {
                    0x80
                } else {
                    0
                }
        })
    }))
}

/// Let a child started by [`spawn_piped`] run, once the caller has recorded its
/// identity. macOS starts the child suspended and continues it here unless `exited`
/// reports that it is already gone; Linux starts it running, so nothing is done and
/// `exited` is not called.
///
/// `pid` must be the caller's own unreaped child, so that the PID cannot name another
/// process. This orders startup only: another process of the same user can also
/// continue a suspended child.
pub fn release_spawned(
    pid: libc::pid_t,
    exited: impl FnOnce() -> io::Result<bool>,
) -> io::Result<()> {
    native::release_spawned(pid, exited)
}

/// Whether `error`, returned by a signal to a whole process group, is how this system
/// answers when no live member is left to signal. macOS answers `EPERM` when only
/// exited, unreaped members remain. Linux signals such a group successfully, so no
/// error means that there.
///
/// `true` proves nothing by itself: the caller must still verify, with
/// [`group_contains_only_leader`] and its own exit checks, that the group holds only
/// its exited leader.
pub fn is_exited_group_denial(error: &io::Error) -> bool {
    native::is_exited_group_denial(error)
}

/// Whether the process group led by the caller's exited, unreaped child `pid` holds
/// that leader and nothing else. An incomplete, empty or failed enumeration is an
/// error, never an empty group.
///
/// Only meaningful after [`is_exited_group_denial`]. Where that is never true (Linux)
/// this reports `false`: the group is not proven empty.
pub fn group_contains_only_leader(pid: libc::pid_t) -> io::Result<bool> {
    native::group_contains_only_leader(pid)
}

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let original = native::pipe()?;
    let duplicate = |fd: OwnedFd| -> io::Result<OwnedFd> {
        if native::PIPE_IS_CLOEXEC && fd.as_raw_fd() >= 3 {
            // Already CLOEXEC from pipe2; transfer it without another duplication.
            return Ok(fd);
        }
        // SAFETY: valid owned descriptor. Keep spawn source fds above2 even if stdio is closed.
        let new = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if new < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful duplication returns a new owned descriptor.
        Ok(unsafe { OwnedFd::from_raw_fd(new) })
    };
    let [read, write] = original;
    Ok((duplicate(read)?, duplicate(write)?))
}

/// Start `program` as the leader of a new session, with pipes for its standard input,
/// output and error, and return its PID with the parent's ends of those pipes, in
/// that order.
///
/// - `argv` is the whole argument vector, `argv[0]` included; `env` is the whole
///   environment, one `NAME=value` string each. Nothing is taken from this process's
///   own environment and no shell is involved.
/// - The child starts in `cwd`, with every signal unblocked and at its default
///   action, and with no descriptor of this process other than the three pipes.
/// - The parent's ends are close-on-exec and numbered above 2. The write end of
///   standard input is non-blocking.
/// - macOS starts the child suspended: call [`release_spawned`] once its identity is
///   recorded. macOS has no `pipe2`, so until the new pipes are marked close-on-exec
///   another thread that starts a process without closing descriptors can leak them.
///
/// The caller owns the child from the moment this returns: it must watch, signal and
/// reap it.
pub fn spawn_piped(
    program: &CStr,
    argv: &[CString],
    env: &[CString],
    cwd: &CStr,
) -> io::Result<(libc::pid_t, File, File, File)> {
    let pointers = |strings: &[CString]| {
        let mut result: Vec<_> = strings.iter().map(|s| s.as_ptr().cast_mut()).collect();
        result.push(std::ptr::null_mut());
        result
    };
    let argv = pointers(argv);
    let envp = pointers(env);
    let (input_child, input) = pipe()?;
    // SAFETY: our pipe's write end; O_NONBLOCK prevents a writer holding the stdin mutex
    // from blocking close/deadline handling when the child stops reading.
    if unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let (output, output_child) = pipe()?;
    let (error, error_child) = pipe()?;
    struct FileActions(libc::posix_spawn_file_actions_t);
    impl Drop for FileActions {
        fn drop(&mut self) {
            // SAFETY: `FileActions` is constructed only after `posix_spawn_file_actions_init`
            // succeeded and owns this object exclusively until drop.
            unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0) };
        }
    }
    struct SpawnAttrs(libc::posix_spawnattr_t);
    impl Drop for SpawnAttrs {
        fn drop(&mut self) {
            // SAFETY: `SpawnAttrs` is constructed only after `posix_spawnattr_init`
            // succeeded and owns this object exclusively until drop.
            unsafe { libc::posix_spawnattr_destroy(&mut self.0) };
        }
    }
    let mut raw_actions = std::mem::MaybeUninit::<libc::posix_spawn_file_actions_t>::zeroed();
    // SAFETY: valid init slot; ownership transfers to `FileActions` immediately on success.
    check(unsafe { libc::posix_spawn_file_actions_init(raw_actions.as_mut_ptr()) })?;
    // SAFETY: `posix_spawn_file_actions_init` succeeded; `FileActions` destroys it on every exit,
    // including unwinding.
    let mut actions = FileActions(unsafe { raw_actions.assume_init() });
    let mut raw_attrs = std::mem::MaybeUninit::<libc::posix_spawnattr_t>::zeroed();
    // SAFETY: valid init slot; ownership transfers to `SpawnAttrs` immediately on success.
    check(unsafe { libc::posix_spawnattr_init(raw_attrs.as_mut_ptr()) })?;
    // SAFETY: `posix_spawnattr_init` succeeded; `SpawnAttrs` destroys it on every exit,
    // including unwinding.
    let mut attrs = SpawnAttrs(unsafe { raw_attrs.assume_init() });
    // SAFETY: `actions` and `attrs` are live initialized objects owned by their Drop guards;
    // all C strings, descriptors, sigsets, and pointer arrays outlive `posix_spawn`.
    let pid = unsafe {
        let actions = &mut actions.0 as *mut libc::posix_spawn_file_actions_t;
        let attrs = &mut attrs.0 as *mut libc::posix_spawnattr_t;
        for (source, target) in [
            (input_child.as_raw_fd(), 0),
            (output_child.as_raw_fd(), 1),
            (error_child.as_raw_fd(), 2),
        ] {
            check(libc::posix_spawn_file_actions_adddup2(
                actions, source, target,
            ))?;
        }
        native::add_chdir(actions, cwd)?;
        let mut mask = std::mem::zeroed();
        let mut defaults = std::mem::zeroed();
        libc::sigemptyset(&mut mask);
        libc::sigfillset(&mut defaults);
        libc::sigdelset(&mut defaults, libc::SIGKILL);
        libc::sigdelset(&mut defaults, libc::SIGSTOP);
        check(libc::posix_spawnattr_setsigmask(attrs, &mask))?;
        check(libc::posix_spawnattr_setsigdefault(attrs, &defaults))?;
        let flags = native::session_flags(actions)?;
        check(libc::posix_spawnattr_setflags(attrs, flags as _))?;
        let mut pid = 0;
        check(libc::posix_spawn(
            &mut pid,
            program.as_ptr(),
            actions,
            attrs,
            argv.as_ptr(),
            envp.as_ptr(),
        ))?;
        pid
    };
    Ok((pid, input.into(), output.into(), error.into()))
}

/// Fill `bytes` from the kernel's random source, without blocking. An entropy pool
/// that is not ready is an error, not a wait.
pub fn entropy(bytes: &mut [u8; 32]) -> io::Result<()> {
    native::entropy(bytes)
}

/// Immutable kernel identity of a process, rather than a reusable numeric PID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    /// Numeric PID.
    pub pid: u32,
    /// Opaque start identity of this system.
    pub started: String,
    /// Opaque boot identity.
    pub boot: String,
}

/// The process that runs a [`RecoveryScan`], and the boot it runs in.
#[derive(Clone, Debug)]
pub struct RecoveryContext {
    /// Real user ID of the scanning process. The scan reports only this user's
    /// processes.
    pub uid: u32,
    /// PID of the scanning process. It is never reported and never a signal target.
    pub pid: u32,
    /// Current boot identity.
    pub boot: String,
}

/// What a process's environment says about the mark variable the scan was opened
/// for. The rest of the environment is never kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkEvidence {
    /// The environment was read completely and gives the variable this one value.
    /// The scan does not judge the value: the consumer decides what a valid mark is.
    Present(String),
    /// The environment was read completely and does not set the variable.
    Absent,
    /// The environment could not be read completely and unchanged, or it sets the
    /// variable to conflicting values or to one that is not UTF-8.
    Unreadable,
}

/// One process in a complete snapshot of the user's processes.
#[derive(Clone, Debug)]
pub struct ProcessObservation {
    /// Numeric PID, also available when the rest of its identity is unreadable.
    pub pid: u32,
    /// Full start/boot identity, or `None` if it could not be read.
    pub identity: Option<ProcessIdentity>,
    /// The parent's exact identity from the same capture, or `None` if the link could
    /// not be verified. A numeric PPID alone is not ancestry evidence.
    pub parent: Option<ProcessIdentity>,
    /// Process group; a hint only.
    pub group: u32,
    /// Session ID; a hint only.
    pub session: u32,
    /// Real user ID.
    pub uid: u32,
    /// True only if the real, effective and saved credentials are the same non-root
    /// user, with no set-id transition.
    pub credentials_safe: bool,
    /// Exited and not yet reaped. Its environment is not read.
    pub zombie: bool,
    /// Mark-only environment evidence.
    pub mark: MarkEvidence,
}

/// A fresh look at one process of the current snapshot, for a consumer that proves
/// membership in a recorded session or group.
#[derive(Clone, Debug)]
pub struct JobObservation {
    /// The identity that was observed.
    pub identity: ProcessIdentity,
    /// Start order of this system: a process started later compares greater or equal.
    pub order: (u64, u64),
    /// Process group.
    pub group: u32,
    /// Session ID.
    pub session: u32,
    /// Live, same-user credentials with no set-id transition, read around the
    /// observation.
    pub safe: bool,
}

/// An individual signal of a [`RecoveryScan`]. There is deliberately no group signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoverySignal {
    /// `SIGTERM`.
    Terminate,
    /// `SIGKILL`.
    Kill,
}

/// A checked signal either reached that identity or found it already gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalOutcome {
    /// Signal submitted to the validated identity, not proof of exit.
    Signalled,
    /// That precise identity no longer exists; a replacement was not signalled.
    Gone,
}

/// The source of the [`io::ErrorKind::TimedOut`] error a [`RecoveryScan`] returns
/// when its absolute deadline has passed. A consumer classifies the failure by this
/// type, never by the message.
#[derive(Debug)]
pub struct DeadlineElapsed(&'static str);

impl fmt::Display for DeadlineElapsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for DeadlineElapsed {}

/// `NAME=` for the environment variable `name`.
fn mark_prefix(name: &str) -> io::Result<Vec<u8>> {
    if name.is_empty() || name.bytes().any(|byte| byte == b'=' || byte == 0) {
        return Err(invalid("invalid mark variable name"));
    }
    let mut prefix = name.as_bytes().to_vec();
    prefix.push(b'=');
    Ok(prefix)
}

/// The native recovery scan: the user's processes as the kernel reports them, with
/// individual signals that are checked against the identity that was observed.
///
/// The consumer decides which processes to signal and why. The scan supplies the
/// observations and refuses to signal anything outside its current snapshot.
///
/// - **Linux** needs pidfds and an unfiltered `/proc` of its own PID namespace. A
///   snapshot keeps a pidfd for each live process it reports; a signal goes through
///   that descriptor after the process is checked again, so it cannot reach a
///   recycled PID. `ENOSYS` or `EPERM` from the pidfd calls is a refusal: there is no
///   fallback to `kill(pid)`.
/// - **macOS** checks the target's start identity and credentials again immediately
///   before an individual `kill`. For a process that is not the caller's child, the
///   PID can still be recycled between that check and the signal.
///
/// No operation reaps a process or signals a process group. Every operation honours
/// its absolute deadline, including across interrupted system calls, and reports a
/// passed deadline as a [`DeadlineElapsed`] error.
pub struct RecoveryScan(native::RecoveryScan);

impl RecoveryScan {
    /// Open the scan for the environment variable `mark_variable`, whose value marks
    /// the processes the consumer started. Runs [`validate`] first, then checks what
    /// the adapter needs (Linux: `/proc` is the procfs of this PID namespace, mounted
    /// unfiltered, and the pidfd calls are permitted).
    pub fn new(mark_variable: &str) -> io::Result<Self> {
        native::RecoveryScan::new(mark_prefix(mark_variable)?).map(Self)
    }

    /// The scanning process and the current boot.
    pub fn context(&self) -> &RecoveryContext {
        self.0.context()
    }

    /// A complete snapshot of the scanning user's processes, without the scanning
    /// process itself. It replaces the previous snapshot: later signals and job
    /// observations refer to this one.
    ///
    /// A parent link carries the exact parent identity from this capture. A truncated
    /// or failed enumeration is an error, never an empty successful scan. A process
    /// whose identity cannot be read is still reported, with `identity: None`.
    pub fn snapshot(&mut self, deadline: Instant) -> io::Result<Vec<ProcessObservation>> {
        self.0.snapshot(deadline)
    }

    /// Observe one process of the current snapshot again: its group, session, start
    /// order and whether its credentials are still safe. `None` if that identity is
    /// not in the snapshot or is gone.
    pub fn job_observation(
        &self,
        identity: &ProcessIdentity,
        deadline: Instant,
    ) -> io::Result<Option<JobObservation>> {
        self.0.job_observation(identity, deadline)
    }

    /// Signal one process of the current snapshot, after checking that it is still
    /// the same process with safe credentials and that `mark_accepted` accepts the
    /// mark evidence read at that moment. A target outside the current snapshot, an
    /// identity that changed without the process being gone, unsafe credentials and
    /// a refused mark are errors; nothing is signalled then.
    pub fn signal_checked(
        &mut self,
        target: &ProcessIdentity,
        mark_accepted: &dyn Fn(&MarkEvidence) -> bool,
        signal: RecoverySignal,
        deadline: Instant,
    ) -> io::Result<SignalOutcome> {
        self.0
            .signal_checked(target, mark_accepted, signal, deadline)
    }

    /// Wait without reaping until this identity is gone or the deadline is reached.
    /// A reused PID means the old identity is gone. A deadline that has already
    /// passed asks for one non-blocking check.
    pub fn wait_exited(
        &mut self,
        identity: &ProcessIdentity,
        deadline: Instant,
    ) -> io::Result<bool> {
        self.0.wait_exited(identity, deadline)
    }
}
