//! Who may use the app-only calls on `host.sock` (`office.app_token`, `office.restart`): the
//! desktop app, recognised by its code signature (ADR-0003 §2.1).
//!
//! The threat is the exec swap: a process connects, keeps a copy of the socket in a child, sends
//! an app-only call, then `exec`s the signed app, so the host checks the app while the child
//! reads the answer. `LOCAL_PEERTOKEN` gives the *current* audit token of the process that last
//! touched the socket, and an `exec` keeps the pid but changes the audit token (its pid
//! version). So, on every app-only call:
//! 1. The request must have been sent after the connection was accepted: its line starts at or
//!    after the bytes already buffered then (`FIONREAD`, read right after the token at accept).
//!    Bytes buffered earlier may have been written by the image the process `exec`ed away from.
//! 2. The caller's audit token now must equal, byte for byte, the one taken at accept. A byte
//!    written by another process (the child) or after an `exec` changes it.
//! 3. That audit token gives the caller's code object (`SecCodeCopyGuestWithAttributes`), which
//!    must satisfy the app's designated requirement, built from the configured bundle id and
//!    team id:
//!
//! ```text
//! identifier "<bundle id>" and anchor apple generic and certificate leaf[subject.OU] = "<team id>"
//! ```
//!
//! An exec between the host's accept and its token sample can still slip through steps 1 and 2
//! if it happens before the client sends anything: the defence is that the app answers only
//! for its own requests. So the **client rule**: the app never sends an app-only call before
//! it has read the answer to its `hello` (so the call can't be buffered before the accept).
//! A request buffered before the accept is refused with `not_authorized`.
//!
//! With no team id configured, no signed app passes. Debug builds only (the code isn't even
//! compiled into release builds) may also allow an unsigned development app instead of step 3:
//! a caller running as this user whose executable is exactly the configured path.

use std::path::Path;
#[cfg(debug_assertions)]
use std::path::PathBuf;

/// The desktop app's bundle id.
pub const APP_BUNDLE_ID: &str = "com.ellosoft.agentic-org";

/// A process's audit token (`audit_token_t`: eight 32-bit words).
pub(crate) type AuditToken = [u8; 32];

/// What the policy needs to know about a caller. The real one asks the kernel and the Security
/// framework about a connection; tests use fakes.
pub(crate) trait Peer {
    /// Whether the request was sent after the connection was accepted (its line starts at or
    /// after the bytes already buffered then).
    fn sent_after_accept(&self) -> bool;
    /// The caller's audit token when its connection was accepted.
    fn accepted_token(&self) -> Option<AuditToken>;
    /// The caller's audit token now.
    fn current_token(&self) -> Option<AuditToken>;
    /// Whether the process `token` names satisfies the designated requirement `requirement`.
    fn satisfies(&self, token: &AuditToken, requirement: &str) -> bool;
    /// The caller's uid (the development fallback).
    #[cfg(debug_assertions)]
    fn uid(&self) -> Option<u32>;
    /// The path of the caller's executable (the development fallback).
    #[cfg(debug_assertions)]
    fn executable(&self) -> Option<PathBuf>;
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
    /// The policy for `bundle_id` and `team_id`, and, in debug builds only, the development
    /// app `dev_app` (release builds ignore it). A bundle id, team id or development path that
    /// isn't well formed is an error, never a weaker policy.
    pub(crate) fn new(
        bundle_id: &str,
        team_id: Option<&str>,
        dev_app: Option<&Path>,
    ) -> Result<Self, String> {
        let requirement = team_id
            .map(|team_id| designated_requirement(bundle_id, team_id))
            .transpose()?;
        #[cfg(debug_assertions)]
        let dev_app = match dev_app {
            Some(path) if !path.is_absolute() => {
                return Err(format!(
                    "the development app path {} isn't absolute",
                    path.display()
                ))
            }
            Some(path) => Some(
                path.canonicalize()
                    .map_err(|e| format!("the development app path {}: {e}", path.display()))?,
            ),
            None => None,
        };
        #[cfg(not(debug_assertions))]
        let _ = dev_app;
        Ok(Self {
            requirement,
            #[cfg(debug_assertions)]
            dev_app,
        })
    }

    /// Whether `peer` may use the app-only calls.
    pub(crate) fn allows(&self, peer: &impl Peer) -> bool {
        // Sent after the accept, by the process the host accepted (see the module docs).
        if !peer.sent_after_accept() {
            return false;
        }
        let Some(token) = peer.accepted_token() else {
            return false;
        };
        if peer.current_token() != Some(token) {
            return false;
        }
        if let Some(requirement) = &self.requirement {
            if peer.satisfies(&token, requirement) {
                return true;
            }
        }
        #[cfg(debug_assertions)]
        if let Some(dev_app) = &self.dev_app {
            let allowed = peer.uid() == Some(crate::home::euid())
                && peer
                    .executable()
                    .and_then(|path| path.canonicalize().ok())
                    .is_some_and(|path| path == *dev_app)
                // Still the same process after its executable was looked up by pid.
                && peer.current_token() == Some(token);
            if allowed {
                // Loudly, every time: this path skips the code-signature check.
                crate::host_log::log(&format!(
                    "DEVELOPMENT FALLBACK: an app-only call was allowed for the unsigned {} \
                     (debug build, no code-signature check)",
                    dev_app.display()
                ));
            }
            return allowed;
        }
        false
    }

