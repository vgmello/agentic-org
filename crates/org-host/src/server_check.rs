//! Who may get an office's secrets key: the check of every office server the host starts,
//! before its handoff (which carries the key) is written.
//!
//! The threat: a same-user process runs the signed host from a copy, a symlink or a hard link,
//! next to its own `agentic-org-office` (the host starts the server next to its own executable),
//! and receives every office's key on the first line. So a release host checks each server it
//! spawns, before that server has run a single instruction: it starts it **suspended**
//! (the platform's process admission, `org_platform_host::admission`; otherwise the image's
//! first code could start a helper that
//! inherits stdin, then `exec` the genuine server and pass the check), and checks it by its
//! **audit token**, which names the image the exec loaded: its code object
//! (`SecCodeCopyGuestWithAttributes`) must satisfy a requirement compiled into the host,
//! the office worker's signing identifier [`OFFICE_SIGNING_ID`] signed with the app team's Developer
//! ID, and run hardened code without the entitlements that let another process reach into it
//! (`code_signing::HARDENED`):
//!
//! ```text
//! identifier "com.vgmello.agentic-org.office" and anchor apple generic
//!   and certificate 1[field.1.2.840.113635.100.6.2.6] exists
//!   and certificate leaf[field.1.2.840.113635.100.6.1.13] exists
//!   and certificate leaf[subject.OU] = "<ORG_APP_TEAM_ID>"
//! ```
//!
//! **Residual risk:** another same-user process can resume a suspended child itself before the
//! host takes its token (stopping or starving the host makes that easy), and so get the check to see the genuine server while its own helper keeps
//! stdin (see the admission adapter's docs). Closing that needs the key handed to an authenticated
//! reader, a contract change; until then this check stops every attack in which nothing resumes the
//! child before the host takes its token.
//!
//! Only a child that passes is resumed. Right before the first line is written, its audit token
//! is taken again and must be unchanged, so an `exec` after the check is refused. A child that
//! fails, or whose token can't be taken, is killed and reaped (still suspended if it failed the
//! first check, so it never ran), and gets nothing; the start counts as a `storage_error`
//! (retried with backoff, up to the give-up rule).
//!
//! A release host with no team id compiled in can't check anything, so it starts no server at
//! all (logged loudly). **Debug builds don't check** (their servers are unsigned development and
//! test builds), which is logged at startup.
//!
//! This module is the policy: [`ServerCheck`] decides whether a server may be started at all,
//! names what its image must be as plain data ([`ServerCheck::expected`]) and words every
//! refusal for the host log ([`ServerCheck::why`]). The platform starts the child, checks it
//! against that data and lets it run ([`ServerCheck::admission`]); on a system without an
//! admission adapter no server starts.
//!
//! The text above is the macOS rule. A system without code signatures (GNU/Linux, ADR-0017
//! decision 7) checks the image before any child exists: the platform reads the program once,
//! when the admission is built, compares its SHA-256 with the digest the selected release's
//! signed manifest names ([`Pin`]) and starts every child from exactly those bytes. There a
//! release host without a selected release starts no server ([`ImageRefusal::Unpinned`]), and
//! a debug host records the digest of its own build output and logs it
//! ([`ServerCheck::describe_admitted`]). The same policy data serves both: each system's
//! adapter reads its own fields of [`ExpectedImage`].

/// The office worker's signing identifier (ADR-0003 §2.1): a release host gives an office's key
/// only to an office worker signed with it by the app's team (`server_check`). The bundle must sign
/// agentic-org-office with it. The signing identifier is stable across filename changes.
pub const OFFICE_SIGNING_ID: &str = "com.vgmello.agentic-org.office";
/// The browser service has no office-worker or Keychain authority (ADR-0008).
pub const WEB_SIGNING_ID: &str = "com.vgmello.agentic-org.web";

use std::io;
use std::path::Path;

use org_platform_host::admission::{
    Admission, AdmissionError, ExpectedImage, ImageIdentity, ImageRefusal, SigningRule,
};

use crate::code_signing::HARDENED;
#[cfg(test)]
use crate::peer::AuditToken;

/// Tests of the pinned image on a system that identifies an image by its code signature.
#[cfg(test)]
mod selected_image_tests;

/// The entitlement that lists the Keychain access groups a signed program may use. The web
/// process has no Keychain authority (ADR-0008): its image may not hold it at all.
const KEYCHAIN_ACCESS_GROUPS: &str = "keychain-access-groups";

/// What the web process's signing information must show: hardened code without the rejected
/// entitlements, as every server, and no Keychain entitlement.
const WEB_SIGNING: SigningRule = SigningRule {
    absent_entitlements: &[KEYCHAIN_ACCESS_GROUPS],
    ..HARDENED
};

/// How the host checks the office servers it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServerCheck {
    /// Debug builds: no check.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Skip,
    /// Every server must satisfy this requirement and run hardened code.
    Requirement(String),
    /// Same Developer ID/hardening rules, additionally forbidding Keychain entitlement.
    Gateway(String),
    /// A release build on a system whose admission checks the bytes it executes, not a code
    /// signature: every server must be the image the selected release names, by the digest
    /// in its signed manifest ([`Pin`]). There is no team and no requirement there; the
    /// publisher is whoever signed the manifest. Without a selected release nothing names
    /// the image, and the platform refuses it ([`ImageRefusal::Unpinned`]).
    Published,
    /// Same platform policy plus the selected publisher-signed exact image.
    Pinned(Box<ServerCheck>, Pin),
    /// No team id: no server may be started.
    Refuse,
    /// The web process may not be started, and why (for the host log).
    WebRefused(&'static str),
    /// Tests only: the admission and the re-check's token come from the test.
    #[cfg(test)]
    Scripted(Scripted),
    /// Tests only: a development image started by its path, with nothing of it verified
    /// (`org_platform_host::testing::admission::path_admission`), for a worker that is a
    /// `#!` script. A system that executes verified bytes cannot start one otherwise.
    #[cfg(test)]
    ByPath,
}

/// The exact image the selected release names for one program: plain data from the verified,
/// signed release manifest. Each system's admission reads its own field: the SHA-256 of the
/// program's bytes where the verified bytes are executed, the CodeDirectory hash where the
/// loaded image is checked. A manifest carries the second only for a system that has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pin {
    pub(crate) sha256: [u8; 32],
    pub(crate) macos_cdhash: Option<[u8; 20]>,
}

/// Why a check has no admission to start servers with ([`ServerCheck::admission`]).
#[derive(Debug)]
pub(crate) enum NoAdmission {
    /// A refusal, in the check's words for the host log: the check may start no server at
    /// all, or the platform read the image and will not start it.
    Refused(String),
    /// This system has no admission adapter, or a step of preparing the image failed.
    Unavailable(io::Error),
}

impl std::fmt::Display for NoAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoAdmission::Refused(why) => f.write_str(why),
            NoAdmission::Unavailable(error) => error.fmt(f),
        }
    }
}

