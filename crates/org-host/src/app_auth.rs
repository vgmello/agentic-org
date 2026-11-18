//! Peer checks reserved for verified CLI sessions in 2b.3. Current host.sock operations
//! never authorize the retired `office.app_token` or `office.restart` methods.
//!
//! The threat is the exec swap: a process connects, keeps a copy of the socket in a child, sends
//! an app-only call, then `exec`s the signed app, so the host checks the app while the child
//! reads the answer. `LOCAL_PEERTOKEN` gives the *current* audit token of the process that last
//! touched the socket, and an `exec` keeps the pid but changes the audit token (its pid
//! version). Any touch, even a read or a zero-length write, moves it to the toucher. So, on
//! every app-only call (handled on the connection's reader, which reads nothing more meanwhile):
//! 1. The request must have been sent after the connection was accepted: its line starts at or
//!    after the bytes already buffered then (`FIONREAD`, read right after the token at accept).
//!    Bytes buffered earlier may have been written by the image the process `exec`ed away from.
//! 2. Nothing may have arrived after the request's line: nothing buffered past it in the
//!    reader, nothing waiting on the socket (`FIONREAD` 0).
//! 3. The caller's audit token now must equal, byte for byte, the one taken at accept. A byte
//!    written by another process (the child) or after an `exec` changes it.
//! 4. That audit token gives the caller's code object (`SecCodeCopyGuestWithAttributes`), which
//!    must satisfy the app's designated requirement, built from the configured bundle id and
//!    team id (a Developer ID Application certificate of that team only):
//!
//! ```text
//! identifier "<bundle id>" and anchor apple generic
//!   and certificate 1[field.1.2.840.113635.100.6.2.6] exists
//!   and certificate leaf[field.1.2.840.113635.100.6.1.13] exists
//!   and certificate leaf[subject.OU] = "<team id>"
//! ```
//!
//! 5. The same code object's signing information (`SecCodeCopySigningInformation`) must show the
//!    hardened runtime and none of the entitlements that let another process or library reach
//!    into it ([`code_signing::REJECTED_ENTITLEMENTS`]).
//! 6. After the decision, checks 2 and 3 are made again, so nothing that happened while it was
//!    taken slips through.
//!
//! Every check fails closed: any error refuses the call (`not_authorized`).
//!
//! An exec between the host's accept and its token sample can still slip through steps 1 and 3
//! if it happens before the client sends anything: the defence is that the app answers only
//! for its own requests. So the **client rule**: the app never sends an app-only call before
//! it has read the answer to its `hello` (so the call can't be buffered before the accept).
//! A request buffered before the accept is refused with `not_authorized`.
//!
//! With no team id configured, no signed app passes. Debug builds only (the code isn't even
//! compiled into release builds) may also allow an unsigned development app instead of steps 4
//! and 5: a caller running as this user whose executable is exactly the configured path (the
//! app's executable, `Contents/MacOS/<exe>`, as `proc_pidpath` reports it), every use logged.
//!
//! **Where the system has no code identity for a native client** (GNU/Linux; ADR-0017
//! decision 11) the kernel names only the user that connected, and that never changes for a
//! connection: there is no image to swap and no signature to check. A caller the kernel names
//! as this host's own user is admitted, in every build, instead of steps 4 and 5; the other
//! steps are made as everywhere. Admission is then no authority at all: authority comes only
//! from the session a person approved. The platform says which kind of system this is with
//! each identity it reports ([`Identity::uid`] is set only where the user is all there is), so
//! this module has one path and asks nothing about the system.

#[cfg(debug_assertions)]
use std::path::{Path, PathBuf};

/// The desktop app's bundle id.
pub const APP_BUNDLE_ID: &str = "com.vgmello.agentic-org";

pub(crate) use crate::peer::Identity;

use crate::code_signing;
use crate::code_signing::developer_id_requirement;

/// Whether `identity` is a peer the system names by its user alone, and that user is this
/// process's own. `false` wherever the system identifies a peer's image instead: there the
/// user is not what admits it.
pub(crate) fn is_this_user(identity: &Identity) -> bool {
    identity.uid == Some(crate::home::euid())
}

