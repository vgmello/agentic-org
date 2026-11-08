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
    /// Whether nothing has arrived after the request's line: nothing buffered past it, and
    /// nothing waiting on the socket.
    fn nothing_after_request(&self) -> bool;
    /// The caller's audit token when its connection was accepted.
    fn accepted_token(&self) -> Option<AuditToken>;
    /// The caller's audit token now.
    fn current_token(&self) -> Option<AuditToken>;
    /// Whether the process `token` names satisfies the designated requirement `requirement`
    /// and runs hardened code (see [`hardened_and_clean`]).
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
        let allowed = self.decide(peer);
        // Nothing may have arrived after the request while it was checked, either.
        allowed && peer.nothing_after_request()
    }

    fn decide(&self, peer: &impl Peer) -> bool {
        // Sent after the accept, with nothing after it, by the process the host accepted (see
        // the module docs).
        if !peer.sent_after_accept() || !peer.nothing_after_request() {
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
        "identifier \"{bundle_id}\" and anchor apple generic and {DEVELOPER_ID} and \
         certificate leaf[subject.OU] = \"{team_id}\""
    ))
}

/// The Developer ID markers: the signing certificate is a Developer ID Application certificate
/// (leaf extension 1.2.840.113635.100.6.1.13) issued by the Developer ID intermediate
/// (1.2.840.113635.100.6.2.6). So only the app's notarizable Developer ID signature passes, not
/// a development or distribution certificate of the same team.
const DEVELOPER_ID: &str = "certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
                            certificate leaf[field.1.2.840.113635.100.6.1.13] exists";

/// The code-signature flag of the hardened runtime (`kSecCodeSignatureRuntime`).
pub(crate) const HARDENED_RUNTIME: u32 = 0x0001_0000;

/// Entitlements that let another process or library reach into the app (a debugger, injected
/// libraries, `DYLD_*` variables): an app with any of them could be made to hand its app token
/// over, so it isn't trusted with one.
pub(crate) const REJECTED_ENTITLEMENTS: [&str; 3] = [
    "com.apple.security.get-task-allow",
    "com.apple.security.cs.allow-dyld-environment-variables",
    "com.apple.security.cs.disable-library-validation",
];

/// What the host reads from a caller's signing information.
#[derive(Debug, Default)]
pub(crate) struct Signing {
    /// The code-signature flags (`kSecCodeInfoFlags`), if present.
    pub(crate) flags: Option<u32>,
    /// The entitlements (`kSecCodeInfoEntitlementsDict`): each key with its value when it's a
    /// boolean (`None` for any other kind of value). `None` when the code has no entitlements.
    pub(crate) entitlements: Option<Vec<(String, Option<bool>)>>,
}

/// Whether signed code may get an app token: it runs with the hardened runtime and has none of
/// the [`REJECTED_ENTITLEMENTS`] (one that's present with any value but `false` counts). Missing
/// flags fail closed.
pub(crate) fn hardened_and_clean(signing: &Signing) -> bool {
    let Some(flags) = signing.flags else {
        return false;
    };
    if flags & HARDENED_RUNTIME == 0 {
        return false;
    }
    !signing
        .entitlements
        .iter()
        .flatten()
        .any(|(key, value)| REJECTED_ENTITLEMENTS.contains(&key.as_str()) && *value != Some(false))
}

/// The caller on a `host.sock` connection, as the kernel and the Security framework see it.
#[cfg(target_os = "macos")]
pub(crate) struct SocketPeer<'a> {
    pub(crate) stream: &'a std::os::unix::net::UnixStream,
    /// What the host noted when it accepted the connection.
    pub(crate) accepted: crate::service::listener::Accepted,
    /// Where the request's line was in the connection's stream when it was read.
    pub(crate) received: crate::service::listener::Received,
}

