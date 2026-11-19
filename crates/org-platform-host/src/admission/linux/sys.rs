//! The system calls the Linux admission adapter makes in the host itself, and the error
//! that names the one that failed.
//!
//! Design section 3.1, "Failing closed": a kernel or a container profile that refuses
//! one of these calls refuses the start, and the error names the call and carries the
//! system's error. Nothing here falls back to another call.

use std::ffi::CStr;
use std::fmt;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

/// A system call that failed: its name and the system's error.
#[derive(Debug)]
pub(super) struct CallFailed {
    /// The call, as its manual page names it.
    pub(super) call: &'static str,
    /// What the system answered.
    pub(super) error: io::Error,
}

impl CallFailed {
    /// The failed call inside `error`, if that is what it is.
    #[cfg(test)]
    pub(super) fn of(error: &io::Error) -> Option<&Self> {
        error.get_ref().and_then(|source| source.downcast_ref())
    }
}

impl fmt::Display for CallFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed: {}", self.call, self.error)
    }
}

impl std::error::Error for CallFailed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// `error` as the failure of `call`: an error of the same kind that names the call
/// and keeps the system's error as its source.
pub(super) fn named(call: &'static str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), CallFailed { call, error })
}

/// The failure of `call` with the system error number `errno`.
pub(super) fn failed(call: &'static str, errno: i32) -> io::Error {
    named(call, io::Error::from_raw_os_error(errno))
}

/// The failure of `call` that just returned an error to this thread.
pub(super) fn last(call: &'static str) -> io::Error {
    named(call, io::Error::last_os_error())
}

/// `MFD_EXEC` (`<linux/memfd.h>`, Linux 6.3): the memory file may be executed.
const MFD_EXEC: libc::c_uint = 0x0010;

/// The lowest number a descriptor may have when the child of a spawn reads it as a
/// source. The child places its pipes on 0 to 2, the image on 3 and its failure report
/// on 4, in that order, so a source below 5 could be overwritten before it is used.
pub(super) const FIRST_FREE: libc::c_int = 5;

/// Whether a refused `memfd_create` with `MFD_EXEC` is tried once more without it.
/// A kernel before 6.3 does not know the flag and answers `EINVAL`; there every
/// memory file is executable. Any other error is final: `EACCES` is
/// `vm.memfd_noexec = 2`, and a profile's `EPERM` or `ENOSYS` is a refusal.
pub(super) fn retry_without_exec(errno: i32) -> bool {
    errno == libc::EINVAL
}

/// A new, empty, sealable, close-on-exec memory file named `name`, which may be
/// executed.
pub(super) fn memory_file(name: &CStr) -> io::Result<OwnedFd> {
    let create = |flags: libc::c_uint| {
        // SAFETY: `name` is NUL-terminated and outlives the call, which keeps nothing.
        let fd = unsafe { libc::memfd_create(name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor that nothing else owns.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    };
    let flags = libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING;
    match create(flags | MFD_EXEC) {
        Err(error) if error.raw_os_error().is_some_and(retry_without_exec) => {
            create(flags).map_err(|error| named("memfd_create", error))
        }
        result => result.map_err(|error| named("memfd_create", error)),
    }
}

/// `fd` itself when its number is at least [`FIRST_FREE`], or else a close-on-exec
/// duplicate that is; the low one is closed.
pub(super) fn above_the_childs_descriptors(fd: OwnedFd) -> io::Result<OwnedFd> {
    if fd.as_raw_fd() >= FIRST_FREE {
        return Ok(fd);
    }
    // SAFETY: a valid descriptor; the call makes a new one and changes nothing else.
    let raised = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, FIRST_FREE) };
    if raised < 0 {
        return Err(last("fcntl(F_DUPFD_CLOEXEC)"));
    }
    // SAFETY: a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(raised) })
}

/// A pipe, both ends close-on-exec from their creation: (read end, write end).
pub(super) fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut ends = [-1; 2];
    // SAFETY: an out-array of two descriptors.
    if unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(last("pipe2"));
    }
    // SAFETY: two new descriptors that nothing else owns.
    Ok(unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) })
}

