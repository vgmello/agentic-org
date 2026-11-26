//! This process's identity, signals to other processes, reaping, an immediate exit,
//! resource limits and the file mode creation mask.

use std::io;

use super::check;

/// The effective user ID of this process (`geteuid`; it cannot fail).
pub fn effective_uid() -> u32 {
    // SAFETY: geteuid takes no arguments, touches no memory and cannot fail.
    unsafe { libc::geteuid() }
}

/// The effective group ID of this process (`getegid`; it cannot fail).
pub fn effective_gid() -> u32 {
    // SAFETY: getegid takes no arguments, touches no memory and cannot fail.
    unsafe { libc::getegid() }
}

/// The real user ID of this process (`getuid`; it cannot fail).
pub fn real_uid() -> u32 {
    // SAFETY: getuid takes no arguments, touches no memory and cannot fail.
    unsafe { libc::getuid() }
}

/// The real group ID of this process (`getgid`; it cannot fail).
pub fn real_gid() -> u32 {
    // SAFETY: getgid takes no arguments, touches no memory and cannot fail.
    unsafe { libc::getgid() }
}

/// Send `signal` to the process `pid` (`kill`). Signal 0 sends nothing and only asks
/// whether the process exists and may be signalled. The caller decides which process: a
/// number it did not start may have been reused.
pub fn kill(pid: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: kill takes two integers and touches no memory of this process.
    check(unsafe { libc::kill(pid, signal) }).map(drop)
}

/// Whether a process `pid` exists that this process may signal (`kill` with signal 0).
pub fn process_exists(pid: libc::pid_t) -> bool {
    kill(pid, 0).is_ok()
}

/// Send `signal` to every process of the group `group` (`killpg`).
pub fn kill_group(group: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: killpg takes two integers and touches no memory of this process.
    check(unsafe { libc::killpg(group, signal) }).map(drop)
}

/// The process group of `pid` (`getpgid`).
pub fn process_group(pid: libc::pid_t) -> io::Result<libc::pid_t> {
    // SAFETY: getpgid takes one integer and touches no memory of this process.
    check(unsafe { libc::getpgid(pid) })
}

/// The session of `pid` (`getsid`; 0 is this process).
pub fn session_id(pid: libc::pid_t) -> io::Result<libc::pid_t> {
    // SAFETY: getsid takes one integer and touches no memory of this process.
    check(unsafe { libc::getsid(pid) })
}

/// Wait for a state change of the child `pid` (`waitpid` with `options`, such as
/// `WNOHANG` or `WUNTRACED`), and return its raw status (`libc::WIFEXITED` and the rest,
/// or `std::os::unix::process::ExitStatusExt::from_raw`). `Ok(None)`: with `WNOHANG`,
/// nothing has changed yet. An exited child is reaped unless the options say otherwise:
/// call it only for a child this process owns. An interrupted wait is
/// [`io::ErrorKind::Interrupted`].
pub fn wait_pid(pid: libc::pid_t, options: libc::c_int) -> io::Result<Option<libc::c_int>> {
    let mut status = 0;
    // SAFETY: `status` is a live, exclusively borrowed int that waitpid writes.
    let waited = check(unsafe { libc::waitpid(pid, &mut status, options) })?;
    Ok((waited != 0).then_some(status))
}

/// [`wait_pid`] without options, retried when interrupted: blocks until the child `pid`
/// ends and reaps it.
pub fn reap(pid: libc::pid_t) -> io::Result<libc::c_int> {
    loop {
        match wait_pid(pid, 0) {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

/// End this process now with `status` (`_exit`): no destructor, no `atexit` handler and
/// no flush of a standard-library buffer runs. For a child that must not run its
/// parent's cleanup, or a fatal failure that must not race other threads' teardown.
pub fn exit_immediately(status: libc::c_int) -> ! {
    // SAFETY: _exit ends the process and never returns; it takes a plain integer.
    unsafe { libc::_exit(status) }
}

/// A resource limit of this process ([`resource_limit`], [`set_resource_limit`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    /// `RLIMIT_CORE`: the largest core file.
    CoreFileSize,
    /// `RLIMIT_NOFILE`: one more than the largest descriptor number.
    OpenFiles,
    /// `RLIMIT_FSIZE`: the largest file the process may write.
    FileSize,
}

/// The soft and the hard limit (`rlim_cur`, `rlim_max`).
pub type Limit = libc::rlimit;

// The constants' type differs by C library; the match lets each system infer its own.
macro_rules! raw_resource {
    ($resource:expr) => {
        match $resource {
            Resource::CoreFileSize => libc::RLIMIT_CORE,
            Resource::OpenFiles => libc::RLIMIT_NOFILE,
            Resource::FileSize => libc::RLIMIT_FSIZE,
        }
    };
}

/// The soft and hard limit of `resource` for this process (`getrlimit`).
pub fn resource_limit(resource: Resource) -> io::Result<Limit> {
    let mut limit = Limit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a live, exclusively borrowed rlimit that getrlimit writes.
    check(unsafe { libc::getrlimit(raw_resource!(resource), &mut limit) })?;
    Ok(limit)
}

/// Set the soft and hard limit of `resource` for this process (`setrlimit`). Raising a
/// hard limit needs privilege.
pub fn set_resource_limit(resource: Resource, limit: Limit) -> io::Result<()> {
    // SAFETY: `limit` is a live rlimit that setrlimit only reads.
    check(unsafe { libc::setrlimit(raw_resource!(resource), &limit) }).map(drop)
}

/// The size of a memory page in bytes (`sysconf(_SC_PAGESIZE)`).
pub fn page_size() -> io::Result<usize> {
    configured(libc::_SC_PAGESIZE)
}

/// The most descriptors this process may have open (`sysconf(_SC_OPEN_MAX)`).
pub fn open_max() -> io::Result<usize> {
    configured(libc::_SC_OPEN_MAX)
}

fn configured(name: libc::c_int) -> io::Result<usize> {
    // sysconf answers -1 both for a failure, which sets errno, and for a value the system
    // does not fix, which leaves errno alone: clear errno first to tell them apart. Where
    // it cannot be cleared, a -1 is never reported with an errno that may be stale.
    let cleared = crate::fs::set_errno(0).is_ok();
    // SAFETY: sysconf takes one integer and touches no memory of this process.
    let value = unsafe { libc::sysconf(name) };
    if let Ok(value) = usize::try_from(value) {
        return Ok(value);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if cleared && code != 0 => Err(error),
        _ => Err(io::Error::other(format!(
            "sysconf({name}) has no determinate value"
        ))),
    }
}

/// Set this process's file mode creation mask to `mask` and return the one it replaces
/// (`umask`; it cannot fail). The mask is the whole process's: set it before threads
/// that create files start, or in a process of its own.
pub fn set_umask(mask: libc::mode_t) -> libc::mode_t {
    // SAFETY: umask takes a plain mode, touches no memory and cannot fail.
    unsafe { libc::umask(mask) }
}
