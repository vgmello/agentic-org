//! The macOS socket adapter: `getpeereid` and the fenced `FD_CLOEXEC` after `recvmsg`.

#[cfg(test)]
mod tests;

use std::io;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::sync::{Mutex, MutexGuard};

pub(super) fn peer_uid(fd: RawFd) -> io::Result<u32> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: both out-pointers are live and writable for the getpeereid call.
    let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    super::checked_uid(rc, uid, 0, 0)
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

    // libc uses socklen_t (u32) on macOS.
    pub(super) fn control_len(&self, message: &libc::msghdr) -> usize {
        message.msg_controllen as usize
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