/// For a caller whose own errors are `io::Error`s: a refusal becomes an error with its words,
/// and a system's error stays itself (a missing adapter stays `Unsupported`).
impl From<NoAdmission> for io::Error {
    fn from(none: NoAdmission) -> Self {
        match none {
            NoAdmission::Refused(why) => io::Error::other(why),
            NoAdmission::Unavailable(error) => error,
        }
    }
}

/// Which step a [`Scripted`] check is asked for: `Admit` for the admission of a started child
/// and each re-admission (the admitted token; `None`: not checked; or a refusal), `Still` for a
/// re-check (the child's token now).
#[cfg(test)]
pub(crate) use org_platform_host::testing::admission::Step;

/// A test's stand-in for the live check: called with the step and the child's pid.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct Scripted(
    pub(crate) std::sync::Arc<dyn Fn(Step, u32) -> Result<Option<AuditToken>, String> + Send + Sync>,
);

#[cfg(test)]
impl std::fmt::Debug for Scripted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Scripted")
    }
}

#[cfg(test)]
impl PartialEq for Scripted {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(test)]
impl Eq for Scripted {}

impl ServerCheck {
    pub(crate) fn pinned(self, pin: Pin) -> Self {
        Self::Pinned(Box::new(self), pin)
    }
    /// The web process's check for a build (`debug`) whose app team id is `team_id`, on this
    /// system: what identifies an image here is the platform admission's own fact
    /// ([`org_platform_host::admission::image_identity`]), as for [`Self::for_build`]. A
    /// team id that isn't well formed is an error, never a weaker check.
    pub(crate) fn web_for_build(debug: bool, team_id: Option<&str>) -> Result<Self, String> {
        Self::web_for_system(
            debug,
            team_id,
            org_platform_host::admission::image_identity().ok(),
        )
    }

    /// The web process's check for a build (`debug`) whose app team id is `team_id`, on a
    /// system whose admission identifies an image by `identity` (`None`: no adapter, where
    /// nothing starts whatever this answers). The same decision as [`Self::for_system`],
    /// with the web process's own requirement.
    ///
    /// - Where a code signature is checked, a release build needs the team its web process
    ///   is signed by; without one the web process is never started
    ///   ([`Self::WebRefused`]).
    /// - Where the executed bytes are checked, there is no signature and no team: a release
    ///   build's web process is the image the selected release names ([`Self::Published`],
    ///   which the host pins), with or without a team id. Without a selected release
    ///   nothing names the image and the platform refuses it
    ///   ([`ImageRefusal::Unpinned`]).
    pub(crate) fn web_for_system(
        debug: bool,
        team_id: Option<&str>,
        identity: Option<ImageIdentity>,
    ) -> Result<Self, String> {
        if debug {
            return Ok(Self::Skip);
        }
        let requirement = team_id
            .map(|team| crate::code_signing::developer_id_requirement(WEB_SIGNING_ID, team))
            .transpose()?;
        Ok(match (identity, requirement) {
            (Some(ImageIdentity::VerifiedBytes), _) => Self::Published,
            (_, Some(requirement)) => Self::Gateway(requirement),
            (_, None) => Self::WebRefused("no app team id (ORG_APP_TEAM_ID at build time)"),
        })
    }

    /// The check for a build (`debug`) whose app team id is `team_id`, on this system: what
    /// identifies an image here is the platform admission's own fact
    /// ([`org_platform_host::admission::image_identity`]). A team id that isn't well formed
    /// is an error, never a weaker check.
    pub(crate) fn for_build(debug: bool, team_id: Option<&str>) -> Result<Self, String> {
        Self::for_system(
            debug,
            team_id,
            org_platform_host::admission::image_identity().ok(),
        )
    }

    /// The check for a build (`debug`) whose app team id is `team_id`, on a system whose
    /// admission identifies an image by `identity` (`None`: no adapter, where nothing starts
    /// whatever this answers).
    ///
    /// - A debug build checks nothing: its servers are its own build output.
    /// - Where a code signature is checked, a release build needs the team its servers are
    ///   signed by; without one no server's signature can be checked and none is started.
    /// - Where the executed bytes are checked, there is no signature and no team: a release
    ///   build's servers are the images the selected release names ([`Self::Published`]).
    ///   A team id given there is still validated and then names nothing.
    pub(crate) fn for_system(
        debug: bool,
        team_id: Option<&str>,
        identity: Option<ImageIdentity>,
    ) -> Result<Self, String> {
        if debug {
            return Ok(ServerCheck::Skip);
        }
        let requirement = team_id.map(server_requirement).transpose()?;
        Ok(match (identity, requirement) {
            (Some(ImageIdentity::VerifiedBytes), _) => ServerCheck::Published,
            (_, Some(requirement)) => ServerCheck::Requirement(requirement),
            (_, None) => ServerCheck::Refuse,
        })
    }

    /// What the host logs about the check at startup.
    pub(crate) fn describe(&self) -> String {
        match self {
            ServerCheck::Pinned(check, _) => format!(
                "{}; exact selected release image required",
                check.describe()
            ),
            ServerCheck::Skip => "debug build: office servers' code signatures aren't checked \
                                  before they get an office's key"
                .to_owned(),
            ServerCheck::Requirement(requirement) => format!(
                "office servers get an office's key only if they run hardened code satisfying: \
                 {requirement}"
            ),
            ServerCheck::Gateway(requirement) => format!(
                "gateway requires hardened code without Keychain entitlement: {requirement}"
            ),
            ServerCheck::Published => "a server is started only from the bytes whose SHA-256 \
                                       the selected release's signed manifest names"
                .to_owned(),
            ServerCheck::Refuse => "WARNING: no app team id (ORG_APP_TEAM_ID at build time), so \
                                    no office server's signature can be checked: no office \
                                    server will be started"
                .to_owned(),
            ServerCheck::WebRefused(cause) => {
                format!("WARNING: the web process will not be started: {cause}")
            }
            #[cfg(test)]
            ServerCheck::Scripted(_) => "scripted (tests)".to_owned(),
            #[cfg(test)]
            ServerCheck::ByPath => "started by path, unverified (tests)".to_owned(),
        }
    }

    /// What the host logs once an admission of this check is built, on a system that
    /// executes the bytes it read then: a development image has no publisher check, so the
    /// log names the SHA-256 of the bytes every server of this host will run. `None` where
    /// there is nothing to add to [`Self::describe`]: the image is pinned by a release, or the
    /// system checks the loaded image instead.
    pub(crate) fn describe_admitted(
        &self,
        program: &Path,
        admission: &Admission,
    ) -> Option<String> {
        let digest = admission.image_sha256()?;
        let expected = self.expected(program).ok()?;
        if !expected.development || expected.sha256.is_some() {
            return None;
        }
        Some(format!(
            "debug build: no publisher check of {}; every server this host starts runs the \
             bytes read from it now, SHA-256 {}",
            program.display(),
            hex(&digest)
        ))
    }

