//! Who is at the other end of a connection on `host.sock`, as the kernel reports it (macOS):
//! its audit token, its pid and its executable's path. `app_auth` decides what they allow.

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

/// The peer's audit token, which the code-signing check identifies the caller by. Unlike a pid,
/// it names one process for its whole life and is never reused.
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
#[cfg(debug_assertions)]
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
#[cfg(debug_assertions)]
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

#[cfg(test)]
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