/// What the policy needs to know about a caller. The real one asks the kernel and the Security
/// framework about a connection; tests use fakes.
pub(crate) trait Peer {
    /// Whether the request was sent after the connection was accepted (its line starts at or
    /// after the bytes already buffered then).
    fn sent_after_accept(&self) -> bool;
    /// Whether nothing has arrived after the request's line: nothing buffered past it, and
    /// nothing waiting on the socket.
    fn nothing_after_request(&self) -> bool;
    /// The caller's identity when its connection was accepted (on macOS its audit token).
    fn accepted_identity(&self) -> Option<Identity>;
    /// The caller's identity now.
    fn current_identity(&self) -> Option<Identity>;
    /// Whether the process `identity` names satisfies the designated requirement `requirement`
    /// and runs hardened code (see [`code_signing::hardened_and_clean`]).
    fn satisfies(&self, identity: &Identity, requirement: &str) -> bool;
    /// The effective UID when the kernel confirms a matching peer, otherwise None (development fallback).
    #[cfg(debug_assertions)]
    fn uid(&self) -> Option<u32>;
    /// The path of the caller's executable (the development fallback).
    #[cfg(debug_assertions)]
    fn executable(&self) -> Option<PathBuf>;
}

/// Which check let a caller in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grant {
    /// The app's code signature.
    Signed,
    /// This host's own user, on a system that has no code identity for a native client.
    User,
    /// The unsigned development app (debug builds only).
    #[cfg(debug_assertions)]
    Development,
}

/// The policy for app-only calls.
#[derive(Debug)]
pub(crate) struct AppPolicy {
    /// The designated requirement, or none when no team id is configured.
    requirement: Option<String>,
    /// The development app allowed without a signature (canonical path).
    #[cfg(debug_assertions)]
    dev_app: Option<PathBuf>,
}

impl AppPolicy {
    /// The policy for `bundle_id` and `team_id` (debug builds may add the development app with
    /// `with_dev_app`). A bundle id or team id that isn't well formed is an error, never
    /// a weaker policy.
    pub(crate) fn new(bundle_id: &str, team_id: Option<&str>) -> Result<Self, String> {
        let requirement = team_id
            .map(|team_id| designated_requirement(bundle_id, team_id))
            .transpose()?;
        Ok(Self {
            requirement,
            #[cfg(debug_assertions)]
            dev_app: None,
        })
    }

