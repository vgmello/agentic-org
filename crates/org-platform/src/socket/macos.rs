//! The macOS socket adapter: `getpeereid`.

#[cfg(test)]
mod tests;

use std::io;
use std::os::fd::RawFd;

pub(super) fn peer_uid(fd: RawFd) -> io::Result<u32> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: both out-pointers are live and writable for the getpeereid call.
    let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    super::checked_uid(rc, uid, 0, 0)
}