    /// Before a spawn: whether any server may be started.
    pub(crate) fn may_spawn(&self) -> Result<(), String> {
        match self {
            ServerCheck::Pinned(check, _) => check.may_spawn(),
            ServerCheck::Refuse => {
                Err("this build has no app team id to check servers with".into())
            }
            ServerCheck::WebRefused(cause) => Err((*cause).into()),
            ServerCheck::Skip | ServerCheck::Requirement(_) => Ok(()),
            ServerCheck::Gateway(_) | ServerCheck::Published => Ok(()),
            #[cfg(test)]
            ServerCheck::Scripted(_) | ServerCheck::ByPath => Ok(()),
        }
    }

    /// What a child started from `program` must be, as the data the platform checks, or why
    /// no child may be started at all (as [`ServerCheck::may_spawn`] says).
    ///
    /// A debug build's servers are its own build output: no publisher check. A release
    /// build's office server must satisfy the requirement and the [`HARDENED`] rule; its web
    /// process, in addition, may hold no Keychain entitlement. A selected release pins the
    /// exact image on top of either, with both identities its manifest names ([`Pin`]).
    pub(crate) fn expected(&self, program: &Path) -> Result<ExpectedImage, String> {
        let image = |development: bool| ExpectedImage {
            program: program.to_owned(),
            sha256: None,
            macos_cdhash: None,
            macos_requirement: None,
            macos_signing: HARDENED,
            development,
        };
        match self {
            ServerCheck::Pinned(check, pin) => {
                let inner = check.expected(program)?;
                // Only a pin names a digest: an inner one must be this very pin.
                if inner.sha256.is_some()
                    && (inner.sha256 != Some(pin.sha256) || inner.macos_cdhash != pin.macos_cdhash)
                {
                    // Two different pins can never both hold.
                    return Err(PIN_MISMATCH.into());
                }
                Ok(ExpectedImage {
                    sha256: Some(pin.sha256),
                    macos_cdhash: pin.macos_cdhash,
                    ..inner
                })
            }
            ServerCheck::Skip => Ok(image(true)),
            // A release image with no requirement: only a pin can name it. A system that
            // checks a code signature never admits it (nothing names its publisher there).
            ServerCheck::Published => Ok(image(false)),
            ServerCheck::Requirement(requirement) => Ok(ExpectedImage {
                macos_requirement: Some(requirement.clone()),
                ..image(false)
            }),
            ServerCheck::Gateway(requirement) => Ok(ExpectedImage {
                macos_requirement: Some(requirement.clone()),
                macos_signing: WEB_SIGNING,
                ..image(false)
            }),
            ServerCheck::Refuse => {
                Err("this build has no app team id to check servers with".into())
            }
            ServerCheck::WebRefused(cause) => Err((*cause).into()),
            // The script answers in place of the publisher check; a pin still applies.
            #[cfg(test)]
            ServerCheck::Scripted(_) | ServerCheck::ByPath => Ok(image(true)),
        }
    }

    /// The platform's admission for this check's servers started from `program`: it starts
    /// each child, checks its image before any of it has run, and re-checks it later. A
    /// system that executes verified bytes reads and checks the image here, so a host builds
    /// its admission once and starts every server from it (`supervisor::child::Images`).
    ///
    /// A check that may not spawn has none, and neither has an image the platform refuses:
    /// both are a refusal in this check's words ([`NoAdmission::Refused`]). A system without
    /// an admission adapter has none either (an [`io::ErrorKind::Unsupported`] error): no
    /// server is started there.
    pub(crate) fn admission(&self, program: &Path) -> Result<Admission, NoAdmission> {
        let expected = self.expected(program).map_err(NoAdmission::Refused)?;
        self.build(expected).map_err(|error| match error {
            AdmissionError::Image(refusal) => NoAdmission::Refused(self.why(&refusal)),
            other => NoAdmission::Unavailable(other.into()),
        })
    }

    /// Asks the platform for the admission of `expected`: the system's own, or in tests the
    /// stand-in this check names.
    fn build(&self, expected: ExpectedImage) -> Result<Admission, AdmissionError> {
        #[cfg(test)]
        if let Some(scripted) = self.scripted() {
            use org_platform_host::admission::Peer;
            let script = scripted.0.clone();
            return org_platform_host::testing::admission::scripted_admission(
                expected,
                std::sync::Arc::new(move |step, pid| {
                    script(step, pid).map(|token| token.map(Peer::from_macos_audit_token))
                }),
            );
        }
        #[cfg(test)]
        if self.by_path() {
            return org_platform_host::testing::admission::path_admission(expected);
        }
        org_platform_host::admission::admission(expected)
    }

    /// The test's script, when this check (or the check a pin wraps) is scripted.
    #[cfg(test)]
    fn scripted(&self) -> Option<&Scripted> {
        match self {
            ServerCheck::Scripted(scripted) => Some(scripted),
            ServerCheck::Pinned(check, _) => check.scripted(),
            _ => None,
        }
    }

    /// Whether this check (or the check a pin wraps) starts its servers by path, unverified.
    #[cfg(test)]
    fn by_path(&self) -> bool {
        match self {
            ServerCheck::ByPath => true,
            ServerCheck::Pinned(check, _) => check.by_path(),
            _ => false,
        }
    }

    /// Whether this is the web process's check (its refusals name the web process).
    fn is_web(&self) -> bool {
        match self {
            ServerCheck::Gateway(_) | ServerCheck::WebRefused(_) => true,
            ServerCheck::Pinned(check, _) => check.is_web(),
            _ => false,
        }
    }

    /// Why the platform refused a child or its image, in this check's words for the host log.
    /// A refusal every system can give is worded for every system; one that only a code
    /// signature can give keeps the words of that check.
    pub(crate) fn why(&self, refusal: &ImageRefusal) -> String {
        match refusal {
            ImageRefusal::Identity(why) => format!("its identity could not be read: {why}"),
            ImageRefusal::NotTheImage if self.is_web() => {
                "it isn't the signed web process (its code signature doesn't satisfy \
                 the web process requirement, it isn't hardened, or it holds a Keychain \
                 entitlement)"
                    .into()
            }
            ImageRefusal::NotTheImage => {
                "it isn't the signed office server (its code signature doesn't satisfy the \
                 requirement, or it isn't hardened)"
                    .into()
            }
            ImageRefusal::Unidentified => "selected release requires platform-audited image".into(),
            ImageRefusal::DigestUnavailable => "selected release kernel image unavailable".into(),
            ImageRefusal::DigestMismatch => PIN_MISMATCH.into(),
            ImageRefusal::Changed => "it changed (exec'ed) after it was checked".into(),
            ImageRefusal::Readmitted => {
                "it was not the same process at a second check as at the first".into()
            }
            ImageRefusal::Unpinned => "this release build has no selected release, so nothing \
                                       names the image it may start (a release image is \
                                       checked against the digest in the signed release \
                                       manifest)"
                .into(),
            ImageRefusal::UnsafeFile(why) => format!(
                "its program file {why}: only a regular file that this user or root alone can \
                 write is started"
            ),
            other => other.to_string(),
        }
    }
}

/// What a pinned image that is not the pinned one is refused with, and two pins that differ.
const PIN_MISMATCH: &str = "selected release image mismatch";