    /// What the host logs about this policy at startup.
    pub(crate) fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        match &self.requirement {
            Some(requirement) => lines.push(format!(
                "office.app_token and office.restart accept the app satisfying: {requirement}"
            )),
            None => lines.push(
                "no app team id configured: office.app_token and office.restart accept no signed \
                 app"
                .to_owned(),
            ),
        }
        #[cfg(debug_assertions)]
        if let Some(dev_app) = &self.dev_app {
            lines.push(format!(
                "DEVELOPMENT FALLBACK (debug build only): office.app_token and office.restart \
                 also accept {} run by this user, WITHOUT checking its code signature",
                dev_app.display()
            ));
        }
        lines
    }
}

/// The app's designated requirement. The bundle id must be a reverse-DNS name (ASCII letters,
/// digits, `.` and `-`) and the team id exactly 10 uppercase ASCII letters or digits, so neither
/// can change the requirement's meaning.
pub(crate) fn designated_requirement(bundle_id: &str, team_id: &str) -> Result<String, String> {
    let bundle_ok = !bundle_id.is_empty()
        && bundle_id.len() <= 255
        && !bundle_id.starts_with(['.', '-'])
        && bundle_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !bundle_ok {
        return Err(format!("the app bundle id {bundle_id:?} isn't well formed"));
    }
    let team_ok = team_id.len() == 10
        && team_id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if !team_ok {
        return Err(format!(
            "the app team id {team_id:?} isn't 10 uppercase letters or digits"
        ));
    }
    Ok(format!(
        "identifier \"{bundle_id}\" and anchor apple generic and certificate leaf[subject.OU] = \"{team_id}\""
    ))
}

/// The caller on a `host.sock` connection, as the kernel and the Security framework see it.
#[cfg(target_os = "macos")]
pub(crate) struct SocketPeer<'a> {
    pub(crate) stream: &'a std::os::unix::net::UnixStream,
    /// What the host noted when it accepted the connection.
    pub(crate) accepted: crate::service::listener::Accepted,
    /// Where the request's line began in the connection's stream.
    pub(crate) sent_at: u64,
}

#[cfg(target_os = "macos")]
impl Peer for SocketPeer<'_> {
    fn sent_after_accept(&self) -> bool {
        self.sent_at >= self.accepted.buffered
    }

    fn accepted_token(&self) -> Option<AuditToken> {
        self.accepted.token
    }

    fn current_token(&self) -> Option<AuditToken> {
        crate::peer::audit_token(self.stream).ok()
    }

    fn satisfies(&self, token: &AuditToken, requirement: &str) -> bool {
        code_satisfies(token, requirement)
    }

    #[cfg(debug_assertions)]
    fn uid(&self) -> Option<u32> {
        crate::service::listener::peer_uid(self.stream).ok()
    }

    #[cfg(debug_assertions)]
    fn executable(&self) -> Option<PathBuf> {
        crate::peer::pid(self.stream)
            .and_then(crate::peer::executable)
            .ok()
    }
}

