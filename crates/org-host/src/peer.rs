//! Who is at the other end of a connection on `host.sock`, as the kernel reports it: its uid,
//! and on macOS its audit token, its pid and its executable's path; what the host noted when it
//! accepted the connection, and where each request was in the stream. `app_auth` decides what
//! these allow; the listener records them. The kernel is asked through
//! `org_platform_host::admission`, which refuses on a system that cannot identify a peer's
//! image: no token is noted there, so nothing that needs one is allowed.

use std::io;
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
        let token = audit_token(stream).ok();
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

/// The peer's audit token, which the code-signing check identifies the caller by. Unlike a pid,
/// it names one process for its whole life and is never reused. A system that cannot identify a
/// peer's image refuses ([`unidentifiable`]).
pub(crate) fn audit_token(stream: &UnixStream) -> io::Result<AuditToken> {
    org_platform_host::admission::peer(stream).map(|peer| peer.macos_audit_token)
}

/// Whether `error`, from [`audit_token`], says that this system cannot identify a peer's image
/// at all, rather than that asking about this peer failed.
pub(crate) fn unidentifiable(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|source| source.is::<org_platform::Refused>())
}

/// The path of the executable the peer's process runs, found through its pid (for the
/// debug-only fallback; a pid can be reused, an audit token can't).
#[cfg(debug_assertions)]
pub(crate) fn executable(stream: &UnixStream) -> io::Result<PathBuf> {
    org_platform_host::admission::peer_executable(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// What is noted at the accept: the bytes the client had already sent, and its token
    /// exactly when the system identifies peers at all.
    #[test]
    fn a_sample_counts_what_was_already_sent_and_keeps_the_token_the_system_gives() {
        let (a, b) = UnixStream::pair().unwrap();
        assert_eq!(Accepted::sample(&a).buffered, 0);
        (&b).write_all(b"early").unwrap();
        let sampled = Accepted::sample(&a);
        assert_eq!(sampled.buffered, 5);
        match audit_token(&a) {
            Ok(token) => {
                assert_eq!(sampled.token, Some(token));
                org_platform_host::admission::native_peers().unwrap();
            }
            Err(error) => {
                assert!(unidentifiable(&error), "{error}");
                assert_eq!(sampled.token, None);
                assert!(org_platform_host::admission::native_peers().is_err());
            }
        }
        // An error that is no refusal of the system is not taken for one.
        assert!(!unidentifiable(&io::Error::from_raw_os_error(
            libc::ENOTSOCK
        )));
    }
}
