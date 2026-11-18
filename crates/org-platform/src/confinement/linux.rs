//! The GNU/Linux web confinement adapter (Linux host design, section 3.3; ADR-0017
//! decision 9): the entry preconditions, the connection to the rendezvous,
//! no-new-privileges, a Landlock domain that allows no path where the kernel offers
//! one, and a seccomp filter that kills the process on every system call outside a
//! fixed list.
//!
//! The filter is the layer that must hold, and it holds every guarantee by itself:
//! without it the adapter refuses. Landlock is a second layer under it. A kernel
//! without Landlock, one that has it turned off, or a container profile that refuses
//! its calls does not stop the web process: it is then confined by the filter alone
//! and says why. Nothing chooses this but what the kernel answers.
//!
//! x86-64 or aarch64. The crate links the C library only, so Landlock and seccomp
//! are raw system calls.

use std::fmt;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

pub(super) use super::shared::fill_entropy;
use super::{Confined, Enforcement, Layers};
#[cfg(any(test, feature = "test-support"))]
pub(super) use profile::refuse_call;

mod entry;
mod landlock;
#[cfg(any(test, feature = "test-support"))]
mod profile;
mod seccomp;
mod status;
mod syscalls;

#[cfg(test)]
mod tests;

/// The adapter's own refusal: the contract's fixed text, then the call and the cause.
/// It carries no OS error number, so it cannot be mistaken for a failed connection.
fn refuse(reason: impl fmt::Display) -> io::Error {
    io::Error::other(format!("{}: {reason}", super::REFUSAL))
}

/// An error number with its name, for the ones a kernel or a container profile
/// answers these calls with.
fn errno_text(errno: i32) -> String {
    let name = match errno {
        libc::EPERM => "EPERM",
        libc::EACCES => "EACCES",
        libc::ENOSYS => "ENOSYS",
        libc::EOPNOTSUPP => "EOPNOTSUPP",
        libc::EINVAL => "EINVAL",
        libc::E2BIG => "E2BIG",
        libc::ENOMSG => "ENOMSG",
        libc::EFAULT => "EFAULT",
        libc::ENOMEM => "ENOMEM",
        libc::EMFILE => "EMFILE",
        libc::ENFILE => "ENFILE",
        _ => return format!("error {errno}"),
    };
    name.to_owned()
}

/// The refusal for a system call that has just failed: its name and `errno`. Call it
/// before anything else can change `errno`.
fn failed_call(call: &str) -> io::Error {
    let errno = io::Error::last_os_error().raw_os_error().unwrap_or(0);
    refuse(format_args!(
        "{call} failed with {} ({})",
        errno_text(errno),
        io::Error::from_raw_os_error(errno)
    ))
}

/// Why the web process cannot be confined on this system, as far as a process can
/// tell without confining itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unavailable {
    /// The adapter has no system call list for this processor architecture.
    Architecture,
    /// The kernel cannot kill a process from a seccomp filter, or refuses the
    /// `seccomp` call, by its error number.
    Seccomp(i32),
    /// The kernel or a profile does not let this process install a filter on all its
    /// threads: what it answered instead of the expected `EFAULT`.
    Install(i32),
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Architecture => write!(
                f,
                "no system call list for the {} architecture (x86_64 and aarch64 have one)",
                std::env::consts::ARCH
            ),
            Self::Seccomp(errno) => write!(
                f,
                "seccomp failed with {}: this kernel or its profile has no seccomp \
                 filter that kills the process",
                errno_text(*errno)
            ),
            Self::Install(errno) => write!(
                f,
                "seccomp failed with {}: this kernel or its profile does not let a \
                 process install a seccomp filter on all its threads",
                errno_text(*errno)
            ),
        }
    }
}

/// The decision of [`supported`] over what its questions answered: the
/// architecture's list, whether a filter can kill the process, whether a filter can
/// be installed. Landlock is not asked: it is never a reason to refuse. Pure.
fn decide(
    arch: Option<&'static syscalls::Arch>,
    kill: Result<(), i32>,
    install: Result<(), i32>,
) -> Result<&'static syscalls::Arch, Unavailable> {
    let arch = arch.ok_or(Unavailable::Architecture)?;
    kill.map_err(Unavailable::Seccomp)?;
    install.map_err(Unavailable::Install)?;
    Ok(arch)
}

/// What this system offers of what the filter needs, asked without confining
/// anything: the architecture's list and the two seccomp questions.
fn available() -> Result<&'static syscalls::Arch, Unavailable> {
    decide(
        syscalls::native(),
        seccomp::kill_process_available(),
        seccomp::installable(),
    )
}

/// A refusal has the form of [`confine_web`]'s: the fixed text, then the reason.
pub(super) fn supported() -> io::Result<()> {
    available().map(drop).map_err(refuse)
}

/// `prctl` with one argument and the rest zero, as these options require.
fn prctl(name: &str, option: libc::c_int, value: libc::c_ulong) -> io::Result<()> {
    // SAFETY: the options this adapter passes take one integer and no pointer.
    if unsafe { libc::prctl(option, value, 0, 0, 0) } != 0 {
        return Err(failed_call(name));
    }
    Ok(())
}

/// Step 3: from here no `execve` can grant a privilege, which is what lets an
/// unprivileged process restrict itself.
fn no_new_privileges() -> io::Result<()> {
    prctl("prctl(PR_SET_NO_NEW_PRIVS)", libc::PR_SET_NO_NEW_PRIVS, 1)
}

