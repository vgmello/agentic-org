//! Who is at the other end of a connection on `host.sock`, as the kernel reports it: its uid,
//! and on macOS its audit token, its pid and its executable's path; what the host noted when it
//! accepted the connection, and where each request was in the stream. `app_auth` decides what
//! these allow; the listener records them.

#[cfg(all(target_os = "macos", debug_assertions))]
use std::ffi::OsStr;
use std::io;
#[cfg(all(target_os = "macos", debug_assertions))]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
#[cfg(all(target_os = "macos", debug_assertions))]
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
#[cfg_attr(not(test), expect(dead_code, reason = "activated by 2b.3 CLI"))]
pub(crate) struct Accepted {
    #[cfg(target_os = "macos")]
    pub(crate) token: Option<AuditToken>,
    pub(crate) buffered: u64,
}

impl Accepted {
    pub(crate) fn sample(stream: &UnixStream) -> Self {
        #[cfg(target_os = "macos")]
        let token = audit_token(stream).ok();
        // After the token: anything sent before it was taken is counted as buffered.
        let buffered = bytes_waiting(stream).unwrap_or(u64::MAX);
        Self {
            #[cfg(target_os = "macos")]
            token,
            buffered,
        }
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

/// The peer's pid (for the debug-only fallback; a pid can be reused, an audit token can't).
#[cfg(all(target_os = "macos", debug_assertions))]
#[cfg_attr(not(test), expect(dead_code, reason = "activated by 2b.3 CLI"))]
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
#[cfg_attr(not(test), expect(dead_code, reason = "activated by 2b.3 CLI"))]
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
