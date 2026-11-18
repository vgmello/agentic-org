//! Unix-socket peer credentials, descriptor-passing flags and socket shape.
//!
//! Linux host design section 4, `socket`. `org-socket` keeps the portable socket code
//! (the framing, the retry loops and the shape policy of an adopted descriptor) and
//! asks this module for each fact that differs by system:
//!
//! - [`peer_uid`]: who the kernel says is at the other end. macOS: `getpeereid`.
//!   Linux: `SO_PEERCRED`.
//!
//! Descriptors are passed as raw numbers. Every function here only queries the
//! descriptor: none closes it, keeps it or changes its flags, and a number that is
//! not an open descriptor is an error from the kernel.

use std::io;
use std::os::fd::RawFd;

adapters!();

/// The effective UID of the process at the other end of a connected Unix socket, as
/// the kernel recorded it. A failed or truncated query is an error; callers refuse
/// the peer.
pub fn peer_uid(fd: RawFd) -> io::Result<u32> {
    native::peer_uid(fd)
}

supported! {
    // Isolate syscall status/size decisions so a truncated credential can be tested hermetically.
    fn checked_uid(
        rc: libc::c_int,
        uid: u32,
        length: libc::socklen_t,
        expected: libc::socklen_t,
    ) -> io::Result<u32> {
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        if length != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected peer credential size",
            ));
        }
        Ok(uid)
    }
}

#[cfg(test)]
mod tests;