#[cfg(target_os = "macos")]
impl Peer for SocketPeer<'_> {
    fn sent_after_accept(&self) -> bool {
        self.received.sent_at >= self.accepted.buffered
    }

    fn nothing_after_request(&self) -> bool {
        // The reader reads nothing more while an app-only call is handled (it runs inline), so
        // the bytes it had buffered past the line are still all there is besides the socket's.
        self.received.buffered_after == 0
            && crate::service::listener::bytes_waiting(self.stream).is_ok_and(|n| n == 0)
    }

    fn accepted_token(&self) -> Option<AuditToken> {
        self.accepted.token
    }

    fn current_token(&self) -> Option<AuditToken> {
        crate::peer::audit_token(self.stream).ok()
    }

    fn satisfies(&self, token: &AuditToken, requirement: &str) -> bool {
        // The requirement and the hardening checked on the same code object.
        guest_code(token).is_some_and(|code| {
            meets(&code, requirement) && signing(&code).is_some_and(|s| hardened_and_clean(&s))
        })
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

/// The code object of the process `token` names. `None` on any error, including a token whose
/// process has exited or `exec`ed since.
#[cfg(target_os = "macos")]
fn guest_code(token: &AuditToken) -> Option<security_framework::os::macos::code_signing::SecCode> {
    use core_foundation::base::TCFType;
    use core_foundation::data::CFData;
    use security_framework::os::macos::code_signing::{Flags, GuestAttributes, SecCode};

    let token = CFData::from_buffer(token);
    let mut attributes = GuestAttributes::new();
    attributes.set_audit_token(token.as_concrete_TypeRef());
    SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE).ok()
}

/// Whether `code` is valid and satisfies `requirement`. Fails closed.
#[cfg(target_os = "macos")]
fn meets(code: &security_framework::os::macos::code_signing::SecCode, requirement: &str) -> bool {
    use security_framework::os::macos::code_signing::{Flags, SecRequirement};
    use std::str::FromStr;

    SecRequirement::from_str(requirement)
        .is_ok_and(|requirement| code.check_validity(Flags::NONE, &requirement).is_ok())
}

/// Whether the process `token` names satisfies `requirement` (no hardening check: what the
/// plumbing test can show on an unsigned-by-Developer-ID test binary).
#[cfg(all(target_os = "macos", test))]
fn code_satisfies(token: &AuditToken, requirement: &str) -> bool {
    guest_code(token).is_some_and(|code| meets(&code, requirement))
}

