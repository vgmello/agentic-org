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

pub use org_host_release::signing::{OFFICE_SIGNING_ID, WEB_SIGNING_ID};

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
    #[cfg_attr(
        not(debug_assertions),
        allow(
            dead_code,
            reason = "ServerCheck::Skip is constructed only in debug builds and tests"
        )
    )]
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

pub(crate) use org_host_release::Pin;

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
mod tests;