/// Whether the process `token` names satisfies `requirement`, per the Security framework. Fails
/// closed: any error is `false`, including a token whose process has exited or `exec`ed since.
#[cfg(target_os = "macos")]
fn code_satisfies(token: &AuditToken, requirement: &str) -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::data::CFData;
    use security_framework::os::macos::code_signing::{
        Flags, GuestAttributes, SecCode, SecRequirement,
    };
    use std::str::FromStr;

    let Ok(requirement) = SecRequirement::from_str(requirement) else {
        return false;
    };
    let token = CFData::from_buffer(token);
    let mut attributes = GuestAttributes::new();
    attributes.set_audit_token(token.as_concrete_TypeRef());
    let Ok(code) = SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE) else {
        return false;
    };
    code.check_validity(Flags::NONE, &requirement).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePeer {
        sent_after_accept: bool,
        accepted: Option<AuditToken>,
        current: Option<AuditToken>,
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
        fn accepted_token(&self) -> Option<AuditToken> {
            self.accepted
        }
        fn current_token(&self) -> Option<AuditToken> {
            self.current
        }
        fn satisfies(&self, _token: &AuditToken, _requirement: &str) -> bool {
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
            accepted: Some([7; 32]),
            current: Some([7; 32]),
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
            "identifier \"com.ellosoft.agentic-org\" and anchor apple generic and \
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
        let policy = AppPolicy::new(APP_BUNDLE_ID, None, None).unwrap();
        assert!(!policy.allows(&caller(true)));
    }

    #[test]
    fn a_signed_caller_passes() {
        let policy = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF"), None).unwrap();
        assert!(policy.allows(&caller(true)));
        assert!(!policy.allows(&caller(false)));
    }

    /// The exec swap: the caller's audit token changed since its connection was accepted (it
    /// `exec`ed the signed app after connecting). Refused, though its code now satisfies the
    /// requirement and it is the development app.
    #[test]
    fn a_caller_whose_audit_token_changed_since_accept_is_refused() {
        let mut swapped = caller(true);
        swapped.current = Some([8; 32]);
        let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF"), None).unwrap();
        assert!(!signed.allows(&swapped));
        #[cfg(debug_assertions)]
        {
            let me = std::env::current_exe().unwrap();
            let dev = AppPolicy::new(APP_BUNDLE_ID, None, Some(&me)).unwrap();
            assert!(!dev.allows(&swapped));
        }
        // And one whose token can't be read, then or now.
        let mut unknown = caller(true);
        unknown.current = None;
        assert!(!signed.allows(&unknown));
        unknown.accepted = None;
        assert!(!signed.allows(&unknown));
    }

    /// A request already buffered when the connection was accepted: refused, though everything
    /// else checks out (it may have been written by the image the process `exec`ed away from).
    #[test]
    fn a_request_buffered_before_the_accept_is_refused() {
        let mut early = caller(true);
        early.sent_after_accept = false;
        let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF"), None).unwrap();
        assert!(!signed.allows(&early));
        #[cfg(debug_assertions)]
        {
            let me = std::env::current_exe().unwrap();
            let dev = AppPolicy::new(APP_BUNDLE_ID, None, Some(&me)).unwrap();
            assert!(!dev.allows(&early));
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_development_fallback_needs_the_same_user_and_path() {
        let me = std::env::current_exe().unwrap();
        let dev = AppPolicy::new(APP_BUNDLE_ID, None, Some(&me)).unwrap();
        assert!(dev.allows(&caller(false)));
        let mut other_user = caller(false);
        other_user.uid = crate::home::euid().wrapping_add(1);
        assert!(!dev.allows(&other_user));
        let mut other_exe = caller(false);
        other_exe.executable = "/bin/sh".into();
        assert!(!dev.allows(&other_exe));
        assert!(AppPolicy::new(APP_BUNDLE_ID, None, Some(Path::new("relative"))).is_err());
    }

    /// The real Security-framework path, both ways, on this test process's own audit token: a
    /// requirement it meets (its own signing identifier) passes, others don't. So broken
    /// plumbing, which would fail closed, is caught.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_signature_check_accepts_what_this_process_satisfies_and_nothing_else() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let token = crate::peer::audit_token(&a).unwrap();
        let Some(identifier) = own_signing_identifier() else {
            // An unsigned test binary (no linker signature, e.g. on Intel): nothing to accept.
            eprintln!("skipped: this test binary has no code signature");
            return;
        };
        assert!(
            code_satisfies(&token, &format!("identifier \"{identifier}\"")),
            "{identifier}"
        );
        assert!(!code_satisfies(
            &token,
            &format!("identifier \"{identifier}.not\"")
        ));
        let requirement = designated_requirement(APP_BUNDLE_ID, "AB12CD34EF").unwrap();
        assert!(!code_satisfies(&token, &requirement));
        // Through the policy, as the host takes it: refused (not the app).
        let peer = SocketPeer {
            stream: &a,
            accepted: crate::service::listener::Accepted {
                token: Some(token),
                buffered: 0,
            },
            sent_at: 0,
        };
        let policy = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF"), None).unwrap();
        assert!(!policy.allows(&peer));
    }

    /// A request counts as sent after the accept only if its line starts at or after the bytes
    /// buffered then (N0).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_socket_peer_compares_the_request_offset_with_what_was_buffered() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = |sent_at| SocketPeer {
            stream: &a,
            accepted: crate::service::listener::Accepted {
                token: None,
                buffered: 10,
            },
            sent_at,
        };
        assert!(!peer(0).sent_after_accept());
        assert!(!peer(9).sent_after_accept());
        assert!(peer(10).sent_after_accept());
        assert!(peer(42).sent_after_accept());
    }

    /// This test binary's signing identifier, as `codesign` reports it (`None` if unsigned).
    #[cfg(target_os = "macos")]
    fn own_signing_identifier() -> Option<String> {
        let out = std::process::Command::new("/usr/bin/codesign")
            .arg("-dv")
            .arg(std::env::current_exe().ok()?)
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .find_map(|line| line.strip_prefix("Identifier=").map(str::to_owned))
    }
}
