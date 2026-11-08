//! The descriptor swap the launch contract requires (`org-office-launch`, "Descriptors"): the
//! host's pipe is on this process's stdin and stdout, and no child of the server may read the
//! host's lines or write a control line. So, before any thread or child exists, the pipe moves
//! to two new close-on-exec descriptors and fds 0 and 1 are reopened on `/dev/null`; a child
//! then gets `/dev/null` unless it is given its own pipes. fd 2 (the logs) stays.
//!
//! The only unsafe code in the server, kept to this file.
#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::os::fd::{FromRawFd, RawFd};

/// The host's pipe: lines from the host, and the server's control lines to it.
pub struct ControlPipe {
    pub from_host: File,
    pub to_host: File,
}

/// Moves stdin and stdout to close-on-exec descriptors and points fds 0 and 1 at `/dev/null`.
/// Call it first in `main`.
pub fn take_control_pipe() -> io::Result<ControlPipe> {
    let from_host = dup_cloexec(0)?;
    let to_host = dup_cloexec(1)?;
    // SAFETY: plain libc calls on descriptors this function owns; nothing else is running yet.
    unsafe {
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
        if null < 0 {
            return Err(io::Error::last_os_error());
        }
        // `dup2` clears close-on-exec on the target: fds 0 and 1 are what a child inherits.
        if libc::dup2(null, 0) < 0 || libc::dup2(null, 1) < 0 {
            return Err(io::Error::last_os_error());
        }
        if null > 2 {
            libc::close(null);
        }
        // Files a server makes are owner-only whatever a caller forgets.
        libc::umask(0o077);
        Ok(ControlPipe {
            // SAFETY: both descriptors were just made by `dup_cloexec` and belong to us.
            from_host: File::from_raw_fd(from_host),
            to_host: File::from_raw_fd(to_host),
        })
    }
}

/// A copy of `fd` at the lowest free descriptor above 2, close-on-exec from the start.
fn dup_cloexec(fd: RawFd) -> io::Result<RawFd> {
    // SAFETY: `fcntl` with F_DUPFD_CLOEXEC only reads `fd` and returns a new descriptor.
    let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if copy < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(copy)
    }
}

/// Whether `fd` is close-on-exec (for tests and checks).
pub fn is_cloexec(fd: RawFd) -> bool {
    // SAFETY: F_GETFD only reads the descriptor's flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    flags >= 0 && flags & libc::FD_CLOEXEC != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_is_above_2_and_close_on_exec_from_the_start() {
        // fd 2 (stderr) exists in every test process.
        let copy = dup_cloexec(2).unwrap();
        assert!(copy >= 3, "{copy}");
        assert!(is_cloexec(copy));
        assert!(!is_cloexec(2), "stderr must stay inheritable");
        // SAFETY: closing the descriptor this test just made.
        unsafe { libc::close(copy) };
    }
}
