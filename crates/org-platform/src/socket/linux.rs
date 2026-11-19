//! The GNU/Linux socket adapter: `SO_PEERCRED`, `MSG_CMSG_CLOEXEC`, `MSG_NOSIGNAL`
//! and `SO_ACCEPTCONN`.

#[cfg(test)]
mod tests;

#[cfg(not(test))]
use libc::getsockopt;
use std::io;
use std::marker::PhantomData;
use std::mem;
use std::os::fd::{OwnedFd, RawFd};
use std::sync::{Mutex, MutexGuard};
#[cfg(test)]
use tests::getsockopt;

/// A peer-address query on a stream whose peer has closed still succeeds, so no
/// peer-query error is accepted.
pub(super) const PEER_CLOSED_IS_EINVAL: bool = false;

/// `MSG_CMSG_CLOEXEC` makes the kernel set close-on-exec with the receive.
pub(super) const RECEIVED_CLOSE_ON_EXEC: super::ReceivedCloseOnExec =
    super::ReceivedCloseOnExec::WithTheReceive(libc::MSG_CMSG_CLOEXEC);

/// [`send_flags`] include `MSG_NOSIGNAL`.
pub(super) const SEND_SUPPRESSES_SIGPIPE: bool = true;

/// A full datagram peer answers `EAGAIN`: the send would block.
pub(super) const DATAGRAM_PEER_FULL: i32 = libc::EAGAIN;

/// A receive after the datagram peer closed only finds nothing to read.
pub(super) const DATAGRAM_PEER_CLOSE_MAY_RESET: bool = false;

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

/// Nothing is held: `MSG_CMSG_CLOEXEC` sets close-on-exec atomically with the
/// receive, so the fence is neither locked nor inspected. The marker is a guard, as
/// the macOS adapter holds one, so the type is `!Send` on every system alike.
pub(super) struct DescriptorReceive<'fence> {
    _fence: PhantomData<MutexGuard<'fence, ()>>,
}

pub(super) fn descriptor_receive(_fence: &Mutex<()>) -> io::Result<DescriptorReceive<'_>> {
    Ok(DescriptorReceive {
        _fence: PhantomData,
    })
}

impl DescriptorReceive<'_> {
    pub(super) fn flags(&self) -> libc::c_int {
        // RECEIVED_CLOSE_ON_EXEC states this flag.
        libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC
    }

    pub(super) fn close_on_exec(
        &self,
        _descriptors: &[OwnedFd],
        _set: impl FnMut(RawFd) -> io::Result<()>,
    ) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn send_flags() -> io::Result<libc::c_int> {
    Ok(libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL)
}

pub(super) fn listening(fd: RawFd) -> io::Result<bool> {
    let mut value: libc::c_int = 0;
    let mut length = mem::size_of_val(&value) as libc::socklen_t;
    // SAFETY: the initialized output and its length cover this integer socket option;
    // the kernel rejects a number that is not an open socket.
    let result = unsafe {
        getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ACCEPTCONN,
            (&mut value as *mut libc::c_int).cast(),
            &mut length,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(value != 0)
}

/// What `socket::testing` needs of this system (`test-support`).
#[cfg(any(test, feature = "test-support"))]
pub(super) mod testing {
    /// Linux has Unix seqpacket sockets.
    pub(in crate::socket) const SEQPACKET: bool = true;
    /// Linux makes no working listener of a stream that was connected.
    pub(in crate::socket) const CONNECTED_STREAM_CAN_LISTEN: bool = false;

    /// A Linux socket address has no length field.
    pub(in crate::socket) fn set_address_length(_address: &mut libc::sockaddr_un) {}

    pub(in crate::socket) fn seqpacket_pair(
    ) -> std::io::Result<Option<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)>> {
        use std::os::fd::{FromRawFd, OwnedFd};
        let mut fds = [-1; 2];
        // SAFETY: socketpair writes two descriptors into the live two-item array.
        let status = unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        };
        if status != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: socketpair succeeded; each new descriptor gets exactly one owner.
        Ok(Some(unsafe {
            (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))
        }))
    }
}