/// The outcome of `mallopt(M_ARENA_MAX, 1)`, which returns 1 when it set the limit.
///
/// glibc gives each thread an allocator arena of its own, and the first time the
/// heap of one of those shrinks it opens `/proc/sys/vm/overcommit_memory`
/// (`malloc/arena.c`, `shrink_heap`). The filter kills that open, and the web
/// process's worker threads reach it under ordinary load. With the limit every
/// thread allocates from the main arena, which grows and shrinks with `brk` and
/// reads no file.
///
/// The limit holds only for a process that never had a second thread, which is what
/// the contract asks of the caller. An earlier thread, even one that has ended, may
/// have left a second arena behind; a later thread can take it over, and its first
/// shrink is then killed by the filter. That fails closed: the process dies, it is
/// never left unconfined.
///
/// The cost is one allocator lock for all threads. Each thread keeps its own cache
/// of small blocks.
fn one_arena(result: libc::c_int) -> io::Result<()> {
    match result {
        1 => Ok(()),
        _ => Err(refuse("mallopt(M_ARENA_MAX, 1) failed")),
    }
}

/// The design's six steps, in order, after the allocator limit of [`one_arena`].
/// Any error leaves the caller without a stream: the one connected in step 2 is
/// dropped here. Step 4, Landlock, never fails the call: where it cannot be applied
/// the process goes on without it and the reason is handed back. A failure of step
/// 5 leaves the process partly confined, and it still refuses.
pub(super) fn confine_web(rendezvous: &Path) -> io::Result<Confined> {
    let arch = syscalls::native().ok_or_else(|| refuse(Unavailable::Architecture))?;
    if !rendezvous.is_absolute() {
        return Err(refuse("the rendezvous path is not absolute"));
    }
    // Before anything is changed: one allocator arena. A refusal here leaves the
    // process as it was.
    // SAFETY: `mallopt` takes two integers and changes only the allocator's limit.
    one_arena(unsafe { libc::mallopt(libc::M_ARENA_MAX, 1) })?;
    // 1. No core dump and no same-user tracing; then the preconditions.
    super::disable_core_dumps()?;
    prctl("prctl(PR_SET_DUMPABLE)", libc::PR_SET_DUMPABLE, 0)?;
    entry::check().map_err(refuse)?;
    // 2. The last new socket this process creates.
    let stream = UnixStream::connect(rendezvous)?;
    // 3.
    no_new_privileges()?;
    // 4. Where the kernel has Landlock and lets this process use it, at the
    //    kernel's ABI: no path can be read, written, executed, created, renamed or
    //    linked; from ABI 3 none truncated; from ABI 4 no TCP socket bound or
    //    connected; from ABI 6 no abstract Unix socket or signal outside the
    //    process. Where it cannot be applied, for whatever reason, the filter below
    //    is the whole confinement, and it stops each of those by itself.
    let (landlock_abi, landlock_not_applied) = match landlock::apply() {
        Ok(abi) => (Some(abi), None),
        Err(reason) => (None, Some(reason.to_string())),
    };
    // 5. The layer that must hold.
    seccomp::install(arch)?;
    // 6.
    Ok(Confined {
        stream,
        enforcement: Enforcement {
            landlock_abi,
            seccomp: true,
        },
        landlock_not_applied,
    })
}

/// The report is the Landlock ABI where Landlock was applied: the ready line has no
/// seccomp field, and the host observes seccomp on the process itself in
/// [`verify_web`]. A report without Landlock is one of a process confined by the
/// filter alone, and is accepted.
pub(super) fn verify_ready_report(enforcement: &Enforcement) -> io::Result<()> {
    match enforcement.landlock_abi {
        Some(abi) if abi < landlock::MINIMUM_ABI => Err(refuse(format_args!(
            "the ready line reports Landlock ABI {abi}, which is no ABI"
        ))),
        Some(_) | None => Ok(()),
    }
}

/// What a process with this report is confined by, once [`verify_web`] has seen its
/// filter.
pub(super) fn layers(enforcement: &Enforcement) -> io::Result<Layers> {
    verify_ready_report(enforcement)?;
    Ok(match enforcement.landlock_abi {
        Some(abi) => Layers {
            description: format!("the seccomp filter and Landlock ABI {abi}"),
            landlock_missing: false,
        },
        None => Layers {
            description: "the seccomp filter alone".to_owned(),
            landlock_missing: true,
        },
    })
}

/// The report, then one read of `/proc/<pid>/status`. `critical` is not used: one
/// process cannot observe another's Landlock domain, so there is no path to probe.
pub(super) fn verify_web(
    pid: u32,
    enforcement: &Enforcement,
    _critical: &[&Path],
) -> io::Result<()> {
    verify_ready_report(enforcement)?;
    if pid == 0 || libc::pid_t::try_from(pid).is_err() {
        return Err(refuse("the web process has no valid process id"));
    }
    let read = |path: String| {
        std::fs::read_to_string(&path)
            .map_err(|error| refuse(format_args!("{path} cannot be read ({:?})", error.kind())))
    };
    let own = status::filters(&read("/proc/self/status".to_owned())?).map_err(refuse)?;
    status::confined(&read(format!("/proc/{pid}/status"))?, own).map_err(refuse)
}
