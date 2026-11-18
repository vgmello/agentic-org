//! The GNU/Linux socket adapter: `SO_PEERCRED`.

#[cfg(test)]
mod tests;

use std::io;
use std::os::fd::RawFd;

pub(super) fn peer_uid(fd: RawFd) -> io::Result<u32> {
    // SAFETY: every bit pattern is valid for ucred's integer fields.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let expected = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let mut length = expected;
    // SAFETY: cred is writable for length bytes; length points to its live size value.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    super::checked_uid(rc, cred.uid, length, expected)
}