    /// Debug builds only: also accept the unsigned development app at `path` (its executable).
    /// A path that isn't absolute is an error. One that doesn't exist (yet: the app isn't built)
    /// or isn't an executable file (the app folder instead of its executable) is logged, and the
    /// fallback stays off.
    #[cfg(debug_assertions)]
    pub(crate) fn with_dev_app(mut self, path: &Path) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err(format!(
                "the development app path {} isn't absolute",
                path.display()
            ));
        }
        match path.canonicalize() {
            Ok(path) if is_executable_file(&path) => self.dev_app = Some(path),
            Ok(_) => crate::host_log::log(&format!(
                "the development fallback is off: {} isn't the app's executable file (give \
                 Contents/MacOS/<exe> inside the app, not the app folder)",
                path.display()
            )),
            Err(e) => crate::host_log::log(&format!(
                "the development fallback is off: the development app {} can't be found ({e}); \
                 build the app, then restart the host",
                path.display()
            )),
        }
        Ok(self)
    }

    /// Whether `peer` may use the app-only calls.
    pub(crate) fn allows(&self, peer: &impl Peer) -> bool {
        let Some(grant) = self.grant(peer) else {
            return false;
        };
        #[cfg(debug_assertions)]
        if let (Grant::Development, Some(dev_app)) = (grant, &self.dev_app) {
            // Loudly, every time, and only for a call that is really allowed (after the final
            // re-check): this path skips the code-signature check.
            crate::host_log::log(&format!(
                "DEVELOPMENT FALLBACK: an app-only call was allowed for the unsigned {} \
                 (debug build, no code-signature check)",
                dev_app.display()
            ));
        }
        let _ = grant;
        true
    }

    /// Which check lets `peer` in, the final re-check included: `None` refuses the call.
    fn grant(&self, peer: &impl Peer) -> Option<Grant> {
        let grant = self.decide(peer)?;
        // Nothing may have happened while it was decided either: nothing arrived after the
        // request, and the caller is still the process accepted.
        let still = peer.nothing_after_request()
            && peer.accepted_identity().is_some()
            && peer.current_identity() == peer.accepted_identity();
        still.then_some(grant)
    }

    /// Which check would let `peer` in, before the final re-check in [`Self::grant`].
    fn decide(&self, peer: &impl Peer) -> Option<Grant> {
        // Sent after the accept, with nothing after it, by the process the host accepted (see
        // the module docs).
        if !peer.sent_after_accept() || !peer.nothing_after_request() {
            return None;
        }
        let identity = peer.accepted_identity()?;
        if peer.current_identity() != Some(identity) {
            return None;
        }
        if let Some(requirement) = &self.requirement {
            if peer.satisfies(&identity, requirement) {
                return Some(Grant::Signed);
            }
        }
        // No code identity exists for it on this system: its user is the whole admission.
        if is_this_user(&identity) {
            return Some(Grant::User);
        }
        #[cfg(debug_assertions)]
        if let Some(dev_app) = &self.dev_app {
            let allowed = peer.uid() == Some(crate::home::euid())
                && peer
                    .executable()
                    .and_then(|path| path.canonicalize().ok())
                    .is_some_and(|path| path == *dev_app)
                // Still the same process after its executable was looked up by pid.
                && peer.current_identity() == Some(identity);
            return allowed.then_some(Grant::Development);
        }
        None
    }

    /// What the host logs about this policy at startup.
    pub(crate) fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines
            .push("host.sock refuses retired office.app_token and office.restart calls".to_owned());
        let by_user = names_peers_by_user();
        if by_user {
            lines.push(
                "host.sock admits a native client run by this user: this system has no code \
                 identity for a command, so admission grants nothing and authority comes only \
                 from an approved session"
                    .to_owned(),
            );
        }
        match &self.requirement {
            Some(requirement) => lines.push(format!(
                "configured CLI peer requirement (inactive until sessions): {requirement}"
            )),
            // Nothing is missing where no peer can be signed.
            None if by_user => {}
            None => lines.push(
                "WARNING: no app team id (a release build takes it from ORG_APP_TEAM_ID when it's \
                 built): no signed CLI peer is configured; session operations are inactive"
                    .to_owned(),
            ),
        }
        #[cfg(debug_assertions)]
        if let Some(dev_app) = &self.dev_app {
            lines.push(format!(
                "DEVELOPMENT FALLBACK (debug build only, inactive until sessions): \
                 configured peer {} run by this user, WITHOUT checking its code signature",
                dev_app.display()
            ));
        }
        lines
    }
}

/// Whether this system names the peer of a connection by its user alone: asked of a
/// connection of this process to itself, as the host asks of every client.
fn names_peers_by_user() -> bool {
    std::os::unix::net::UnixStream::pair()
        .and_then(|(one, _other)| crate::peer::identity(&one))
        .is_ok_and(|identity| identity.uid.is_some())
}

/// Whether `path` is a regular file someone may execute (debug builds: the development app).
#[cfg(debug_assertions)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The app's designated requirement: its bundle id, signed with the team's Developer ID
/// (see [`developer_id_requirement`]).
pub(crate) fn designated_requirement(bundle_id: &str, team_id: &str) -> Result<String, String> {
    developer_id_requirement(bundle_id, team_id)
}

/// The caller on a `host.sock` connection, as the kernel and the Security framework see it.
/// On a system that cannot identify a peer at all it has no identity, then or now, so it is
/// refused every app-only call.
pub(crate) struct SocketPeer<'a> {
    pub(crate) stream: &'a std::os::unix::net::UnixStream,
    /// What the host noted when it accepted the connection.
    pub(crate) accepted: crate::peer::Accepted,
    /// Where the request's line was in the connection's stream when it was read.
    pub(crate) received: crate::peer::Received,
}