/// `code`'s signing information: its flags and entitlements (`SecCodeCopySigningInformation`).
/// `None` on any error.
#[cfg(target_os = "macos")]
fn signing(code: &security_framework::os::macos::code_signing::SecCode) -> Option<Signing> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn SecCodeCopySigningInformation(
            code: *const std::ffi::c_void,
            flags: u32,
            information: *mut CFDictionaryRef,
        ) -> i32;
        static kSecCodeInfoFlags: CFStringRef;
        static kSecCodeInfoEntitlementsDict: CFStringRef;
    }
    /// `kSecCSSigningInformation | kSecCSRequirementInformation`.
    const INFORMATION: u32 = (1 << 1) | (1 << 2);

    let mut information: CFDictionaryRef = std::ptr::null();
    // SAFETY: a valid code object (a SecCode is accepted where a SecStaticCode is expected), and
    // an out-pointer the call fills with a dictionary we then own (create rule).
    let status = unsafe {
        SecCodeCopySigningInformation(
            code.as_concrete_TypeRef().cast(),
            INFORMATION,
            &mut information,
        )
    };
    if status != 0 || information.is_null() {
        return None;
    }
    // SAFETY: a dictionary the call created for us.
    let information: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(information) };
    // SAFETY: framework constants, valid for the process's life.
    let (flags_key, entitlements_key) = unsafe {
        (
            CFString::wrap_under_get_rule(kSecCodeInfoFlags),
            CFString::wrap_under_get_rule(kSecCodeInfoEntitlementsDict),
        )
    };
    let flags = information
        .find(&flags_key)
        .and_then(|value| value.downcast::<CFNumber>())
        .and_then(|number| number.to_i64())
        .and_then(|number| u32::try_from(number).ok());
    let entitlements = information
        .find(&entitlements_key)
        .and_then(|value| value.downcast::<CFDictionary>())
        .map(|dictionary| {
            // SAFETY: an entitlements dictionary maps strings to property-list values.
            let dictionary: CFDictionary<CFString, CFType> =
                unsafe { CFDictionary::wrap_under_get_rule(dictionary.as_concrete_TypeRef()) };
            let (keys, values) = dictionary.get_keys_and_values();
            keys.into_iter()
                .zip(values)
                .map(|(key, value)| {
                    // SAFETY: the keys and values of the dictionary, retained by it.
                    let key = unsafe { CFString::wrap_under_get_rule(key.cast()) };
                    let value = unsafe { CFType::wrap_under_get_rule(value) };
                    (
                        key.to_string(),
                        value.downcast::<CFBoolean>().map(bool::from),
                    )
                })
                .collect()
        });
    Some(Signing {
        flags,
        entitlements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePeer {
        sent_after_accept: bool,
        nothing_after: bool,
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
        fn nothing_after_request(&self) -> bool {
            self.nothing_after
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
            nothing_after: true,
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

    /// Anything that arrived after the request (an extra byte, another line), however it got
    /// there: refused, though everything else checks out.
    #[test]
    fn a_request_with_anything_after_it_is_refused() {
        let mut followed = caller(true);
        followed.nothing_after = false;
        let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF"), None).unwrap();
        assert!(!signed.allows(&followed));
        #[cfg(debug_assertions)]
        {
            let me = std::env::current_exe().unwrap();
            let dev = AppPolicy::new(APP_BUNDLE_ID, None, Some(&me)).unwrap();
            assert!(!dev.allows(&followed));
        }
    }

    fn entitlements(pairs: &[(&str, Option<bool>)]) -> Option<Vec<(String, Option<bool>)>> {
        Some(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }

    #[test]
    fn only_hardened_code_without_the_rejected_entitlements_passes() {
        let hardened = Some(HARDENED_RUNTIME | 0x2);
        let ok = |flags, entitlements| {
            hardened_and_clean(&Signing {
                flags,
                entitlements,
            })
        };
        assert!(ok(hardened, None));
        assert!(ok(
            hardened,
            entitlements(&[
                ("com.apple.security.network.client", Some(true)),
                ("com.apple.security.get-task-allow", Some(false)),
            ])
        ));
        // No hardened runtime, or no flags at all.
        assert!(!ok(Some(0x2), None));
        assert!(!ok(None, None));
        // Each rejected entitlement, set true or with a value that isn't a boolean.
        for key in REJECTED_ENTITLEMENTS {
            assert!(!ok(hardened, entitlements(&[(key, Some(true))])), "{key}");
            assert!(!ok(hardened, entitlements(&[(key, None)])), "{key}");
        }
    }

    /// The real signing information of this test binary matches what `codesign` reports, and
    /// it isn't hardened (a linker or ad-hoc signature): so the extraction works, and the
    /// decision says no for it. (No Developer ID-signed, hardened binary is at hand for the
    /// other answer; the decision itself is tested on synthetic information above.)
    #[cfg(target_os = "macos")]
    #[test]
    fn the_signing_information_of_this_process_is_read_as_codesign_shows_it() {
        let Some(expected) = own_codesign_field("flags=") else {
            eprintln!("skipped: this test binary has no code signature");
            return;
        };
        let expected = u32::from_str_radix(
            expected.split('(').next().unwrap().trim_start_matches("0x"),
            16,
        )
        .unwrap();
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let token = crate::peer::audit_token(&a).unwrap();
        let code = guest_code(&token).expect("this process's code object");
        let info = signing(&code).expect("its signing information");
        assert_eq!(info.flags, Some(expected));
        assert!(!hardened_and_clean(&info));
    }

    /// `nothing_after_request` on a real socket: true only while nothing waits past the
    /// request, in the reader's buffer or on the socket.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_socket_peer_sees_bytes_after_the_request() {
        use std::io::Write;
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = |stream, buffered_after| SocketPeer {
            stream,
            accepted: crate::service::listener::Accepted {
                token: None,
                buffered: 0,
            },
            received: crate::service::listener::Received {
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
            received: crate::service::listener::Received {
                sent_at: 0,
                buffered_after: 0,
            },
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
            received: crate::service::listener::Received {
                sent_at,
                buffered_after: 0,
            },
        };
        assert!(!peer(0).sent_after_accept());
        assert!(!peer(9).sent_after_accept());
        assert!(peer(10).sent_after_accept());
        assert!(peer(42).sent_after_accept());
    }

    /// This test binary's signing identifier, as `codesign` reports it (`None` if unsigned).
    #[cfg(target_os = "macos")]
    fn own_signing_identifier() -> Option<String> {
        own_codesign_field("Identifier=")
    }

    /// A field of `codesign -dv`'s report on this test binary (`None` if unsigned).
    #[cfg(target_os = "macos")]
    fn own_codesign_field(prefix: &str) -> Option<String> {
        let out = std::process::Command::new("/usr/bin/codesign")
            .arg("-dv")
            .arg(std::env::current_exe().ok()?)
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .find_map(|line| {
                line.split_whitespace()
                    .find_map(|word| word.strip_prefix(prefix))
                    .map(str::to_owned)
            })
    }
}
