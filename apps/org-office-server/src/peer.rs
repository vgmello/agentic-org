//! Who is at the other end of the key hand-off connection (launch v3, "The key hand-off"): the
//! server reads the app token and the database key only from the host's listener. Another process
//! of the same user could unlink the host's one-shot socket and bind its own at the same path, and
//! feed the server secrets of its choosing; the host's own checks (inode, owner and mode around its
//! bind) only detect a swap, so the server's check is what stops forged secrets.
//!
//! The check is made right after the connect, BEFORE anything is read: the host writes its line
//! and closes at once, and once the peer has closed the kernel no longer says who it was
//! (`LOCAL_PEERPID` and `LOCAL_PEERTOKEN` fail with `ENOTCONN`; probed on macOS). At connect the
//! peer is the process that listens on the socket.
//!
//! * A **debug** build (unsigned development and test builds): the peer's pid is the server's
//!   parent (the host started it) and its uid is ours.
//! * A **release** build (macOS): the peer is the server's parent too, and its audit token
//!   (`LOCAL_PEERTOKEN`, taken first, before the slower signature work) names code that
//!   satisfies the host's requirement (its signing identifier signed with the app team's
//!   Developer ID) and runs hardened without the entitlements that let another process reach into
//!   it. The team is a compiled-in build input (`ORG_APP_TEAM_ID`, as for the host); the server
//!   also checks that its OWN signature carries that team and is hardened, so a misbuilt or
//!   re-signed server cannot quietly trust another team. No team, an unsigned or ad-hoc server,
//!   or any error: refused.
//!
//! Nothing is sent or read on the connection by this check (`getsockopt` and `getpeereid` only).
#![allow(unsafe_code)]

use std::os::fd::RawFd;

/// The host's signing identifier (the bundle signs `Contents/MacOS/org-office-host` with it).
pub(crate) const HOST_SIGNING_ID: &str = "com.ellosoft.agentic-org.org-office-host";

/// The app team's id, a build input; `None` makes a release server refuse every hand-off.
const TEAM_ID: Option<&str> = option_env!("ORG_APP_TEAM_ID");

/// Whom the server takes secrets from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerRule {
    /// The host that started this server (the rule of the real server).
    Host,
    /// A process with this pid, in the same uid (tests, which play the host in-process).
    #[cfg(test)]
    Pid(libc::pid_t),
}

/// Whether the peer of the connected socket `fd` is allowed under `rule`. Fails closed. What the
/// kernel says about the peer (its pid, its audit token, its uid) is taken FIRST and all at once:
/// the host may close at any moment, after which the kernel no longer answers; the slower checks
/// (the code signature) are made on what was saved.
pub(crate) fn admits(fd: RawFd, rule: PeerRule) -> bool {
    let pid = peer_pid(fd);
    #[cfg(target_os = "macos")]
    let token = peer_token(fd);
    let user_ok = same_user(fd);
    // SAFETY: `getppid` has no preconditions.
    let parent = unsafe { libc::getppid() };
    match rule {
        #[cfg(test)]
        PeerRule::Pid(expected) => user_ok && pid == Some(expected),
        // The host started this server: it is the parent, in debug builds as in release.
        PeerRule::Host if cfg!(debug_assertions) => user_ok && pid == Some(parent),
        PeerRule::Host => {
            #[cfg(target_os = "macos")]
            {
                user_ok && pid == Some(parent) && release_host(token)
            }
            #[cfg(not(target_os = "macos"))]
            {
                false
            }
        }
    }
}

/// Whether the peer's uid is this process's.
fn same_user(fd: RawFd) -> bool {
    let (mut uid, mut gid): (libc::uid_t, libc::gid_t) = (0, 0);
    // SAFETY: a socket descriptor and two valid out-values.
    let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    // SAFETY: `geteuid` has no preconditions.
    rc == 0 && uid == unsafe { libc::geteuid() }
}

/// The pid of the process at the other end (`LOCAL_PEERPID`).
#[cfg(target_os = "macos")]
fn peer_pid(fd: RawFd) -> Option<libc::pid_t> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: a valid socket fd and an out-value of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    (rc == 0 && pid > 0).then_some(pid)
}

#[cfg(not(target_os = "macos"))]
fn peer_pid(_fd: RawFd) -> Option<libc::pid_t> {
    None
}

/// The peer's audit token (`LOCAL_PEERTOKEN`).
#[cfg(target_os = "macos")]
fn peer_token(fd: RawFd) -> Option<crate::codesign::AuditToken> {
    use crate::codesign::AUDIT_TOKEN_BYTES;
    let mut token = [0u8; AUDIT_TOKEN_BYTES];
    let mut len = AUDIT_TOKEN_BYTES as libc::socklen_t;
    // SAFETY: a valid socket fd and an out-buffer of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut len,
        )
    };
    (rc == 0 && len as usize == AUDIT_TOKEN_BYTES).then_some(token)
}

/// The release rule on the audit token already taken: see the module docs.
#[cfg(target_os = "macos")]
fn release_host(token: Option<crate::codesign::AuditToken>) -> bool {
    use crate::codesign::{
        developer_id_requirement, hardened_and_clean, own_signing, satisfies_hardened,
    };
    let (Some(team), Some(token)) = (TEAM_ID, token) else {
        return false;
    };
    // This server's own signature must carry the team and be hardened and clean.
    let own_ok = own_signing()
        .is_some_and(|own| own.team_id.as_deref() == Some(team) && hardened_and_clean(&own));
    if !own_ok {
        return false;
    }
    let Ok(requirement) = developer_id_requirement(HOST_SIGNING_ID, team) else {
        return false;
    };
    satisfies_hardened(&token, &requirement)
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    use super::*;

    #[test]
    fn a_peer_is_admitted_by_its_pid_and_uid_and_not_by_another_pid() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let me = std::process::id() as libc::pid_t;
        // A socket pair's peer is this very process.
        assert!(admits(ours.as_raw_fd(), PeerRule::Pid(me)));
        assert!(!admits(ours.as_raw_fd(), PeerRule::Pid(me + 1)));
        assert!(!admits(ours.as_raw_fd(), PeerRule::Pid(1)));
    }

    #[test]
    fn in_a_test_build_the_host_rule_wants_the_parent_and_this_peer_is_not_it() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        // The peer is this process, not its parent.
        assert!(!admits(ours.as_raw_fd(), PeerRule::Host));
    }

    #[test]
    fn a_closed_or_non_socket_descriptor_is_not_admitted() {
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(!admits(file.as_raw_fd(), PeerRule::Pid(1)));
        assert!(!admits(-1, PeerRule::Host));
    }
}