/// The status of the open file `fd`.
pub(super) fn status(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
    // SAFETY: an all-zero `stat` is a valid value for the call to overwrite.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor and a valid out-pointer.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(last("fstat"));
    }
    Ok(stat)
}

/// A pidfd for the process `pid` names now. The caller must know that `pid` still
/// names the process it means (its own unreaped child, or itself).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn pidfd_open(pid: libc::pid_t) -> io::Result<OwnedFd> {
    // SAFETY: plain integer arguments; the call returns a new descriptor or an error.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_pidfd_open,
            libc::c_long::from(pid),
            0 as libc::c_long,
        )
    };
    if fd < 0 {
        return Err(last("pidfd_open"));
    }
    // SAFETY: a new close-on-exec descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) })
}

/// Send `signal` to the process `pidfd` names, and to no other: a pidfd never names
/// a later process that got the same PID. Signal 0 sends nothing and only asks.
pub(super) fn pidfd_send_signal(pidfd: BorrowedFd<'_>, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: a valid pidfd, a signal number, no `siginfo` (the kernel fills in an
    // ordinary `kill`'s) and no flags.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            libc::c_long::from(pidfd.as_raw_fd()),
            libc::c_long::from(signal),
            std::ptr::null_mut::<libc::siginfo_t>(),
            0 as libc::c_long,
        )
    };
    if rc != 0 {
        return Err(last("pidfd_send_signal"));
    }
    Ok(())
}

/// Whether the process `pidfd` names has exited (reaped or not). Does not wait.
pub(super) fn exited(pidfd: BorrowedFd<'_>) -> io::Result<bool> {
    let mut slot = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: one valid poll slot and a zero timeout.
        let ready = unsafe { libc::poll(&mut slot, 1, 0) };
        if ready >= 0 {
            // A pidfd is readable from the moment its process has exited. Any other
            // event on it is not a running process either.
            return Ok(ready > 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(named("poll", error));
        }
    }
}

/// The credentials of the process that connected the other end of `socket`, as the
/// kernel recorded them at that moment (`SO_PEERCRED`).
pub(super) fn peer_credentials(socket: BorrowedFd<'_>) -> io::Result<libc::ucred> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: libc::uid_t::MAX,
        gid: libc::gid_t::MAX,
    };
    let size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let mut len = size;
    // SAFETY: a valid descriptor, and an out-value of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(last("getsockopt(SO_PEERCRED)"));
    }
    if len != size || credentials.pid <= 0 {
        // A socket that never had a peer process answers zeroes.
        return Err(io::Error::other("the socket names no peer process"));
    }
    Ok(credentials)
}

/// Closes this process's memory, and the descriptors it holds, to other processes of
/// the same user (`PR_SET_DUMPABLE` 0): they can no longer trace it, read or write
/// `/proc/<pid>/mem`, or open `/proc/<pid>/fd/<n>`. The setting is inherited by a
/// child made with `clone`, until that child executes an image.
///
/// The adapter depends on it twice. While the image is copied, the memory file is not
/// yet sealed, and an open by `/proc/<pid>/fd/<n>` could write to it. Between `clone`
/// and `execveat` a child is a copy of this process's memory, secrets included. So it
/// is set before the memory file is made and again before every start, and a process
/// that cannot set it gets no admission.
pub(super) fn close_to_other_processes() -> io::Result<()> {
    // SAFETY: `prctl` with scalar arguments only; it changes one attribute of this
    // process and keeps nothing.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(last("prctl(PR_SET_DUMPABLE)"));
    }
    // SAFETY: as above; the call only reads the attribute.
    match unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } {
        0 => Ok(()),
        value if value < 0 => Err(last("prctl(PR_GET_DUMPABLE)")),
        _ => Err(io::Error::other(
            "prctl(PR_SET_DUMPABLE): this process is still open to other processes",
        )),
    }
}

/// This process's effective user ID.
pub(super) fn effective_uid() -> libc::uid_t {
    // SAFETY: no arguments and no side effects; it cannot fail.
    unsafe { libc::geteuid() }
}
