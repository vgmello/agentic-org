//! The GNU/Linux socket adapter: `SO_PEERCRED`, `MSG_CMSG_CLOEXEC` and `MSG_NOSIGNAL`.

#[cfg(test)]
mod tests;

use std::io;
use std::marker::PhantomData;
use std::os::fd::{OwnedFd, RawFd};
use std::sync::Mutex;

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
/// receive, so the fence is neither locked nor inspected.
pub(super) struct DescriptorReceive<'fence> {
    _fence: PhantomData<&'fence Mutex<()>>,
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

    // libc uses size_t on Linux.
    pub(super) fn control_len(&self, message: &libc::msghdr) -> usize {
        message.msg_controllen
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