impl Peer for SocketPeer<'_> {
    fn sent_after_accept(&self) -> bool {
        self.received.sent_at >= self.accepted.buffered
    }

    fn nothing_after_request(&self) -> bool {
        // The reader reads nothing more while an app-only call is handled (it runs inline), so
        // the bytes it had buffered past the line are still all there is besides the socket's.
        self.received.buffered_after == 0
            && crate::peer::bytes_waiting(self.stream).is_ok_and(|n| n == 0)
    }

    fn accepted_identity(&self) -> Option<Identity> {
        self.accepted.identity
    }

    fn current_identity(&self) -> Option<Identity> {
        crate::peer::identity(self.stream).ok()
    }

    fn satisfies(&self, identity: &Identity, requirement: &str) -> bool {
        // The requirement and the hardening checked on the same code object.
        code_signing::satisfies_hardened(identity, requirement)
    }

    /// Returns the effective UID only when the peer matches it, otherwise None.
    /// A foreign peer's UID is not reported by this adapter.
    #[cfg(debug_assertions)]
    fn uid(&self) -> Option<u32> {
        let uid = org_socket::effective_uid();
        org_socket::peer_allowed(self.stream, uid).then_some(uid)
    }

    #[cfg(debug_assertions)]
    fn executable(&self) -> Option<PathBuf> {
        crate::peer::executable(self.stream).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A caller identified by its image: the audit token `byte` repeated.
    fn image(byte: u8) -> Identity {
        Identity::from_macos_audit_token([byte; 32])
    }

    /// A caller the system names by its user alone: `uid`, and some PID.
    fn user(uid: u32) -> Identity {
        Identity::from_linux_credentials(uid, 4242)
    }

    struct FakePeer {
        sent_after_accept: bool,
        nothing_after: bool,
        accepted: Option<Identity>,
        current: Option<Identity>,
        /// After this many reads of the current token, it changes (another process wrote).
        swap_after: Option<usize>,
        reads: std::cell::Cell<usize>,
        signed: bool,
        #[cfg(debug_assertions)]
        uid: u32,
        #[cfg(debug_assertions)]
        executable: PathBuf,
    }

    impl Peer for FakePeer {
        fn sent_after_accept(&self) -> bool {
            self.sent_after_accept
        }
        fn nothing_after_request(&self) -> bool {
            self.nothing_after
        }
        fn accepted_identity(&self) -> Option<Identity> {
            self.accepted
        }
        fn current_identity(&self) -> Option<Identity> {
            let reads = self.reads.get() + 1;
            self.reads.set(reads);
            match self.swap_after {
                Some(after) if reads > after => Some(image(9)),
                _ => self.current,
            }
        }
        fn satisfies(&self, _identity: &Identity, _requirement: &str) -> bool {
            self.signed
        }
        #[cfg(debug_assertions)]
        fn uid(&self) -> Option<u32> {
            Some(self.uid)
        }
        #[cfg(debug_assertions)]
        fn executable(&self) -> Option<PathBuf> {
            Some(self.executable.clone())
        }
    }

    /// A caller that's still the process accepted (the same audit token), sending after the
    /// accept: this user, running this test binary.
    fn caller(signed: bool) -> FakePeer {
        FakePeer {
            sent_after_accept: true,
            nothing_after: true,
            accepted: Some(image(7)),
            current: Some(image(7)),
            swap_after: None,
            reads: std::cell::Cell::new(0),
            signed,
            #[cfg(debug_assertions)]
            uid: crate::home::euid(),
            #[cfg(debug_assertions)]
            executable: std::env::current_exe().unwrap(),
        }
    }

    #[test]
    fn the_requirement_names_the_bundle_and_team() {
        assert_eq!(
            designated_requirement(APP_BUNDLE_ID, "AB12CD34EF").unwrap(),
            "identifier \"com.vgmello.agentic-org\" and anchor apple generic and \
             certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
             certificate leaf[field.1.2.840.113635.100.6.1.13] exists and \
             certificate leaf[subject.OU] = \"AB12CD34EF\""
        );
        for team in ["", "ab12cd34ef", "AB12CD34E", "AB12CD34EFG", "AB12CD34\"F"] {
            assert!(
                designated_requirement(APP_BUNDLE_ID, team).is_err(),
                "{team}"
            );
        }
        for bundle in ["", ".com.x", "com.x\" or anchor apple", "com x", "com.é"] {
            assert!(
                designated_requirement(bundle, "AB12CD34EF").is_err(),
                "{bundle}"
            );
        }
    }

    /// As a release build is set up: no team id and no development app. Nobody passes, not even
    /// a caller whose code would satisfy any requirement.
    #[test]
    fn with_no_team_id_and_no_development_app_everyone_is_refused() {
        let policy = AppPolicy::new(APP_BUNDLE_ID, None).unwrap();
        assert!(!policy.allows(&caller(true)));
    }

    #[test]
    fn a_signed_caller_passes() {
        let policy = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
        assert!(policy.allows(&caller(true)));
        assert!(!policy.allows(&caller(false)));
    }

    /// The exec swap: the caller's audit token changed since its connection was accepted (it
    /// `exec`ed the signed app after connecting). Refused, though its code now satisfies the
    /// requirement and it is the development app.
    #[test]
    fn a_caller_whose_audit_token_changed_since_accept_is_refused() {
        let mut swapped = caller(true);
        swapped.current = Some(image(8));
        let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
        assert!(!signed.allows(&swapped));
        #[cfg(debug_assertions)]
        {
            let me = std::env::current_exe().unwrap();
            let dev = AppPolicy::new(APP_BUNDLE_ID, None)
                .unwrap()
                .with_dev_app(&me)
                .unwrap();
            assert!(!dev.allows(&swapped));
        }
        // And one whose token can't be read, then or now.
        let mut unknown = caller(true);
        unknown.current = None;
        assert!(!signed.allows(&unknown));
        unknown.accepted = None;
        assert!(!signed.allows(&unknown));
    }

    /// Another process touches the connection after the decision was taken: the caller's audit
    /// token changes between the decision and the answer. Refused by the final re-check.
    #[test]
    fn a_caller_whose_audit_token_changes_after_the_decision_is_refused() {
        #[allow(unused_mut)] // Release builds have no development app.
        let mut policies = vec![(
            AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap(),
            Grant::Signed,
        )];
        #[cfg(debug_assertions)]
        {
            let me = std::env::current_exe().unwrap();
            policies.push((
                AppPolicy::new(APP_BUNDLE_ID, None)
                    .unwrap()
                    .with_dev_app(&me)
                    .unwrap(),
                Grant::Development,
            ));
        }
        for (policy, expected) in policies {
            // How many times the decision alone reads the current token.
            let unchanged = caller(true);
            assert!(policy.decide(&unchanged).is_some());
            assert_eq!(policy.grant(&caller(true)), Some(expected));
            let decision_reads = unchanged.reads.get();
            // The token changes right after those reads: the decision still says yes...
            let mut late = caller(true);
            late.swap_after = Some(decision_reads);
            assert!(policy.decide(&late).is_some());
            // ...and the whole check says no (so the development fallback, which logs only
            // for a grant, never logs this call as allowed)...
            let mut late = caller(true);
            late.swap_after = Some(decision_reads);
            assert_eq!(policy.grant(&late), None);
            let mut late = caller(true);
            late.swap_after = Some(decision_reads);
            assert!(!policy.allows(&late));
            assert!(policy.allows(&caller(true)));
        }
    }

    /// A request already buffered when the connection was accepted: refused, though everything
    /// else checks out (it may have been written by the image the process `exec`ed away from).
    #[test]
    fn a_request_buffered_before_the_accept_is_refused() {
        let mut early = caller(true);
        early.sent_after_accept = false;
        let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
        assert!(!signed.allows(&early));
        #[cfg(debug_assertions)]
        {
            let me = std::env::current_exe().unwrap();
            let dev = AppPolicy::new(APP_BUNDLE_ID, None)
                .unwrap()
                .with_dev_app(&me)
                .unwrap();
            assert!(!dev.allows(&early));
        }
    }

    /// Anything that arrived after the request (an extra byte, another line), however it got
    /// there: refused, though everything else checks out.
    #[test]
    fn a_request_with_anything_after_it_is_refused() {
        let mut followed = caller(true);
        followed.nothing_after = false;
        let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
        assert!(!signed.allows(&followed));
        #[cfg(debug_assertions)]
        {
            let me = std::env::current_exe().unwrap();
            let dev = AppPolicy::new(APP_BUNDLE_ID, None)
                .unwrap()
                .with_dev_app(&me)
                .unwrap();
            assert!(!dev.allows(&followed));
        }
    }

    /// A caller the system names by its user alone (it has no code identity for a native
    /// client) is admitted exactly when that user is this host's own, in every build, with or
    /// without a team id; the checks of the request's place in the stream still apply.
    #[test]
    fn a_caller_named_by_its_user_alone_passes_only_as_this_hosts_user() {
        let by_user = |uid: u32| {
            let mut peer = caller(false);
            peer.accepted = Some(user(uid));
            peer.current = Some(user(uid));
            peer
        };
        let me = crate::home::euid();
        for policy in [
            AppPolicy::new(APP_BUNDLE_ID, None).unwrap(),
            AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap(),
        ] {
            assert_eq!(policy.grant(&by_user(me)), Some(Grant::User));
            assert!(policy.allows(&by_user(me)));
            // Another user is refused, root among them: nothing admits root here.
            for other in [me.wrapping_add(1), 0] {
                if other != me {
                    assert!(!policy.allows(&by_user(other)), "{other}");
                }
            }
            // The stream checks are the same as for a signed caller.
            let mut early = by_user(me);
            early.sent_after_accept = false;
            assert!(!policy.allows(&early));
            let mut followed = by_user(me);
            followed.nothing_after = false;
            assert!(!policy.allows(&followed));
            // Not the connection that was accepted any more.
            let mut other_process = by_user(me);
            other_process.current = Some(Identity::from_linux_credentials(me, 4243));
            assert!(!policy.allows(&other_process));
            let mut unknown = by_user(me);
            unknown.current = None;
            assert!(!policy.allows(&unknown));
        }
        // A caller identified by its image is never admitted for its user.
        assert!(!is_this_user(&image(7)));
        assert!(is_this_user(&user(me)));
    }

    /// What the host logs: where a native client is named by its user alone it says so and
    /// misses no team id; elsewhere a missing team id is a warning.
    #[test]
    fn the_description_says_how_this_system_admits_a_native_client() {
        let lines = AppPolicy::new(APP_BUNDLE_ID, None).unwrap().describe();
        let by_user = lines.iter().any(|line| line.contains("run by this user"));
        let warned = lines.iter().any(|line| line.starts_with("WARNING"));
        assert_eq!(by_user, names_peers_by_user(), "{lines:?}");
        assert_ne!(by_user, warned, "{lines:?}");
        let (one, _other) = std::os::unix::net::UnixStream::pair().unwrap();
        assert_eq!(
            by_user,
            crate::peer::identity(&one).is_ok_and(|identity| is_this_user(&identity))
        );
    }

    /// `nothing_after_request` on a real socket: true only while nothing waits past the
    /// request, in the reader's buffer or on the socket.
    #[test]
    fn a_socket_peer_sees_bytes_after_the_request() {
        use std::io::Write;
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = |stream, buffered_after| SocketPeer {
            stream,
            accepted: crate::peer::Accepted {
                identity: None,
                buffered: 0,
            },
            received: crate::peer::Received {
                sent_at: 0,
                buffered_after,
            },
        };
        assert!(peer(&a, 0).nothing_after_request());
        assert!(!peer(&a, 1).nothing_after_request());
        (&b).write_all(b"x").unwrap();
        assert!(!peer(&a, 0).nothing_after_request());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_socket_peer_reports_the_effective_uid_after_shared_admission() {
        let (stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = SocketPeer {
            stream: &stream,
            accepted: crate::peer::Accepted {
                identity: None,
                buffered: 0,
            },
            received: crate::peer::Received {
                sent_at: 0,
                buffered_after: 0,
            },
        };
        assert_eq!(crate::home::euid(), org_socket::effective_uid());
        assert_eq!(peer.uid(), Some(crate::home::euid()));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_socket_peer_refuses_a_failed_credential_query() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let stream = std::os::unix::net::UnixStream::from(std::os::fd::OwnedFd::from(file));
        let peer = SocketPeer {
            stream: &stream,
            accepted: crate::peer::Accepted {
                identity: None,
                buffered: 0,
            },
            received: crate::peer::Received {
                sent_at: 0,
                buffered_after: 0,
            },
        };
        assert_eq!(peer.uid(), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_development_fallback_needs_the_same_user_and_path() {
        let me = std::env::current_exe().unwrap();
        let dev = AppPolicy::new(APP_BUNDLE_ID, None)
            .unwrap()
            .with_dev_app(&me)
            .unwrap();
        assert!(dev.allows(&caller(false)));
        let mut other_user = caller(false);
        other_user.uid = crate::home::euid().wrapping_add(1);
        assert!(!dev.allows(&other_user));
        let mut other_exe = caller(false);
        other_exe.executable = "/bin/sh".into();
        assert!(!dev.allows(&other_exe));
        assert!(AppPolicy::new(APP_BUNDLE_ID, None)
            .unwrap()
            .with_dev_app(Path::new("relative"))
            .is_err());
    }

    /// The development app isn't built yet: the host still starts, with the fallback off.
    #[cfg(debug_assertions)]
    #[test]
    fn a_development_app_that_isnt_built_leaves_the_fallback_off() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("Org.app/Contents/MacOS/Org");
        let dev = AppPolicy::new(APP_BUNDLE_ID, None)
            .unwrap()
            .with_dev_app(&missing)
            .unwrap();
        assert!(dev.dev_app.is_none());
        assert!(!dev.describe().iter().any(|line| line.contains("FALLBACK")));
        let mut there = caller(false);
        there.executable = missing;
        assert!(!dev.allows(&there));
    }

    /// The app folder, or a file that can't be executed, instead of the app's executable: no
    /// caller could ever match it, so the fallback stays off rather than being described as on.
    #[cfg(debug_assertions)]
    #[test]
    fn a_development_app_folder_instead_of_its_executable_leaves_the_fallback_off() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Org.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        let dev = AppPolicy::new(APP_BUNDLE_ID, None)
            .unwrap()
            .with_dev_app(&bundle)
            .unwrap();
        assert!(dev.dev_app.is_none());
        assert!(!dev.describe().iter().any(|line| line.contains("FALLBACK")));
        // A file inside it that can't be executed: off too.
        let plain = bundle.join("Contents/MacOS/Org");
        std::fs::write(&plain, b"not a program").unwrap();
        let dev = AppPolicy::new(APP_BUNDLE_ID, None)
            .unwrap()
            .with_dev_app(&plain)
            .unwrap();
        assert!(dev.dev_app.is_none());
        assert!(!dev.describe().iter().any(|line| line.contains("FALLBACK")));
    }

    /// This test process's own identity, through the policy, as the host takes it. Its code
    /// satisfies no requirement on any system (it is not the app). Where the system identifies
    /// its image, it is refused; where the system names only its user, it is this host's own
    /// user and is admitted for that. The Security-framework path itself, both ways, is tested
    /// with the adapter (`org_platform_host::admission`).
    #[test]
    fn the_policy_judges_this_process_by_its_real_identity() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let identity = crate::peer::identity(&a).unwrap();
        let requirement = designated_requirement(APP_BUNDLE_ID, "AB12CD34EF").unwrap();
        assert!(!code_signing::satisfies_hardened(&identity, &requirement));
        let peer = SocketPeer {
            stream: &a,
            accepted: crate::peer::Accepted {
                identity: Some(identity),
                buffered: 0,
            },
            received: crate::peer::Received {
                sent_at: 0,
                buffered_after: 0,
            },
        };
        let policy = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
        assert_eq!(policy.allows(&peer), is_this_user(&identity));
        assert_eq!(
            policy.grant(&peer),
            is_this_user(&identity).then_some(Grant::User)
        );
    }

    /// A request counts as sent after the accept only if its line starts at or after the bytes
    /// buffered then (N0).
    #[test]
    fn a_socket_peer_compares_the_request_offset_with_what_was_buffered() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = |sent_at| SocketPeer {
            stream: &a,
            accepted: crate::peer::Accepted {
                identity: None,
                buffered: 10,
            },
            received: crate::peer::Received {
                sent_at,
                buffered_after: 0,
            },
        };
        assert!(!peer(0).sent_after_accept());
        assert!(!peer(9).sent_after_accept());
        assert!(peer(10).sent_after_accept());
        assert!(peer(42).sent_after_accept());
    }
}
