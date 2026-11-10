//! The first thing the app does (host contract, `office.app_token` / `office.restart` callers): make sure file
//! descriptors 0, 1 and 2 are not sockets. A parent could hand the app a socket as its stdin, stdout or stderr; the host
//! must never be fooled into trusting a peer through one, and the app must never read or write a descriptor it didn't
//! open itself. Any of the three that is a socket, or is closed, is pointed at `/dev/null`.

#[cfg(unix)]
use std::os::fd::RawFd;

/// Point `fd` at `/dev/null` if it is a socket or is not open. Returns whether it changed anything.
#[cfg(unix)]
pub fn sanitize_fd(fd: RawFd) -> bool {
    // SAFETY: plain libc calls on integers and a zeroed `stat` the kernel fills in; `open` gets a NUL-terminated
    // literal; `dup2` and `close` only touch descriptors this function opened or was asked to replace.
    unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        let open = libc::fstat(fd, &mut st) == 0;
        if open && (st.st_mode & libc::S_IFMT) != libc::S_IFSOCK {
            return false;
        }
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if null < 0 {
            return false;
        }
        let done = null == fd || libc::dup2(null, fd) >= 0;
        if null != fd {
            libc::close(null);
        }
        done
    }
}

/// Whether `fd` is a socket or not open: what `sanitize_fd` replaces.
#[cfg(unix)]
fn needs_fixing(fd: RawFd) -> bool {
    // SAFETY: as in `sanitize_fd`.
    unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        libc::fstat(fd, &mut st) != 0 || (st.st_mode & libc::S_IFMT) == libc::S_IFSOCK
    }
}

/// Run `sanitize_fd` over stdin, stdout and stderr. Call it first in `main`, before any output, logging or thread. If a
/// socket can't be replaced the process stops: it must not run with one behind a standard descriptor.
pub fn sanitize_stdio() {
    #[cfg(unix)]
    for fd in 0..=2 {
        if needs_fixing(fd) && !sanitize_fd(fd) {
            std::process::abort();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    fn mode(fd: RawFd) -> Option<u32> {
        // SAFETY: as in `sanitize_fd`.
        unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            (libc::fstat(fd, &mut st) == 0).then_some(st.st_mode as u32 & libc::S_IFMT as u32)
        }
    }

    /// A copy of `fd` at some high descriptor number, so the test never touches 0, 1 or 2.
    fn high_copy(fd: RawFd, min: i32) -> RawFd {
        // SAFETY: F_DUPFD duplicates an open descriptor to the lowest free one at or above 100.
        unsafe { libc::fcntl(fd, libc::F_DUPFD, min) }
    }

    #[test]
    fn a_socket_is_replaced_by_dev_null_and_the_original_is_untouched() {
        let (a, _b) = UnixStream::pair().unwrap();
        let copy = high_copy(a.as_raw_fd(), 100);
        assert_eq!(mode(copy), Some(libc::S_IFSOCK as u32));
        assert!(sanitize_fd(copy));
        assert_eq!(mode(copy), Some(libc::S_IFCHR as u32), "now /dev/null");
        assert_eq!(
            mode(a.as_raw_fd()),
            Some(libc::S_IFSOCK as u32),
            "the socket itself is still a socket"
        );
        // SAFETY: `copy` is ours.
        unsafe { libc::close(copy) };
    }

    #[test]
    fn a_descriptor_that_is_not_a_socket_is_left_alone_and_a_closed_one_is_filled() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let copy = high_copy(file.as_raw_fd(), 200);
        assert!(!sanitize_fd(copy), "a character device is fine");
        // SAFETY: `copy` is ours.
        unsafe { libc::close(copy) };
        assert_eq!(mode(copy), None, "closed");
        assert!(
            sanitize_fd(copy),
            "a closed descriptor is pointed at /dev/null"
        );
        assert_eq!(mode(copy), Some(libc::S_IFCHR as u32));
        // SAFETY: `copy` is ours again.
        unsafe { libc::close(copy) };
    }
}
