//! Kernel peer credentials for Unix sockets; no protocol or caller identity policy.
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

fn peer_allowed_fd(fd: RawFd, expected_uid: u32) -> bool {
    peer_uid(fd).is_ok_and(|uid| uid == expected_uid)
}

fn peer_uid(fd: RawFd) -> io::Result<u32> {
    #[cfg(not(target_os = "linux"))]
    {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: both out-pointers are live and writable for the getpeereid call.
        let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
        checked_uid(rc, uid, 0, 0)
    }
    #[cfg(target_os = "linux")]
    {
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
        checked_uid(rc, cred.uid, length, expected)
    }
}

// Isolate syscall status/size decisions so a truncated credential can be tested hermetically.
fn checked_uid(
    rc: libc::c_int,
    uid: u32,
    length: libc::socklen_t,
    expected: libc::socklen_t,
) -> io::Result<u32> {
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if length != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected peer credential size",
        ));
    }
    Ok(uid)
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
    #[test]
    fn credential_status_size_and_uid_zero_are_checked_independently() {
        for uid in [0, 501, u32::MAX] {
            assert_eq!(checked_uid(0, uid, 12, 12).unwrap(), uid);
            assert!(checked_uid(-1, uid, 12, 12).is_err());
            for length in [0, 11, 13] {
                assert_eq!(
                    checked_uid(0, uid, length, 12).unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
            }
        }
    }
}
