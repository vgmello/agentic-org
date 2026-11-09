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

/// What the check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    /// The peer is the host.
    Admitted,
    /// The peer was seen and is not the host.
    Stranger,
    /// The kernel would not say (the peer has already closed: `ENOTCONN`). Nothing was read;
    /// the launch can be tried again.
    Unknown,
}

/// Whether the peer of the connected socket `fd` is allowed under `rule`. Fails closed. What the
/// kernel says about the peer (its pid, its audit token, its uid) is taken FIRST and all at once:
/// the host may close at any moment, after which the kernel no longer answers; the slower checks
/// (the code signature) are made on what was saved.
pub(crate) fn admits(fd: RawFd, rule: PeerRule) -> Admission {
    let pid = peer_pid(fd);
    #[cfg(target_os = "macos")]
    let token = peer_token(fd);
    let user = peer_uid(fd);
    let Some(pid) = pid else {
        return Admission::Unknown;
    };
    let Some(user) = user else {
        return Admission::Unknown;
    };
    // SAFETY: `geteuid` and `getppid` have no preconditions.
    let (me, parent) = unsafe { (libc::geteuid(), libc::getppid()) };
    let user_ok = user == me;
    let admitted = match rule {
        #[cfg(test)]
        PeerRule::Pid(expected) => user_ok && pid == expected,
        // The host started this server: it is the parent, in debug builds as in release.
        PeerRule::Host if cfg!(debug_assertions) => user_ok && pid == parent,
        PeerRule::Host => {
            #[cfg(target_os = "macos")]
            {
                // The audit token must have been readable too.
                let Some(token) = token else {
                    return Admission::Unknown;
                };
                user_ok && pid == parent && release_host(token)
            }
            #[cfg(not(target_os = "macos"))]
            {
                false
            }
        }
    };
    if admitted {
        Admission::Admitted
    } else {
        Admission::Stranger
    }
}

/// The peer's uid (`getpeereid`), if the kernel says.
fn peer_uid(fd: RawFd) -> Option<libc::uid_t> {
    let (mut uid, mut gid): (libc::uid_t, libc::gid_t) = (0, 0);
    // SAFETY: a socket descriptor and two valid out-values.
    (unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } == 0).then_some(uid)
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
fn release_host(token: crate::codesign::AuditToken) -> bool {
    use crate::codesign::{
        developer_id_requirement, hardened_and_clean, own_signing, satisfies_hardened,
    };
    let Some(team) = TEAM_ID else {
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
        assert_eq!(
            admits(ours.as_raw_fd(), PeerRule::Pid(me)),
            Admission::Admitted
        );
        assert_eq!(
            admits(ours.as_raw_fd(), PeerRule::Pid(me + 1)),
            Admission::Stranger
        );
        assert_eq!(
            admits(ours.as_raw_fd(), PeerRule::Pid(1)),
            Admission::Stranger
        );
    }

    #[test]
    fn in_a_test_build_the_host_rule_wants_the_parent_and_this_peer_is_not_it() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        // The peer is this process, not its parent.
        assert_eq!(
            admits(ours.as_raw_fd(), PeerRule::Host),
            Admission::Stranger
        );
    }

    #[test]
    fn a_closed_or_non_socket_descriptor_is_not_admitted() {
        let file = std::fs::File::open("/dev/null").unwrap();
        assert_eq!(
            admits(file.as_raw_fd(), PeerRule::Pid(1)),
            Admission::Unknown
        );
        assert_eq!(admits(-1, PeerRule::Host), Admission::Unknown);
    }

    #[test]
    fn a_peer_that_has_closed_cannot_be_asked_and_is_unknown_not_a_stranger() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        drop(theirs);
        assert_eq!(
            admits(ours.as_raw_fd(), PeerRule::Pid(1)),
            Admission::Unknown
        );
    }
}
