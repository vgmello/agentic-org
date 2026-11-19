//! Test support for consumers' socket tests (`test-support`): the sockets those tests
//! build whose construction or behaviour differs by system.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::native;

/// Whether a Unix stream socket that is already connected can still be bound and put
/// in the listening state (macOS), rather than being refused with `EINVAL` (Linux).
/// macOS then reports such a listener's peer address with `EINVAL`, as for a stream
/// whose peer has closed (`socket::listening` tells the two apart).
pub const CONNECTED_STREAM_CAN_LISTEN: bool = native::testing::CONNECTED_STREAM_CAN_LISTEN;

/// Whether this system has Unix seqpacket sockets (not macOS).
pub const SEQPACKET: bool = native::testing::SEQPACKET;

/// A connected pair of Unix seqpacket sockets, both close-on-exec, or `None` where the
/// system has no Unix seqpacket sockets (macOS).
pub fn seqpacket_pair() -> io::Result<Option<(OwnedFd, OwnedFd)>> {
    if !SEQPACKET {
        return Ok(None);
    }
    let mut fds = [-1; 2];
    // SAFETY: socketpair writes two descriptors into the live two-item array.
    if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: socketpair succeeded; each new descriptor gets exactly one owner.
    let pair = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in [&pair.0, &pair.1] {
        // SAFETY: F_SETFD sets a valid flag on a descriptor owned above.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(Some(pair))
}

/// Binds the Unix socket `fd` to `path`, with the address laid out as this system
/// expects it (macOS: with `sun_len`).
pub fn bind_path(fd: BorrowedFd<'_>, path: &Path) -> io::Result<()> {
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: an all-zero sockaddr_un is valid and holds the path's terminator.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path does not fit a Unix socket address",
        ));
    }
    address.sun_family = libc::AF_UNIX as _;
    native::testing::set_address_length(&mut address);
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as _;
    }
    // SAFETY: a live socket and an initialized address covered by the given length.
    let status = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