/// Lowercase hexadecimal, for a digest in the host log.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The requirement an office worker must satisfy: [`OFFICE_SIGNING_ID`] signed with the
/// Developer ID of team `team_id`.
pub(crate) fn server_requirement(team_id: &str) -> Result<String, String> {
    crate::code_signing::developer_id_requirement(OFFICE_SIGNING_ID, team_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEAM: &str = "AB12CD34EF";

    #[test]
    fn the_requirement_names_the_server_and_the_team() {
        assert_eq!(
            server_requirement(TEAM).unwrap(),
            "identifier \"com.vgmello.agentic-org.office\" and anchor apple generic \
             and certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
             certificate leaf[field.1.2.840.113635.100.6.1.13] exists and \
             certificate leaf[subject.OU] = \"AB12CD34EF\""
        );
        assert!(server_requirement("not a team").is_err());
    }

    /// What this system's admission identifies an image by: the platform's own fact. Every
    /// test below that differs by system expects exactly the outcome this names.
    fn identity() -> ImageIdentity {
        crate::test_programs::image_identity()
    }

    /// The office check as a pure decision over the build, the team id and what the system's
    /// admission identifies an image by.
    #[test]
    fn the_check_follows_the_build_and_what_the_system_identifies_an_image_by() {
        let signed = Some(ImageIdentity::CodeSignature);
        let bytes = Some(ImageIdentity::VerifiedBytes);
        // A debug build checks nothing, on every system.
        for identity in [signed, bytes, None] {
            for team in [Some(TEAM), None, Some("ab12")] {
                assert_eq!(
                    ServerCheck::for_system(true, team, identity),
                    Ok(ServerCheck::Skip)
                );
            }
        }
        // Where a code signature is checked (and where there is no adapter at all): the
        // team's requirement, and without a team no server is ever started.
        for identity in [signed, None] {
            assert_eq!(
                ServerCheck::for_system(false, Some(TEAM), identity),
                Ok(ServerCheck::Requirement(server_requirement(TEAM).unwrap()))
            );
            assert_eq!(
                ServerCheck::for_system(false, None, identity),
                Ok(ServerCheck::Refuse)
            );
        }
        // Where the executed bytes are checked: no team is needed, and none names anything.
        assert_eq!(
            ServerCheck::for_system(false, None, bytes),
            Ok(ServerCheck::Published)
        );
        assert_eq!(
            ServerCheck::for_system(false, Some(TEAM), bytes),
            Ok(ServerCheck::Published)
        );
        // A malformed team id is an error everywhere, never a weaker check.
        for identity in [signed, bytes, None] {
            assert!(ServerCheck::for_system(false, Some("ab12"), identity).is_err());
        }
        // The host's own constructor is that decision for this system.
        for (debug, team) in [(true, None), (false, None), (false, Some(TEAM))] {
            assert_eq!(
                ServerCheck::for_build(debug, team),
                ServerCheck::for_system(debug, team, Some(identity()))
            );
        }
        assert!(ServerCheck::Refuse.may_spawn().is_err());
        assert!(ServerCheck::Refuse.describe().starts_with("WARNING"));
        assert_eq!(
            ServerCheck::Refuse.describe(),
            "WARNING: no app team id (ORG_APP_TEAM_ID at build time), so no office server's \
             signature can be checked: no office server will be started"
        );
        assert!(ServerCheck::Skip.may_spawn().is_ok());
    }

    /// The release check of a system without code signatures: it may spawn, names a release
    /// image with no requirement and no digest of its own, and a selected release gives it
    /// the pin. Its log line names no signature and no team.
    #[test]
    fn a_published_check_names_a_release_image_that_only_a_pin_identifies() {
        let check = ServerCheck::Published;
        assert!(check.may_spawn().is_ok());
        let expected = check.expected(PROGRAM.as_ref()).unwrap();
        assert!(!expected.development);
        assert_eq!(expected.macos_requirement, None);
        assert_eq!((expected.sha256, expected.macos_cdhash), (None, None));
        let pinned = check
            .clone()
            .pinned(Pin {
                sha256: [7; 32],
                macos_cdhash: None,
            })
            .expected(PROGRAM.as_ref())
            .unwrap();
        assert_eq!(
            pinned,
            ExpectedImage {
                sha256: Some([7; 32]),
                ..expected
            }
        );
        let line = check.describe();
        assert!(line.contains("signed manifest"), "{line}");
        for foreign in ["team", "code signature", "Keychain", "hardened", "kernel"] {
            assert!(!line.contains(foreign), "{line}");
        }
        let pinned_line = check.pinned(pin(7)).describe();
        assert!(
            pinned_line.ends_with("; exact selected release image required"),
            "{pinned_line}"
        );
        assert!(!pinned_line.contains("kernel"), "{pinned_line}");
    }

    /// The web process's check as the same pure decision, and the host's constructor as that
    /// decision for what this system identifies an image by.
    #[test]
    fn the_web_check_follows_the_build_and_what_the_system_identifies_an_image_by() {
        let signed = Some(ImageIdentity::CodeSignature);
        let bytes = Some(ImageIdentity::VerifiedBytes);
        for identity in [signed, bytes, None] {
            assert_eq!(
                ServerCheck::web_for_system(true, None, identity),
                Ok(ServerCheck::Skip)
            );
            assert!(ServerCheck::web_for_system(false, Some("bad"), identity).is_err());
        }
        for identity in [signed, None] {
            assert!(matches!(
                ServerCheck::web_for_system(false, Some(TEAM), identity),
                Ok(ServerCheck::Gateway(_))
            ));
            assert!(matches!(
                ServerCheck::web_for_system(false, None, identity),
                Ok(ServerCheck::WebRefused(_))
            ));
        }
        for team in [None, Some(TEAM)] {
            assert_eq!(
                ServerCheck::web_for_system(false, team, bytes),
                Ok(ServerCheck::Published)
            );
        }
        for (debug, team) in [
            (true, None),
            (false, None),
            (false, Some(TEAM)),
            (false, Some("bad")),
        ] {
            assert_eq!(
                ServerCheck::web_for_build(debug, team),
                ServerCheck::web_for_system(debug, team, Some(identity()))
            );
        }
    }

    /// A release build without a team id, on this system: exactly one of the two outcomes,
    /// the one the platform's fact names.
    #[test]
    fn release_web_without_a_team_is_refused_or_checked_by_its_pin_alone() {
        let check = ServerCheck::web_for_build(false, None).unwrap();
        match identity() {
            // No signature can be checked without a team: no web process, and the log
            // says so in the web process's words.
            ImageIdentity::CodeSignature => {
                assert_eq!(
                    check,
                    ServerCheck::WebRefused("no app team id (ORG_APP_TEAM_ID at build time)")
                );
                assert!(check.may_spawn().is_err());
                assert_eq!(
                    check.describe(),
                    "WARNING: the web process will not be started: no app team id \
                     (ORG_APP_TEAM_ID at build time)"
                );
            }
            // No team names anything here: the image is the one a selected release pins,
            // and without a release the platform refuses it.
            ImageIdentity::VerifiedBytes => {
                assert_eq!(check, ServerCheck::Published);
                assert!(check.may_spawn().is_ok());
                let expected = check.expected(PROGRAM.as_ref()).unwrap();
                assert!(!expected.development);
                assert_eq!(expected.sha256, None);
                assert_eq!(expected.macos_requirement, None);
                assert!(
                    check
                        .why(&ImageRefusal::Unpinned)
                        .contains("no selected release"),
                    "{}",
                    check.why(&ImageRefusal::Unpinned)
                );
                let pinned = check.pinned(pin(7)).expected(PROGRAM.as_ref()).unwrap();
                assert_eq!(pinned.sha256, Some([7; 32]));
                assert!(!pinned.development);
            }
        }
        assert!(ServerCheck::web_for_build(true, None)
            .unwrap()
            .may_spawn()
            .is_ok());
    }

    /// The code-signature rule of the web process's check, as the pure decision: it holds
    /// on a system that checks a code signature, whatever system this test runs on.
    #[test]
    fn gateway_requires_its_distinct_identity_and_never_a_missing_team() {
        let signed = Some(ImageIdentity::CodeSignature);
        let ServerCheck::Gateway(requirement) =
            ServerCheck::web_for_system(false, Some(TEAM), signed).unwrap()
        else {
            panic!("release gateway policy")
        };
        assert!(requirement.contains(WEB_SIGNING_ID));
        assert!(!requirement.contains(OFFICE_SIGNING_ID));
        let no_team = ServerCheck::web_for_system(false, None, signed).unwrap();
        assert!(matches!(no_team, ServerCheck::WebRefused(_)));
        assert!(no_team.may_spawn().is_err());
        assert!(no_team.describe().contains("web process"));
        assert!(no_team.describe().contains("no app team id"));
        assert!(ServerCheck::web_for_system(false, Some("bad"), signed).is_err());
        assert!(ServerCheck::web_for_build(false, Some("bad")).is_err());
        assert_eq!(
            ServerCheck::web_for_system(true, None, signed).unwrap(),
            ServerCheck::Skip
        );
        // A missing team names no image, so no admission exists to ask anything of.
        for refused in [ServerCheck::Refuse, no_team] {
            assert!(refused.expected(PROGRAM.as_ref()).is_err(), "{refused:?}");
            assert!(refused.admission(PROGRAM.as_ref()).is_err(), "{refused:?}");
            // A pin on top of it changes nothing.
            let pinned = refused.pinned(pin(7));
            assert!(pinned.may_spawn().is_err(), "{pinned:?}");
            assert!(pinned.admission(PROGRAM.as_ref()).is_err(), "{pinned:?}");
        }
        let refusal = ServerCheck::Gateway(requirement).why(&ImageRefusal::NotTheImage);
        // The web process refusal names the web process and its full admission condition.
        assert!(refusal.contains("web process"), "{refusal}");
        assert!(!refusal.contains("office server"), "{refusal}");
        for condition in ["code signature", "hardened", "Keychain entitlement"] {
            assert!(refusal.contains(condition), "{refusal}");
        }
    }

    /// A program path for checks that start nothing.
    const PROGRAM: &str = "/nonexistent/agentic-org-office";

    /// A pin as a manifest for a system with CodeDirectory hashes names it: both identities.
    fn pin(byte: u8) -> Pin {
        Pin {
            sha256: [byte; 32],
            macos_cdhash: Some([byte; 20]),
        }
    }

    /// The SHA-256 of the file at `path`.
    fn sha256_of(path: &Path) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(std::fs::read(path).unwrap()).into()
    }

    /// Signing information as the platform reports it: hardened, with these entitlements.
    fn hardened_with(entitlements: &[(&str, Option<bool>)]) -> crate::code_signing::Signing {
        crate::code_signing::Signing {
            flags: Some(crate::code_signing::HARDENED_RUNTIME),
            entitlements: Some(
                entitlements
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), *value))
                    .collect(),
            ),
            keychain_access_groups: None,
        }
    }

    /// The production web check starts real children through the platform's live token,
    /// signature and Keychain probes. With an Apple-anchored requirement (no signing team
    /// needed), hardened Apple binaries reach every probe: `/usr/bin/scp` (no entitlements) is
    /// admitted; `/usr/bin/ssh` (hardened, only keychain-access-groups) satisfies the
    /// signature check, as the office `Requirement` check shows, yet the web process check
    /// refuses it; unhardened `/bin/sleep` fails the signature check.
    #[test]
    fn production_web_admission_uses_the_live_keychain_probe() {
        use org_platform_host::admission::{ChildEnv, SpawnRefused};
        // Only where admission checks a code signature (and so the Keychain entitlement).
        if org_platform_host::admission::image_identity() != Ok(ImageIdentity::CodeSignature) {
            return;
        }
        // Started and checked, never let run: each child is killed still suspended.
        let admit_with = |check: &ServerCheck, program: &str| {
            check
                .admission(program.as_ref())
                .unwrap()
                .start(&[], &ChildEnv::new(), &crate::cloexec::SPAWN_LOCK)
                .map(drop)
                .map_err(|error| {
                    let refused = SpawnRefused::of(&error).expect("an image refusal");
                    check.why(&refused.refusal)
                })
        };
        let web = ServerCheck::Gateway("anchor apple".into());
        let office = ServerCheck::Requirement("anchor apple".into());
        assert!(
            admit_with(&web, "/usr/bin/scp").is_ok(),
            "a hardened child without a Keychain entitlement was refused"
        );
        assert!(
            admit_with(&office, "/usr/bin/ssh").is_ok(),
            "/usr/bin/ssh must satisfy the signature check for this test to isolate the probe"
        );
        let refused = admit_with(&web, "/usr/bin/ssh").expect_err("Keychain child admitted");
        assert!(refused.contains("Keychain entitlement"), "{refused}");
        assert!(
            admit_with(&web, "/bin/sleep").is_err(),
            "an unhardened child was admitted"
        );
    }

    #[test]
    fn the_web_is_admitted_only_without_a_keychain_entitlement() {
        // The rule of a system that checks a code signature, as the pure decision.
        let ServerCheck::Gateway(requirement) =
            ServerCheck::web_for_system(false, Some(TEAM), Some(ImageIdentity::CodeSignature))
                .unwrap()
        else {
            panic!("release gateway policy")
        };
        let check = ServerCheck::Gateway(requirement.clone());
        let expected = check.expected(PROGRAM.as_ref()).unwrap();
        // The platform is asked for exactly the web process requirement, on a release image.
        assert_eq!(
            expected.macos_requirement.as_deref(),
            Some(requirement.as_str())
        );
        assert!(!expected.development);
        assert_eq!(expected.program, Path::new(PROGRAM));
        let rule = expected.macos_signing;
        assert_eq!(rule.absent_entitlements, ["keychain-access-groups"]);
        // A satisfying signature without a Keychain entitlement is admitted.
        assert!(rule.allows(&hardened_with(&[])));
        assert!(rule.allows(&hardened_with(&[(
            "com.apple.security.network.client",
            Some(true)
        )])));
        // The same satisfying signature holding a Keychain entitlement is refused, whatever
        // its value.
        for value in [Some(true), Some(false), None] {
            let held = hardened_with(&[("keychain-access-groups", value)]);
            assert!(!rule.allows(&held), "{value:?}");
            assert!(crate::code_signing::hardened_and_clean(&held), "{value:?}");
        }
        let refusal = check.why(&ImageRefusal::NotTheImage);
        assert!(refusal.contains("web process"), "{refusal}");
        assert!(refusal.contains("Keychain entitlement"), "{refusal}");
        // The office server's check never asks about the Keychain entitlement.
        let office = ServerCheck::Requirement("R".into())
            .expected(PROGRAM.as_ref())
            .unwrap();
        assert_eq!(office.macos_signing, HARDENED);
        assert!(office.macos_signing.absent_entitlements.is_empty());
        assert!(office
            .macos_signing
            .allows(&hardened_with(&[("keychain-access-groups", Some(true))])));
    }

    #[test]
    fn a_child_is_admitted_only_with_a_token_that_satisfies_the_requirement() {
        let check = ServerCheck::Requirement("R".into());
        let expected = check.expected(PROGRAM.as_ref()).unwrap();
        // The platform is asked for exactly this requirement and the hardening rule, with no
        // pin, and never as a development image.
        assert_eq!(expected.macos_requirement.as_deref(), Some("R"));
        assert_eq!(expected.macos_signing, HARDENED);
        assert_eq!(expected.macos_cdhash, None);
        assert!(!expected.development);
        // Hardened code with a rejected entitlement, or code that isn't hardened, is refused.
        assert!(expected.macos_signing.allows(&hardened_with(&[])));
        for key in crate::code_signing::REJECTED_ENTITLEMENTS {
            assert!(
                !expected
                    .macos_signing
                    .allows(&hardened_with(&[(key, Some(true))])),
                "{key}"
            );
        }
        assert!(!expected
            .macos_signing
            .allows(&crate::code_signing::Signing::default()));
        assert_eq!(
            check.why(&ImageRefusal::NotTheImage),
            "it isn't the signed office server (its code signature doesn't satisfy the \
             requirement, or it isn't hardened)"
        );
        // No identity: refused, with the system's reason.
        let err = check.why(&ImageRefusal::Identity("gone".into()));
        assert_eq!(err, "its identity could not be read: gone");
        // Debug skips: a development image with no requirement. No team id refuses: no image.
        let skipped = ServerCheck::Skip.expected(PROGRAM.as_ref()).unwrap();
        assert!(skipped.development);
        assert_eq!(skipped.macos_requirement, None);
        assert_eq!(skipped.macos_cdhash, None);
        assert_eq!(
            ServerCheck::Refuse.expected(PROGRAM.as_ref()),
            Err("this build has no app team id to check servers with".into())
        );
    }

    #[test]
    fn a_child_that_exec_ed_after_the_check_is_refused() {
        let check = ServerCheck::Requirement("R".into());
        assert_eq!(
            check.why(&ImageRefusal::Changed),
            "it changed (exec'ed) after it was checked"
        );
        assert_eq!(
            check.why(&ImageRefusal::Identity("gone".into())),
            "its identity could not be read: gone"
        );
    }

    /// Every refusal has words for the host log, and a refusal that a system without code
    /// signatures can give names nothing that only another system has.
    #[test]
    fn every_refusal_is_worded_and_the_common_ones_name_no_system() {
        let common = [
            ImageRefusal::Identity("the child has exited".into()),
            ImageRefusal::DigestMismatch,
            ImageRefusal::Changed,
            ImageRefusal::Readmitted,
            ImageRefusal::Unpinned,
            ImageRefusal::UnsafeFile("is writable by its group or by others"),
        ];
        let signed_only = [
            ImageRefusal::NotTheImage,
            ImageRefusal::Unidentified,
            ImageRefusal::DigestUnavailable,
        ];
        for check in [
            ServerCheck::Skip,
            ServerCheck::Requirement("R".into()),
            ServerCheck::Gateway("W".into()),
            ServerCheck::Published,
            ServerCheck::Skip.pinned(pin(7)),
            ServerCheck::Published.pinned(pin(7)),
        ] {
            for refusal in &common {
                let words = check.why(refusal);
                assert!(!words.is_empty(), "{refusal:?}");
                for foreign in [
                    "audit token",
                    "signed office",
                    "signed web",
                    "signature",
                    "Keychain",
                    "hardened",
                    "kernel",
                ] {
                    assert!(!words.contains(foreign), "{refusal:?}: {words}");
                }
            }
            for refusal in &signed_only {
                assert!(!check.why(refusal).is_empty(), "{refusal:?}");
            }
        }
        let check = ServerCheck::Requirement("R".into());
        assert_eq!(
            check.why(&ImageRefusal::DigestMismatch),
            "selected release image mismatch"
        );
        let unpinned = check.why(&ImageRefusal::Unpinned);
        assert!(unpinned.contains("no selected release"), "{unpinned}");
        let unsafe_file = check.why(&ImageRefusal::UnsafeFile("is owned by another user"));
        assert!(
            unsafe_file.starts_with("its program file is owned by another user"),
            "{unsafe_file}"
        );
    }

    #[test]
    fn a_selected_release_pins_the_image_on_top_of_the_build_check() {
        let hash = pin(7);
        for check in [
            ServerCheck::Skip,
            ServerCheck::Requirement("R".into()),
            ServerCheck::Gateway("W".into()),
        ] {
            let plain = check.expected(PROGRAM.as_ref()).unwrap();
            // Without a release nothing names a digest of either kind.
            assert_eq!(
                (plain.sha256, plain.macos_cdhash),
                (None, None),
                "{check:?}"
            );
            let pinned = check
                .clone()
                .pinned(hash)
                .expected(PROGRAM.as_ref())
                .unwrap();
            // Both of the manifest's identities reach the platform, on top of the build's
            // own check: each system's admission reads its own.
            assert_eq!(
                pinned,
                ExpectedImage {
                    sha256: Some([7; 32]),
                    macos_cdhash: Some([7; 20]),
                    ..plain.clone()
                },
                "{check:?}"
            );
            // A manifest for a system without CodeDirectory hashes names the digest alone.
            let digest_only = Pin {
                sha256: [7; 32],
                macos_cdhash: None,
            };
            assert_eq!(
                check
                    .clone()
                    .pinned(digest_only)
                    .expected(PROGRAM.as_ref())
                    .unwrap(),
                ExpectedImage {
                    sha256: Some([7; 32]),
                    ..plain
                },
                "{check:?}"
            );
            // The same pin twice is one pin; two different pins can never both hold,
            // whichever identity differs.
            assert_eq!(
                check
                    .clone()
                    .pinned(hash)
                    .pinned(hash)
                    .expected(PROGRAM.as_ref()),
                Ok(pinned)
            );
            for other in [
                pin(8),
                digest_only,
                Pin {
                    sha256: [8; 32],
                    ..hash
                },
                Pin {
                    macos_cdhash: Some([8; 20]),
                    ..hash
                },
            ] {
                assert_eq!(
                    check
                        .clone()
                        .pinned(hash)
                        .pinned(other)
                        .expected(PROGRAM.as_ref()),
                    Err("selected release image mismatch".into()),
                    "{check:?} {other:?}"
                );
            }
        }
        let check = ServerCheck::Skip.pinned(hash);
        for (refusal, text) in [
            (
                ImageRefusal::Unidentified,
                "selected release requires platform-audited image",
            ),
            (
                ImageRefusal::DigestUnavailable,
                "selected release kernel image unavailable",
            ),
            (
                ImageRefusal::DigestMismatch,
                "selected release image mismatch",
            ),
        ] {
            assert_eq!(check.why(&refusal), text);
        }
        // A pinned web check still words its refusal as the web process's.
        assert!(ServerCheck::Gateway("W".into())
            .pinned(hash)
            .why(&ImageRefusal::NotTheImage)
            .contains("web process"));
    }

    /// A scripted check (tests only) may spawn, names a development image, keeps a pin, and
    /// gets an admission exactly where the platform has an adapter to start children with.
    #[test]
    fn a_scripted_check_stands_in_for_the_publisher_check_only() {
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = std::sync::Arc::clone(&asked);
        let check = ServerCheck::Scripted(Scripted(std::sync::Arc::new(
            move |step: Step, pid: u32| -> Result<Option<AuditToken>, String> {
                seen.lock().unwrap().push((step, pid));
                Ok(None)
            },
        )));
        assert!(check.may_spawn().is_ok());
        assert_eq!(check.describe(), "scripted (tests)");
        assert_eq!(check, check.clone());
        let expected = check.expected(PROGRAM.as_ref()).unwrap();
        assert!(expected.development && expected.macos_requirement.is_none());
        let pinned = check.clone().pinned(pin(7));
        assert_eq!(
            pinned.expected(PROGRAM.as_ref()).unwrap().macos_cdhash,
            Some([7; 20])
        );
        assert!(pinned.scripted().is_some());
        assert!(ServerCheck::Skip.scripted().is_none());
        // A scripted admission is built as the system's own is: of a program that is not
        // there, at once where the image is checked on the started child (nothing is read
        // yet), and not at all where the bytes are read when the admission is built (the
        // system's error, never a refusal of the check).
        for check in [check.clone(), ServerCheck::Skip] {
            match (identity(), check.admission(PROGRAM.as_ref())) {
                (ImageIdentity::CodeSignature, Ok(_)) => {}
                (ImageIdentity::VerifiedBytes, Err(NoAdmission::Unavailable(error))) => {
                    assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error:?}");
                }
                (identity, other) => panic!("{identity:?}: {:?}", other.map(drop)),
            }
        }
        // Building the admission started nothing and asked the script nothing.
        assert!(asked.lock().unwrap().is_empty());
        assert_eq!(
            check.why(&ImageRefusal::Identity("no token".into())),
            "its identity could not be read: no token"
        );
        let _ = [Step::Admit, Step::Still];
    }

    /// The by-path check (tests only) starts a development image by its path: it may spawn,
    /// names no digest, keeps a pin, and gets an admission without the image being read, on
    /// every system with an adapter.
    #[test]
    fn a_by_path_check_names_a_development_image_and_reads_nothing() {
        let check = ServerCheck::ByPath;
        assert!(check.may_spawn().is_ok());
        assert!(check.by_path() && check.clone().pinned(pin(7)).by_path());
        assert!(!ServerCheck::Skip.by_path());
        let expected = check.expected(PROGRAM.as_ref()).unwrap();
        assert!(expected.development && expected.sha256.is_none());
        // Nothing at the path is read when the admission is built: the program is opened
        // only when a child is started.
        let admission = check
            .admission(PROGRAM.as_ref())
            .unwrap_or_else(|none| panic!("{none}"));
        assert_eq!(admission.image_sha256(), None);
        assert_eq!(check.describe_admitted(PROGRAM.as_ref(), &admission), None);
    }

    /// A debug build's admission of a real program (a compiled test worker). Where the system
    /// executes the bytes it read, the admission reports their SHA-256, which is the file's,
    /// and the host's log line names it. Where the loaded image is checked instead there is
    /// no digest and nothing more to log.
    #[test]
    fn a_development_admission_reports_the_digest_of_the_bytes_it_read() {
        let me = crate::test_programs::recording_worker();
        let check = ServerCheck::Skip;
        let admission = check.admission(&me).unwrap_or_else(|none| panic!("{none}"));
        let line = check.describe_admitted(&me, &admission);
        match identity() {
            ImageIdentity::VerifiedBytes => {
                let digest = admission
                    .image_sha256()
                    .expect("the digest of the bytes read");
                assert_eq!(digest, sha256_of(&me));
                let line = line.expect("a development image's digest is logged");
                assert!(line.contains(&hex(&digest)), "{line}");
                assert!(line.contains("no publisher check"), "{line}");
                assert!(line.contains(me.to_str().unwrap()), "{line}");
            }
            ImageIdentity::CodeSignature => {
                assert_eq!(admission.image_sha256(), None);
                assert_eq!(line, None);
            }
        }
        // A pinned image is the release's, not a development image: nothing more is logged,
        // on either kind of system.
        let pinned = ServerCheck::Skip.pinned(Pin {
            sha256: sha256_of(&me),
            macos_cdhash: None,
        });
        let admission = pinned
            .admission(&me)
            .unwrap_or_else(|none| panic!("{none}"));
        assert_eq!(pinned.describe_admitted(&me, &admission), None);
    }

    /// A pinned check never starts a program that is not the pinned image, whatever the
    /// build. A system that executes verified bytes refuses when the admission is built (the
    /// digest differs), so no child exists. A system that checks the loaded image builds its
    /// admission, starts the child held, cannot identify a development image, and kills it.
    #[test]
    fn a_pinned_check_never_starts_an_image_that_is_not_the_pinned_one() {
        use org_platform_host::admission::{ChildEnv, SpawnRefused};
        let me = crate::test_programs::recording_worker();
        let mut other = sha256_of(&me);
        other[0] ^= 1;
        let check = ServerCheck::Skip.pinned(Pin {
            sha256: other,
            macos_cdhash: Some([0; 20]),
        });
        match (identity(), check.admission(&me)) {
            (ImageIdentity::VerifiedBytes, Err(NoAdmission::Refused(why))) => {
                assert_eq!(why, "selected release image mismatch");
            }
            (ImageIdentity::CodeSignature, Ok(admission)) => {
                let error = admission
                    .start(&[], &ChildEnv::new(), &crate::cloexec::SPAWN_LOCK)
                    .err()
                    .expect("an image that is not the pinned one was started");
                let refused = SpawnRefused::of(&error).expect("an image refusal");
                assert_eq!(refused.refusal, ImageRefusal::Unidentified);
                // SAFETY: signal 0 only checks the pid.
                assert!(
                    unsafe { libc::kill(refused.pid as libc::pid_t, 0) } != 0,
                    "the refused child was left behind"
                );
            }
            (identity, other) => panic!("{identity:?}: {:?}", other.map(drop)),
        }
        // The program under the digest of its own bytes has an admission on both kinds of
        // system; where the digest is what is checked, the admission reports that digest.
        let exact = ServerCheck::Skip.pinned(Pin {
            sha256: sha256_of(&me),
            macos_cdhash: None,
        });
        let admission = exact.admission(&me).unwrap_or_else(|none| panic!("{none}"));
        assert_eq!(
            admission.image_sha256(),
            match identity() {
                ImageIdentity::VerifiedBytes => Some(sha256_of(&me)),
                ImageIdentity::CodeSignature => None,
            }
        );
    }

    /// A release build's requirement check without a selected release. Where a code
    /// signature is checked, the admission exists and reads nothing yet. Where the executed
    /// bytes are checked there is no admission, in words that say what is missing.
    #[test]
    fn a_release_check_without_a_selected_release_is_worded_where_it_refuses() {
        let me = crate::test_programs::recording_worker();
        let check = ServerCheck::Requirement(server_requirement(TEAM).unwrap());
        match (identity(), check.admission(&me)) {
            (ImageIdentity::CodeSignature, Ok(admission)) => {
                assert_eq!(admission.image_sha256(), None);
            }
            (ImageIdentity::VerifiedBytes, Err(NoAdmission::Refused(why))) => {
                assert_eq!(why, check.why(&ImageRefusal::Unpinned));
                assert!(why.contains("no selected release"), "{why}");
            }
            (identity, other) => panic!("{identity:?}: {:?}", other.map(drop)),
        }
    }

    /// A release build with no team id, through the system's own admission and a compiled
    /// program (the decision of a system that checks the executed bytes, [`Published`]).
    ///
    /// Where the executed bytes are checked: a pin whose digest is the program's admits it,
    /// and a child runs; a pin with another digest refuses; no pin refuses, each before any
    /// child exists. Where a code signature is checked, this check names no requirement to
    /// check a release image against, so the platform starts nothing in any of the three
    /// cases (its error is not an image refusal: no child existed to refuse); there a
    /// release build without a team id is never given this check at all (`for_system`
    /// answers `Refuse`).
    ///
    /// [`Published`]: ServerCheck::Published
    #[test]
    fn a_release_build_without_a_team_admits_only_the_image_the_release_pins() {
        use org_platform_host::admission::{ChildEnv, SpawnRefused};
        use std::io::Read;
        let me = crate::test_programs::recording_worker();
        let digest = sha256_of(&me);
        let mut other = digest;
        other[31] ^= 1;
        let bytes_pin = |sha256| Pin {
            sha256,
            macos_cdhash: None,
        };
        let matching = ServerCheck::Published.pinned(bytes_pin(digest));
        let mismatching = ServerCheck::Published.pinned(bytes_pin(other));
        let unpinned = ServerCheck::Published;
        assert_eq!(
            ServerCheck::for_system(false, None, Some(ImageIdentity::VerifiedBytes)),
            Ok(unpinned.clone())
        );
        let home = tempfile::tempdir().unwrap();
        let mut env = ChildEnv::new();
        env.set("ORG_OFFICE_HOME", home.path()).unwrap();
        match identity() {
            ImageIdentity::VerifiedBytes => {
                let admission = matching
                    .admission(&me)
                    .unwrap_or_else(|none| panic!("the pinned image was refused: {none}"));
                assert_eq!(admission.image_sha256(), Some(digest));
                let mut started = admission
                    .spawn(&[], &env, &crate::cloexec::SPAWN_LOCK)
                    .expect("the pinned image starts");
                drop(started.stdin);
                let mut rest = Vec::new();
                started.stdout.read_to_end(&mut rest).unwrap();
                assert_eq!(crate::test_programs::recorded_starts(home.path()), 1);
                started.child.kill();
                match mismatching.admission(&me) {
                    Err(NoAdmission::Refused(why)) => {
                        assert_eq!(why, "selected release image mismatch");
                    }
                    other => panic!("{:?}", other.map(drop)),
                }
                match unpinned.admission(&me) {
                    Err(NoAdmission::Refused(why)) => {
                        assert_eq!(why, unpinned.why(&ImageRefusal::Unpinned));
                    }
                    other => panic!("{:?}", other.map(drop)),
                }
            }
            ImageIdentity::CodeSignature => {
                for check in [&matching, &mismatching, &unpinned] {
                    let admission = check
                        .admission(&me)
                        .unwrap_or_else(|none| panic!("{check:?}: {none}"));
                    let error = admission
                        .spawn(&[], &env, &crate::cloexec::SPAWN_LOCK)
                        .err()
                        .unwrap_or_else(|| panic!("{check:?}: an unsigned image was started"));
                    // A release image with no requirement: refused before anything is
                    // started, so there is no child and no image refusal to name.
                    assert_eq!(
                        error.kind(),
                        io::ErrorKind::InvalidInput,
                        "{check:?}: {error}"
                    );
                    assert!(SpawnRefused::of(&error).is_none(), "{check:?}: {error}");
                    assert_eq!(
                        crate::test_programs::recorded_starts(home.path()),
                        0,
                        "{check:?}"
                    );
                }
            }
        }
        // Nothing but the one admitted child ever ran.
        assert_eq!(
            crate::test_programs::recorded_starts(home.path()),
            match identity() {
                ImageIdentity::VerifiedBytes => 1,
                ImageIdentity::CodeSignature => 0,
            }
        );
    }

    /// A release check and a real program that isn't the office server (`/bin/sleep`): it is
    /// never started as one. Where the loaded image is checked, the admission exists, the
    /// child is started held (its loader hasn't run), refused with the office server's
    /// refusal, killed and reaped. Where the verified bytes are executed, a release check
    /// without a selected release has no admission at all, so no child ever exists.
    #[test]
    fn a_real_child_that_isnt_the_signed_server_is_refused() {
        use org_platform_host::admission::{ChildEnv, SpawnRefused};
        let check = ServerCheck::Requirement(server_requirement(TEAM).unwrap());
        let args = [std::ffi::OsStr::new("30")];
        match (identity(), check.admission("/bin/sleep".as_ref())) {
            (ImageIdentity::CodeSignature, Ok(admission)) => {
                let error = admission
                    .spawn(&args, &ChildEnv::new(), &crate::cloexec::SPAWN_LOCK)
                    .err()
                    .expect("/bin/sleep was admitted as the office server");
                let refused = SpawnRefused::of(&error).expect("an image refusal");
                assert_eq!(refused.refusal, ImageRefusal::NotTheImage);
                assert!(check.why(&refused.refusal).contains("office server"));
                // SAFETY: signal 0 only checks the pid.
                assert!(
                    unsafe { libc::kill(refused.pid as libc::pid_t, 0) } != 0,
                    "the refused child was left behind"
                );
            }
            (ImageIdentity::VerifiedBytes, Err(NoAdmission::Refused(why))) => {
                assert_eq!(why, check.why(&ImageRefusal::Unpinned));
            }
            (identity, other) => panic!("{identity:?}: {:?}", other.map(drop)),
        }
    }
}
