//! Unix-socket peer credentials, descriptor-passing flags and socket shape.
//!
//! Linux host design section 4, `socket`. `org-socket` keeps the portable socket code
//! (the framing, the retry loops and the shape policy of an adopted descriptor) and
//! asks this module for each fact that differs by system:
//!
//! - [`peer_uid`]: who the kernel says is at the other end. macOS: `getpeereid`.
//!   Linux: `SO_PEERCRED`.
//! - [`descriptor_receive`]: how a received descriptor becomes close-on-exec. Linux
//!   sets it atomically with `MSG_CMSG_CLOEXEC` and takes no lock. macOS cannot, so
//!   the caller's spawn fence is held from before `recvmsg` until every received
//!   descriptor has `FD_CLOEXEC`.
//! - [`send_flags`]: Linux adds `MSG_NOSIGNAL`; macOS has no such flag.
//! - [`listening`]: macOS reads the socket's options through `proc_pidfdinfo`, which
//!   also sees a socket that was connected before it listened; Linux reads
//!   `SO_ACCEPTCONN`.
//! - [`peer_closed`]: which peer-query error means only that the peer already closed.
//!
//! Descriptors are passed as raw numbers. Every function here only queries the
//! descriptor: none closes it, keeps it or changes its flags, and a number that is
//! not an open descriptor is an error from the kernel.

use std::io;
use std::os::fd::{OwnedFd, RawFd};
use std::sync::Mutex;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

// Unused in a build for macOS or GNU/Linux, where only its tests call it.
#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
mod shared;
#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
use shared::checked_uid;

/// The effective UID of the process at the other end of a connected Unix socket, as
/// the kernel recorded it. A failed or truncated query is an error; callers refuse
/// the peer.
pub fn peer_uid(fd: RawFd) -> io::Result<u32> {
    native::peer_uid(fd)
}

/// One nonblocking descriptor receive (`recvmsg` with `SCM_RIGHTS`) on this system.
///
/// Create it immediately before each `recvmsg` attempt and keep it until the received
/// descriptors are either close-on-exec or closed. On macOS it holds the caller's
/// fence for that whole time, so a concurrent `fork`/`exec` under the same fence
/// cannot inherit a descriptor that is not yet close-on-exec. On Linux it holds
/// nothing: [`Self::flags`] makes the kernel set close-on-exec atomically.
///
/// A value exists only where the build target has an adapter, so its methods cannot
/// fail for lack of one.
pub struct DescriptorReceive<'fence>(native::DescriptorReceive<'fence>);

/// Begin one descriptor receive. `fence` is the lock every concurrent spawn path of
/// the process also takes.
///
/// macOS: locks `fence`; a poisoned fence is an error and nothing may be received.
/// Linux: neither locks nor inspects `fence`.
pub fn descriptor_receive(fence: &Mutex<()>) -> io::Result<DescriptorReceive<'_>> {
    native::descriptor_receive(fence).map(DescriptorReceive)
}

impl DescriptorReceive<'_> {
    /// The `recvmsg` flags: nonblocking, and close-on-exec for received descriptors
    /// where the system can set it atomically.
    pub fn flags(&self) -> libc::c_int {
        self.0.flags()
    }

    /// `message.msg_controllen` as a byte count. `libc` declares the field as `size_t`
    /// on Linux and as `socklen_t` on macOS.
    pub fn control_len(&self, message: &libc::msghdr) -> usize {
        self.0.control_len(message)
    }

    /// Make `descriptors`, just received with [`Self::flags`], close-on-exec where
    /// the receive could not: `set` is called once per descriptor, in order, and the
    /// first error is returned without calling it again. The caller then closes every
    /// descriptor before it drops `self`. Where the receive already set the flag,
    /// `set` is not called.
    pub fn close_on_exec(
        &self,
        descriptors: &[OwnedFd],
        set: impl FnMut(RawFd) -> io::Result<()>,
    ) -> io::Result<()> {
        self.0.close_on_exec(descriptors, set)
    }
}

/// The `sendmsg` flags for one nonblocking send that must not raise `SIGPIPE` where
/// the system has a flag for that (Linux: `MSG_NOSIGNAL`). On macOS the caller owns
/// `SIGPIPE`.
pub fn send_flags() -> io::Result<libc::c_int> {
    native::send_flags()
}

/// Whether the socket `fd` is listening for connections.
pub fn listening(fd: RawFd) -> io::Result<bool> {
    native::listening(fd)
}

/// Whether `error`, from a peer-address query on a Unix stream socket, means only
/// that the peer has already closed, so the stream may still be adopted and read to
/// its end.
///
/// Ask only after [`listening`] has returned `false` for the socket: on macOS a
/// listener answers the query with the same error. Where the system has no such
/// error, or no adapter, this is `false` and the caller refuses the descriptor.
pub fn peer_closed(error: &io::Error) -> bool {
    peer_closed_when(error, native::PEER_CLOSED_IS_EINVAL)
}

// Both platforms reject listeners first, including macOS sockets that were
// connected before listening. Only then may macOS peer-gone EINVAL be adopted.
// Linux never accepts peer-query errors.
fn peer_closed_when(error: &io::Error, einval_means_closed: bool) -> bool {
    einval_means_closed && error.raw_os_error() == Some(libc::EINVAL)
}

#[cfg(test)]
mod tests;
