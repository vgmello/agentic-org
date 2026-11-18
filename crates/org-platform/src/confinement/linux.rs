//! The GNU/Linux web confinement adapter (Linux host design, section 3.3; ADR-0017
//! decision 9): the entry preconditions, the connection to the rendezvous,
//! no-new-privileges, a Landlock domain that allows no path, and a seccomp filter
//! that kills the process on every system call outside a fixed list.
//!
//! Linux 6.2 or later with Landlock enabled, on x86-64 or aarch64. The crate links
//! the C library only, so Landlock and seccomp are raw system calls.

use std::fmt;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

pub(super) use super::shared::fill_entropy;
use super::Enforcement;
use crate::Refused;
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
    Landlock(landlock::Unavailable),
    /// The kernel cannot kill a process from a seccomp filter, or refuses the
    /// `seccomp` call, by its error number.
    Seccomp(i32),
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Architecture => write!(
                f,
                "no system call list for the {} architecture (x86_64 and aarch64 have one)",
                std::env::consts::ARCH
            ),
            Self::Landlock(reason) => reason.fmt(f),
            Self::Seccomp(errno) => write!(
                f,
                "seccomp failed with {}: this kernel or its profile has no seccomp \
                 filter that kills the process",
                errno_text(*errno)
            ),
        }
    }
}

impl Unavailable {
    /// The refusal as the contract's [`Refused`], whose text is fixed at build time:
    /// the capability, with the reason in brackets.
    fn refused(self) -> Refused {
        use landlock::Unavailable as Landlock;
        Refused::unsupported(match self {
            Self::Architecture => {
                "web process confinement (no system call list for this processor \
                 architecture; x86_64 and aarch64 have one)"
            }
            Self::Landlock(Landlock::Missing) => {
                "web process confinement (landlock_create_ruleset failed with ENOSYS: \
                 this kernel has no Landlock; Linux 6.2 or later with Landlock is required)"
            }
            Self::Landlock(Landlock::Disabled) => {
                "web process confinement (landlock_create_ruleset failed with EOPNOTSUPP: \
                 Landlock is built into this kernel but is not an enabled security module)"
            }
            Self::Landlock(Landlock::TooOld(_)) => {
                "web process confinement (this kernel's Landlock ABI is below 3; Linux 6.2 \
                 or later is required)"
            }
            Self::Landlock(Landlock::Failed(libc::EPERM)) => {
                "web process confinement (landlock_create_ruleset failed with EPERM, as \
                 under a profile that refuses it)"
            }
            Self::Landlock(Landlock::Failed(_)) => {
                "web process confinement (landlock_create_ruleset failed: the Landlock \
                 version query was refused)"
            }
            Self::Seccomp(libc::EPERM) => {
                "web process confinement (seccomp failed with EPERM, as under a profile \
                 that refuses it)"
            }
            Self::Seccomp(_) => {
                "web process confinement (seccomp failed: this kernel has no seccomp \
                 filter that kills the process)"
            }
        })
    }
}

/// The decision of [`supported`] over what the three queries answered. Pure.
fn decide(
    arch: Option<&'static syscalls::Arch>,
    landlock: Result<u32, landlock::Unavailable>,
    seccomp: Result<(), i32>,
) -> Result<(&'static syscalls::Arch, u32), Unavailable> {
    let arch = arch.ok_or(Unavailable::Architecture)?;
    let abi = landlock.map_err(Unavailable::Landlock)?;
    seccomp.map_err(Unavailable::Seccomp)?;
    Ok((arch, abi))
}

/// What this system offers, asked without confining anything: the architecture's
/// list, the Landlock version query and the seccomp action query.
fn available() -> Result<(&'static syscalls::Arch, u32), Unavailable> {
    decide(
        syscalls::native(),
        landlock::abi(),
        seccomp::kill_process_available(),
    )
}

pub(super) fn supported() -> Result<(), Refused> {
    available().map(drop).map_err(Unavailable::refused)
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

/// The design's six steps, in order. Any error leaves the caller without a stream:
/// the one connected in step 2 is dropped here. After step 4 a failure leaves the
/// process partly confined, and it still refuses.
pub(super) fn confine_web(rendezvous: &Path) -> io::Result<(UnixStream, Enforcement)> {
    let arch = syscalls::native().ok_or_else(|| refuse(Unavailable::Architecture))?;
    if !rendezvous.is_absolute() {
        return Err(refuse("the rendezvous path is not absolute"));
    }
    // 1. No core dump and no same-user tracing; then the preconditions.
    super::disable_core_dumps()?;
    prctl("prctl(PR_SET_DUMPABLE)", libc::PR_SET_DUMPABLE, 0)?;
    entry::check().map_err(refuse)?;
    // 2. The last new socket this process creates.
    let stream = UnixStream::connect(rendezvous)?;
    // 3.
    no_new_privileges()?;
    // 4. No path can be read, written, executed, created, renamed, linked or
    //    truncated; no TCP socket bound or connected; no abstract Unix socket or
    //    signal outside the process.
    let kernel = landlock::abi().map_err(refuse)?;
    let landlock_abi = landlock::restrict(kernel)?;
    // 5.
    seccomp::install(arch)?;
    // 6.
    Ok((
        stream,
        Enforcement {
            landlock_abi: Some(landlock_abi),
            seccomp: true,
        },
    ))
}

/// The report is the Landlock ABI alone: the ready line has no seccomp field, and
/// the host observes seccomp on the process itself in [`verify_web`].
pub(super) fn verify_ready_report(enforcement: &Enforcement) -> io::Result<()> {
    match enforcement.landlock_abi {
        Some(abi) if abi >= landlock::MINIMUM_ABI => Ok(()),
        Some(abi) => Err(refuse(format_args!(
            "the ready line reports Landlock ABI {abi}, below {}",
            landlock::MINIMUM_ABI
        ))),
        None => Err(refuse("the ready line reports no Landlock domain")),
    }
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
