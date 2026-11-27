//! The descriptor swap the launch contract requires (`org-office-launch`, "Descriptors"): the
//! host's pipe is on this process's stdin and stdout, and no child of the server may read the
//! host's lines or write a control line. So, before any thread or child exists, the pipe moves
//! to two new close-on-exec descriptors and fds 0 and 1 are reopened on `/dev/null`; a child
//! then gets `/dev/null` unless it is given its own pipes. fd 2 (the logs) stays.
//!
//! The calls are `org_platform::posix`'s; the server has no `unsafe` code.

use std::fs::File;
use std::io;
use std::os::fd::{OwnedFd, RawFd};

use org_platform::posix::{self, Standard};

/// The host's pipe: lines from the host, and the server's control lines to it.
pub struct ControlPipe {
    pub from_host: File,
    pub to_host: File,
}

/// Moves stdin and stdout to close-on-exec descriptors and points fds 0 and 1 at `/dev/null`.
/// Call it first in `main`, before any thread exists.
pub fn take_control_pipe() -> io::Result<ControlPipe> {
    let from_host = dup_cloexec(0)?;
    let to_host = dup_cloexec(1)?;
    // `dup2` clears close-on-exec on the target: fds 0 and 1 are what a child inherits.
    // Where a descriptor 0-2 was closed at launch, the null device keeps that number
    // inherited too.
    posix::reopen_standard_on_null(&[Standard::Input, Standard::Output])?;
    // Files a server makes are owner-only whatever a caller forgets.
    org_platform::process::owner_only_umask();
    Ok(ControlPipe {
        from_host: File::from(from_host),
        to_host: File::from(to_host),
    })
}

/// A copy of standard input (0) or output (1) at the lowest free descriptor above 2,
/// close-on-exec from the start.
fn dup_cloexec(fd: RawFd) -> io::Result<OwnedFd> {
    match fd {
        0 => posix::duplicate_above(std::io::stdin(), 3),
        1 => posix::duplicate_above(std::io::stdout(), 3),
        _ => posix::duplicate_above(std::io::stderr(), 3),
    }
}

/// Whether `fd` is close-on-exec (for tests and checks).
pub fn is_cloexec(fd: RawFd) -> bool {
    posix::is_close_on_exec(fd).unwrap_or(false)
}

#[cfg(test)]
mod tests;
