//! The macOS socket adapter: `getpeereid`, the fenced `FD_CLOEXEC` after `recvmsg`,
//! and listener state from `proc_pidfdinfo`.

mod listening;
#[cfg(test)]
mod tests;

use std::io;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::sync::{Mutex, MutexGuard};

/// A peer-address query on a stream whose peer has closed fails with `EINVAL`.
pub(super) const PEER_CLOSED_IS_EINVAL: bool = true;

/// No receive flag sets close-on-exec: it is set under the caller's fence.
pub(super) const RECEIVED_CLOSE_ON_EXEC: super::ReceivedCloseOnExec =
    super::ReceivedCloseOnExec::UnderTheFence;

/// macOS has no `MSG_NOSIGNAL`.
pub(super) const SEND_SUPPRESSES_SIGPIPE: bool = false;

/// A full datagram peer answers `ENOBUFS`, even while `poll` reports `POLLOUT`.
pub(super) const DATAGRAM_PEER_FULL: i32 = libc::ENOBUFS;

/// A receive after the datagram peer closed has been seen to fail with `ECONNRESET`,
/// and also to find nothing to read.
pub(super) const DATAGRAM_PEER_CLOSE_MAY_RESET: bool = true;

/// Closing a Unix stream peer with unread buffered bytes yields EOF on macOS.
pub(super) const STREAM_PEER_CLOSE_WITH_UNREAD_RESETS: bool = false;

pub(super) fn peer_uid(fd: RawFd) -> io::Result<u32> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: both out-pointers are live and writable for the getpeereid call.
    let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    super::checked_uid(rc, uid, 0, 0)
}

pub(super) fn peer_pid(fd: RawFd) -> io::Result<libc::pid_t> {
    let mut pid: libc::pid_t = 0;
    let expected = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    let mut length = expected;
    // SAFETY: pid is live and writable for length bytes.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut length,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if length != expected || pid <= 0 {
        return Err(invalid("unexpected peer pid response"));
    }
    Ok(pid)
}

/// The caller's fence, held from before `recvmsg` through every `FD_CLOEXEC` update:
/// `recvmsg` installs descriptors without close-on-exec here.
pub(super) struct DescriptorReceive<'fence> {
    _fence: MutexGuard<'fence, ()>,
}

pub(super) fn descriptor_receive(fence: &Mutex<()>) -> io::Result<DescriptorReceive<'_>> {
    match fence.lock() {
        Ok(guard) => Ok(DescriptorReceive { _fence: guard }),
        Err(_) => Err(io::Error::other("descriptor fence is poisoned")),
    }
}

impl DescriptorReceive<'_> {
    pub(super) fn flags(&self) -> libc::c_int {
        libc::MSG_DONTWAIT
    }

    pub(super) fn close_on_exec(
        &self,
        descriptors: &[OwnedFd],
        mut set: impl FnMut(RawFd) -> io::Result<()>,
    ) -> io::Result<()> {
        for descriptor in descriptors {
            set(descriptor.as_raw_fd())?;
        }
        Ok(())
    }
}

/// macOS has no `MSG_NOSIGNAL`: the caller ignores `SIGPIPE` or sets `SO_NOSIGPIPE`.
pub(super) fn send_flags() -> io::Result<libc::c_int> {
    Ok(libc::MSG_DONTWAIT)
}

pub(super) fn listening(fd: RawFd) -> io::Result<bool> {
    listening::listening(fd)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// What `socket::testing` needs of this system (`test-support`).
#[cfg(any(test, feature = "test-support"))]
pub(super) mod testing {
    /// macOS has no Unix seqpacket sockets (`EPROTONOSUPPORT`).
    pub(in crate::socket) const SEQPACKET: bool = false;
    /// macOS makes a working listener of a stream whose peer has closed.
    pub(in crate::socket) const CONNECTED_STREAM_CAN_LISTEN: bool = true;

    /// A BSD socket address states its own length.
    pub(in crate::socket) fn set_address_length(address: &mut libc::sockaddr_un) {
        address.sun_len = std::mem::size_of_val(address) as u8;
    }

    /// No Unix seqpacket sockets here.
    pub(in crate::socket) fn seqpacket_pair(
    ) -> std::io::Result<Option<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)>> {
        Ok(None)
    }
}
