//! Who is at the other end of a connection on `host.sock` (or a key socket, `handoff`), as the
//! kernel reports it: its uid, and on macOS its audit token, its pid and its executable's path;
//! what the host noted when it accepted the connection, and where each request was in the stream.
//! `app_auth` decides what these allow; the listener records them.

#[cfg(debug_assertions)]
use std::ffi::OsStr;
use std::io;
#[cfg(debug_assertions)]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
#[cfg(debug_assertions)]
use std::path::PathBuf;

/// An `audit_token_t`: eight 32-bit words.
pub(crate) const AUDIT_TOKEN_BYTES: usize = 32;

/// A process's audit token (`audit_token_t`).
pub(crate) type AuditToken = [u8; AUDIT_TOKEN_BYTES];

/// Where a request's line was in the connection's stream when the reader read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Received {
    /// The byte offset where the line began.
    pub(crate) sent_at: u64,
    /// The bytes the reader had already buffered past the line (none means nothing more had
    /// reached the host's buffer; the socket may still hold some, see [`bytes_waiting`]).
    pub(crate) buffered_after: usize,
}

/// What the host notes about a client as it accepts its connection, for the app-only calls
/// (`app_auth`): its audit token, taken first, then how many bytes it had already sent. A byte
/// buffered before the token was taken may come from a process image that was replaced since
/// (the exec swap), so an app-only call is accepted only from a request line that starts at or
/// after `buffered`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Accepted {
    pub(crate) token: Option<AuditToken>,
    pub(crate) buffered: u64,
}

impl Accepted {
    pub(crate) fn sample(stream: &UnixStream) -> Self {
        #[cfg(target_os = "macos")]
        let token = audit_token(stream).ok();
        #[cfg(not(target_os = "macos"))]
        let token = None;
        // After the token: anything sent before it was taken is counted as buffered.
        let buffered = bytes_waiting(stream).unwrap_or(u64::MAX);
        Self { token, buffered }
    }
}

