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
