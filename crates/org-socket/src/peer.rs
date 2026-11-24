//! Kernel peer credentials for Unix sockets; no protocol or caller identity policy.
use org_platform::socket::{peer_pid as platform_peer_pid, peer_uid};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

/// The effective UID of this process, for admitting same-user local connections.
pub fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments or memory safety preconditions.
    unsafe { libc::geteuid() }
}

/// Whether the kernel identifies the peer as `expected_uid`. Query failures refuse the peer.
pub fn peer_allowed(stream: &UnixStream, expected_uid: u32) -> bool {
    peer_allowed_fd(stream.as_raw_fd(), expected_uid)
}

/// The kernel-reported PID of the peer on a connected Unix stream socket.
pub fn peer_pid(stream: &UnixStream) -> io::Result<libc::pid_t> {
    platform_peer_pid(stream.as_raw_fd())
}

fn peer_allowed_fd(fd: RawFd, expected_uid: u32) -> bool {
    peer_uid(fd).is_ok_and(|uid| uid == expected_uid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_pair_admits_only_its_kernel_uid_and_bad_descriptors_fail_closed() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        // SAFETY: geteuid has no preconditions; this is an independent oracle for the export.
        let uid = unsafe { libc::geteuid() };
        assert_eq!(effective_uid(), uid);
        assert_eq!(peer_uid(stream.as_raw_fd()).unwrap(), uid);
        assert!(peer_allowed(&stream, uid));
        assert!(!peer_allowed(&stream, uid.wrapping_add(1)));
        assert!(!peer_allowed_fd(-1, 0));
        assert_eq!(peer_uid(-1).unwrap_err().raw_os_error(), Some(libc::EBADF));
    }
}