/// How many bytes are waiting to be read on `stream` (`FIONREAD`).
pub(crate) fn bytes_waiting(stream: &UnixStream) -> io::Result<u64> {
    let mut waiting: libc::c_int = 0;
    // SAFETY: a valid socket fd, and an out-value of the size FIONREAD writes.
    let rc = unsafe { libc::ioctl(stream.as_raw_fd(), libc::FIONREAD, &mut waiting) };
    if rc == 0 {
        Ok(u64::try_from(waiting).unwrap_or(0))
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The uid of the process at the other end of a Unix socket.
pub(crate) fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    #[cfg(not(target_os = "linux"))]
    {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: a valid socket fd and two valid out-pointers.
        let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
        if rc == 0 {
            Ok(uid)
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: zeroed ucred is valid; getsockopt writes at most `len` bytes into it.
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        if rc == 0 {
            Ok(cred.uid)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

/// The peer's audit token, which the code-signing check identifies the caller by. Unlike a pid,
/// it names one process for its whole life and is never reused.
#[cfg(target_os = "macos")]
pub(crate) fn audit_token(stream: &UnixStream) -> io::Result<[u8; AUDIT_TOKEN_BYTES]> {
    let mut token = [0u8; AUDIT_TOKEN_BYTES];
    let mut len = AUDIT_TOKEN_BYTES as libc::socklen_t;
    // SAFETY: a valid socket fd, and an out-buffer of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != AUDIT_TOKEN_BYTES {
        return Err(io::Error::other(
            "the peer's audit token has an unexpected size",
        ));
    }
    Ok(token)
}

/// The peer's pid (for the debug-only fallbacks, and the tests' hand-off rule; a pid can be
/// reused, an audit token can't).
#[cfg(all(target_os = "macos", any(debug_assertions, test)))]
pub(crate) fn pid(stream: &UnixStream) -> io::Result<libc::pid_t> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: a valid socket fd, and an out-value of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    if rc == 0 {
        Ok(pid)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The path of the executable process `pid` runs (for the debug-only fallback).
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) fn executable(pid: libc::pid_t) -> io::Result<PathBuf> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: a buffer of exactly the size passed, which outlives the call.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(n as usize);
    Ok(PathBuf::from(OsStr::from_bytes(&buf)))
}

/// Tests only: a connection to `path` made by **another process**, then passed to this one with
/// `SCM_RIGHTS`, the way a helper could hand a connection to the process the host admitted. A
/// forked child connects, sends the descriptor over a socket pair and exits; this process
/// receives it without touching the connection itself. Returns it and the child's pid.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn connection_made_elsewhere(
    path: &std::path::Path,
) -> io::Result<(std::os::fd::OwnedFd, libc::pid_t)> {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;

    let (ours, theirs) = UnixStream::pair()?;
    // Everything the child uses is made before the fork: after it, the child calls only
    // async-signal-safe functions (this process has other threads) and allocates nothing.
    // SAFETY: an all-zero `sockaddr_un` is a valid value.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() >= addr.sun_path.len() {
        return Err(io::Error::other("path too long"));
    }
    for (slot, &b) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = b as libc::c_char;
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let addr_len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    addr.sun_len = addr_len as u8;
    let fd_len = std::mem::size_of::<libc::c_int>() as u32;
    // SAFETY: CMSG_SPACE only computes a size.
    let space = unsafe { libc::CMSG_SPACE(fd_len) } as usize;
    let mut control = vec![0u8; space];
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: an all-zero `msghdr` is a valid value; its pointers are set below, to memory that
    // outlives every use.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as libc::socklen_t;
    let theirs_fd = theirs.as_raw_fd();

    // SAFETY: the child runs only async-signal-safe calls (socket, connect, sendmsg, _exit) on
    // memory made before the fork, then exits without returning or unwinding.
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // SAFETY: as above; `msg` points at `iov`, `byte` and `control`, which are this
        // process's own copies after the fork.
        unsafe {
            let s = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
            if s < 0 || libc::connect(s, (&addr as *const libc::sockaddr_un).cast(), addr_len) != 0
            {
                libc::_exit(1);
            }
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(fd_len) as libc::socklen_t;
            std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<libc::c_int>(), s);
            if libc::sendmsg(theirs_fd, &msg, 0) != 1 {
                libc::_exit(2);
            }
            libc::_exit(0);
        }
    }
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut status = 0;
    // SAFETY: our own child, and a valid out-pointer.
    if unsafe { libc::waitpid(pid, &mut status, 0) } != pid || status != 0 {
        return Err(io::Error::other(format!(
            "the connecting child failed ({status})"
        )));
    }
    drop(theirs);
    control.fill(0);
    // SAFETY: `msg` still points at `iov`, `byte` and `control`, all alive; recvmsg writes at
    // most their sizes.
    let n = unsafe { libc::recvmsg(ours.as_raw_fd(), &mut msg, 0) };
    if n != 1 {
        return Err(io::Error::other("no descriptor arrived"));
    }
    // SAFETY: `msg` was filled by recvmsg; the header, if any, lies within `control`.
    let fd = unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() || (*cmsg).cmsg_type != libc::SCM_RIGHTS {
            return Err(io::Error::other("no descriptor arrived"));
        }
        std::ptr::read_unaligned(libc::CMSG_DATA(cmsg).cast::<libc::c_int>())
    };
    // SAFETY: a descriptor SCM_RIGHTS just gave this process, owned by nothing else.
    Ok((unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) }, pid))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn a_socket_pair_reports_this_process() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert!(audit_token(&a).unwrap().iter().any(|&b| b != 0));
        #[cfg(debug_assertions)]
        {
            assert_eq!(pid(&a).unwrap(), std::process::id() as libc::pid_t);
            let me = executable(std::process::id() as libc::pid_t).unwrap();
            assert_eq!(
                me.canonicalize().unwrap(),
                std::env::current_exe().unwrap().canonicalize().unwrap()
            );
        }
    }
}
