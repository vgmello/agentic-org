//! Who may get an office's database key: the check of every office server the host starts,
//! before its first line (which carries the key) is written.
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

/// The office worker's signing identifier (ADR-0003 §2.1): a release host gives an office's key
/// only to an office worker signed with it by the app's team (`server_check`). The bundle must sign
/// agentic-org-office with it. The signing identifier is stable across filename changes.
pub const OFFICE_SIGNING_ID: &str = "com.vgmello.agentic-org.office";
/// The browser service has no office-worker or Keychain authority (ADR-0008).
pub const WEB_SIGNING_ID: &str = "com.vgmello.agentic-org.web";

use std::io;
use std::path::Path;

use org_platform_host::admission::{Admission, ExpectedImage, ImageRefusal, SigningRule};

use crate::code_signing::HARDENED;
#[cfg(test)]
use crate::peer::AuditToken;

#[cfg(all(test, target_os = "macos"))]
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
    /// Same platform policy plus the selected publisher-signed exact image.
    Pinned(Box<ServerCheck>, [u8; 20]),
    /// No team id: no server may be started.
    Refuse,
    /// The web process may not be started, and why (for the host log).
    WebRefused(&'static str),
    /// Tests only: the admission and the re-check's token come from the test.
    #[cfg(test)]
    Scripted(Scripted),
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
    pub(crate) fn pinned(self, hash: [u8; 20]) -> Self {
        Self::Pinned(Box::new(self), hash)
    }
    pub(crate) fn gateway_for_build(debug: bool, team_id: Option<&str>) -> Result<Self, String> {
        if debug {
            return Ok(Self::Skip);
        }
        match team_id {
            Some(team) => crate::code_signing::developer_id_requirement(WEB_SIGNING_ID, team)
                .map(Self::Gateway),
            None => Ok(Self::WebRefused(
                "no app team id (ORG_APP_TEAM_ID at build time)",
            )),
        }
    }
    /// The check for a build (`debug`) whose app team id is `team_id`. A team id that isn't well
    /// formed is an error, never a weaker check.
    pub(crate) fn for_build(debug: bool, team_id: Option<&str>) -> Result<Self, String> {
        if debug {
            return Ok(ServerCheck::Skip);
        }
        match team_id {
            Some(team_id) => Ok(ServerCheck::Requirement(server_requirement(team_id)?)),
            None => Ok(ServerCheck::Refuse),
        }
    }

    /// What the host logs about the check at startup.
    pub(crate) fn describe(&self) -> String {
        match self {
            ServerCheck::Pinned(check, _) => format!(
                "{}; exact selected release kernel image required",
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
            ServerCheck::Refuse => "WARNING: no app team id (ORG_APP_TEAM_ID at build time), so \
                                    no office server's signature can be checked: no office \
                                    server will be started"
                .to_owned(),
            ServerCheck::WebRefused(cause) => {
                format!("WARNING: the web process will not be started: {cause}")
            }
            #[cfg(test)]
            ServerCheck::Scripted(_) => "scripted (tests)".to_owned(),
        }
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
            ServerCheck::Gateway(_) => Ok(()),
            #[cfg(test)]
            ServerCheck::Scripted(_) => Ok(()),
        }
    }

    /// What a child started from `program` must be, as the data the platform checks, or why
    /// no child may be started at all (as [`ServerCheck::may_spawn`] says).
    ///
    /// A debug build's servers are its own build output: no publisher check. A release
    /// build's office server must satisfy the requirement and the [`HARDENED`] rule; its web
    /// process, in addition, may hold no Keychain entitlement. A selected release pins the
    /// exact image on top of either.
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
            ServerCheck::Pinned(check, hash) => {
                let inner = check.expected(program)?;
                if inner.macos_cdhash.is_some_and(|other| other != *hash) {
                    // Two different pins can never both hold.
                    return Err("selected release kernel image mismatch".into());
                }
                Ok(ExpectedImage {
                    macos_cdhash: Some(*hash),
                    ..inner
                })
            }
            ServerCheck::Skip => Ok(image(true)),
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
            ServerCheck::Scripted(_) => Ok(image(true)),
        }
    }

    /// The platform's admission for this check's servers started from `program`: it starts
    /// each child, checks its image before any of it has run, and re-checks it later. A check
    /// that may not spawn has none, and neither has a system without an admission adapter
    /// (an [`io::ErrorKind::Unsupported`] error): no server is started there.
    pub(crate) fn admission(&self, program: &Path) -> io::Result<Admission> {
        let expected = self.expected(program).map_err(io::Error::other)?;
        #[cfg(test)]
        if let Some(scripted) = self.scripted() {
            use org_platform_host::admission::Peer;
            let script = scripted.0.clone();
            return Ok(org_platform_host::testing::admission::scripted_admission(
                expected,
                std::sync::Arc::new(move |step, pid| {
                    script(step, pid).map(|token| token.map(Peer::from_macos_audit_token))
                }),
            )?);
        }
        Ok(org_platform_host::admission::admission(expected)?)
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

    /// Whether this is the web process's check (its refusals name the web process).
    fn is_web(&self) -> bool {
        match self {
            ServerCheck::Gateway(_) | ServerCheck::WebRefused(_) => true,
            ServerCheck::Pinned(check, _) => check.is_web(),
            _ => false,
        }
    }

    /// Why the platform refused a child, in this check's words for the host log.
    pub(crate) fn why(&self, refusal: &ImageRefusal) -> String {
        match refusal {
            ImageRefusal::Identity(why) => format!("its audit token: {why}"),
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
            ImageRefusal::DigestMismatch => "selected release kernel image mismatch".into(),
            ImageRefusal::Changed => "it changed (exec'ed) after it was checked".into(),
            other => other.to_string(),
        }
    }
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

    #[test]
    fn the_check_follows_the_build() {
        assert_eq!(
            ServerCheck::for_build(true, Some(TEAM)),
            Ok(ServerCheck::Skip)
        );
        assert_eq!(ServerCheck::for_build(true, None), Ok(ServerCheck::Skip));
        assert_eq!(
            ServerCheck::for_build(false, Some(TEAM)),
            Ok(ServerCheck::Requirement(server_requirement(TEAM).unwrap()))
        );
        assert_eq!(ServerCheck::for_build(false, None), Ok(ServerCheck::Refuse));
        assert!(ServerCheck::for_build(false, Some("ab12")).is_err());
        assert!(ServerCheck::Refuse.may_spawn().is_err());
        assert!(ServerCheck::Refuse.describe().starts_with("WARNING"));
        assert!(ServerCheck::Skip.may_spawn().is_ok());
    }

    #[test]
    fn release_web_without_a_team_may_not_spawn() {
        let check = ServerCheck::gateway_for_build(false, None).unwrap();
        assert!(matches!(check, ServerCheck::WebRefused(_)));
        assert!(check.may_spawn().is_err());
        assert!(check.describe().contains("web process"));
        assert!(check.describe().contains("no app team id"));
        assert!(ServerCheck::gateway_for_build(true, None)
            .unwrap()
            .may_spawn()
            .is_ok());
    }

    #[test]
    fn gateway_requires_its_distinct_identity_and_never_a_missing_team() {
        let ServerCheck::Gateway(requirement) =
            ServerCheck::gateway_for_build(false, Some(TEAM)).unwrap()
        else {
            panic!("release gateway policy")
        };
        assert!(requirement.contains(WEB_SIGNING_ID));
        assert!(!requirement.contains(OFFICE_SIGNING_ID));
        assert!(matches!(
            ServerCheck::gateway_for_build(false, None).unwrap(),
            ServerCheck::WebRefused(_)
        ));
        assert!(ServerCheck::gateway_for_build(false, Some("bad")).is_err());
        assert_eq!(
            ServerCheck::gateway_for_build(true, None).unwrap(),
            ServerCheck::Skip
        );
        // A missing team names no image, so no admission exists to ask anything of.
        for refused in [
            ServerCheck::Refuse,
            ServerCheck::gateway_for_build(false, None).unwrap(),
        ] {
            assert!(refused.expected(PROGRAM.as_ref()).is_err(), "{refused:?}");
            assert!(refused.admission(PROGRAM.as_ref()).is_err(), "{refused:?}");
            // A pin on top of it changes nothing.
            let pinned = refused.pinned([7; 20]);
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
    #[cfg(target_os = "macos")]
    #[test]
    fn production_web_admission_uses_the_live_keychain_probe() {
        use org_platform_host::admission::{ChildEnv, SpawnRefused};
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
        let ServerCheck::Gateway(requirement) =
            ServerCheck::gateway_for_build(false, Some(TEAM)).unwrap()
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
        // No token: refused, in the words the host always logged.
        let err = check.why(&ImageRefusal::Identity("gone".into()));
        assert_eq!(err, "its audit token: gone");
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
            "its audit token: gone"
        );
    }

    #[test]
    fn a_selected_release_pins_the_image_on_top_of_the_build_check() {
        let hash = [7u8; 20];
        for check in [
            ServerCheck::Skip,
            ServerCheck::Requirement("R".into()),
            ServerCheck::Gateway("W".into()),
        ] {
            let plain = check.expected(PROGRAM.as_ref()).unwrap();
            let pinned = check
                .clone()
                .pinned(hash)
                .expected(PROGRAM.as_ref())
                .unwrap();
            assert_eq!(
                pinned,
                ExpectedImage {
                    macos_cdhash: Some(hash),
                    ..plain
                },
                "{check:?}"
            );
            // The same pin twice is one pin; two different pins can never both hold.
            assert_eq!(
                check
                    .clone()
                    .pinned(hash)
                    .pinned(hash)
                    .expected(PROGRAM.as_ref()),
                Ok(pinned)
            );
            assert!(check
                .clone()
                .pinned(hash)
                .pinned([8; 20])
                .expected(PROGRAM.as_ref())
                .is_err());
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
                "selected release kernel image mismatch",
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
        let pinned = check.clone().pinned([7; 20]);
        assert_eq!(
            pinned.expected(PROGRAM.as_ref()).unwrap().macos_cdhash,
            Some([7; 20])
        );
        assert!(pinned.scripted().is_some());
        assert!(ServerCheck::Skip.scripted().is_none());
        // Whether a production admission exists decides whether a scripted one does.
        assert_eq!(
            check.admission(PROGRAM.as_ref()).is_ok(),
            ServerCheck::Skip.admission(PROGRAM.as_ref()).is_ok()
        );
        // No admission here means no adapter, or an adapter that reads the image when the
        // admission is built and found none at this path.
        if let Err(error) = check.admission(PROGRAM.as_ref()) {
            assert!(
                matches!(
                    error.kind(),
                    io::ErrorKind::Unsupported | io::ErrorKind::NotFound
                ),
                "{error:?}"
            );
        }
        // Building the admission started nothing and asked the script nothing.
        assert!(asked.lock().unwrap().is_empty());
        assert_eq!(
            check.why(&ImageRefusal::Identity("no token".into())),
            "its audit token: no token"
        );
        let _ = [Step::Admit, Step::Still];
    }

    /// The live check on a child as the host checks it: started suspended, so its loader
    /// hasn't run. `/bin/sleep` (Apple's, not the office server) is refused, killed and
    /// reaped before it ran, with the office server's refusal.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_real_child_that_isnt_the_signed_server_is_refused() {
        use org_platform_host::admission::{ChildEnv, SpawnRefused};
        let check = ServerCheck::Requirement(server_requirement(TEAM).unwrap());
        let args = [std::ffi::OsStr::new("30")];
        let error = check
            .admission("/bin/sleep".as_ref())
            .unwrap()
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
}
