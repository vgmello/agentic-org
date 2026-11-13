//! Who may get an office's database key: the check of every office server the host starts,
//! before it learns where to fetch its keys (launch v3: the first line names a rendezvous, made
//! only after this check, and the keys go only to the admitted token's connection).
//!
//! The threat: a same-user process runs the signed host from a copy, a symlink or a hard link,
//! next to its own `org-server` (the host starts the server next to its own executable),
//! and receives every office's key. So a release host checks each server it
//! spawns, before that server has run a single instruction: it starts it **suspended**
//! (`spawn::spawn_suspended`; otherwise the image's first code could start a helper that
//! inherits stdin, then `exec` the genuine server and pass the check), and checks it by its
//! **audit token**, which names the image the exec loaded: its code object
//! (`SecCodeCopyGuestWithAttributes`) must satisfy a requirement compiled into the host,
//! the server's signing identifier [`SERVER_SIGNING_ID`] signed with the app team's Developer
//! ID, and run hardened code without the entitlements that let another process reach into it
//! (`code_signing::satisfies_hardened`):
//!
//! ```text
//! identifier "com.ellosoft.agentic-org.org-server" and anchor apple generic
//!   and certificate 1[field.1.2.840.113635.100.6.2.6] exists
//!   and certificate leaf[field.1.2.840.113635.100.6.1.13] exists
//!   and certificate leaf[subject.OU] = "<ORG_APP_TEAM_ID>"
//! ```
//!
//! An early resumer (another same-user process that resumes the child before the host takes its
//! token, see `spawn`'s module docs) can still leave a helper holding stdin, but stdin carries
//! no secret now: the keys go to the rendezvous connection whose peer token is the admitted one
//! (`supervisor::rendezvous`).
//!
//! Only a child that passes is resumed. Right before the first line is written, its audit token
//! is taken again and must be unchanged, so an `exec` after the check is refused. A child that
//! fails, or whose token can't be taken, is killed and reaped (still suspended if it failed the
//! first check, so it never ran), and gets nothing; the start counts as a `storage_error`
//! (retried with backoff, up to the give-up rule).
//!
//! A release host with no team id compiled in can't check anything, so it starts no server at
//! all (logged loudly). **Debug builds skip only the signing requirement** (their servers are
//! unsigned development and test builds), which is logged at startup: the token is still taken,
//! re-checked and matched to the rendezvous peer.

/// The office server's signing identifier (ADR-0003 §2.1): a release host gives an office's key
/// only to a server signed with it by the app's team (`server_check`). The bundle must sign
/// `Contents/MacOS/org-server` with it.
pub const SERVER_SIGNING_ID: &str = "com.ellosoft.agentic-org.org-server";

use crate::peer::AuditToken;

/// How the host checks the office servers it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServerCheck {
    /// Debug builds: no signing requirement; the token is still taken and checked (X-02 §5.1
    /// step 6).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Skip,
    /// Every server must satisfy this requirement and run hardened code.
    Requirement(String),
    /// No team id: no server may be started.
    Refuse,
    /// Tests only: the admission and the re-check's token come from the test.
    #[cfg(test)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Scripted(Scripted),
}

/// Which step a [`Scripted`] check is asked for.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// [`ServerCheck::admit`]: the admitted token, or a refusal.
    Admit,
    /// [`ServerCheck::still`]: the child's token now.
    Still,
}

/// A test's stand-in for the live check: called with the step and the child's pid.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct Scripted(
    pub(crate) std::sync::Arc<dyn Fn(Step, u32) -> Result<AuditToken, String> + Send + Sync>,
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
            ServerCheck::Skip => "debug build: office servers' code signatures aren't checked \
                                  (their audit tokens are) before they get an office's key"
                .to_owned(),
            ServerCheck::Requirement(requirement) => format!(
                "office servers get an office's key only if they run hardened code satisfying: \
                 {requirement}"
            ),
            ServerCheck::Refuse => "WARNING: no app team id (ORG_APP_TEAM_ID at build time), so \
                                    no office server's signature can be checked: no office \
                                    server will be started"
                .to_owned(),
            #[cfg(test)]
            ServerCheck::Scripted(_) => "scripted (tests)".to_owned(),
        }
    }

    /// Before a spawn: whether any server may be started.
    pub(crate) fn may_spawn(&self) -> Result<(), String> {
        match self {
            ServerCheck::Refuse => {
                Err("this build has no app team id to check servers with".into())
            }
            ServerCheck::Skip | ServerCheck::Requirement(_) => Ok(()),
            #[cfg(test)]
            ServerCheck::Scripted(_) => Ok(()),
        }
    }

    /// After the spawn of child `pid`: its audit token if it may get a key, or why not.
    pub(crate) fn admit(&self, pid: u32) -> Result<AuditToken, String> {
        #[cfg(test)]
        if let ServerCheck::Scripted(scripted) = self {
            return (scripted.0)(Step::Admit, pid);
        }
        admit(self, || token_of(pid), satisfies)
    }

    /// Right before child `pid`'s first line: whether it is still the image `admitted` (what
    /// [`ServerCheck::admit`] returned).
    pub(crate) fn still(&self, pid: u32, admitted: AuditToken) -> Result<(), String> {
        #[cfg(test)]
        if let ServerCheck::Scripted(scripted) = self {
            return unchanged(admitted, (scripted.0)(Step::Still, pid));
        }
        unchanged(admitted, token_of(pid))
    }
}

/// The requirement an office server must satisfy: [`SERVER_SIGNING_ID`] signed with the
/// Developer ID of team `team_id`.
pub(crate) fn server_requirement(team_id: &str) -> Result<String, String> {
    crate::code_signing::developer_id_requirement(SERVER_SIGNING_ID, team_id)
}

/// The decision, pure: `token` gives the child's audit token, `satisfies` checks the code it
/// names against a requirement. Neither is asked unless the check needs it.
fn admit(
    check: &ServerCheck,
    token: impl FnOnce() -> Result<AuditToken, String>,
    satisfies: impl FnOnce(&AuditToken, &str) -> bool,
) -> Result<AuditToken, String> {
    match check {
        ServerCheck::Skip => token().map_err(|why| format!("its audit token: {why}")),
        ServerCheck::Refuse => Err("this build has no app team id to check servers with".into()),
        #[cfg(test)]
        ServerCheck::Scripted(_) => Err("a scripted check isn't decided here".into()),
        ServerCheck::Requirement(requirement) => {
            let token = token().map_err(|why| format!("its audit token: {why}"))?;
            if satisfies(&token, requirement) {
                Ok(token)
            } else {
                Err(
                    "it isn't the signed office server (its code signature doesn't satisfy the \
                     requirement, or it isn't hardened)"
                        .into(),
                )
            }
        }
    }
}

/// Whether the child's token `now` is still the one admitted.
fn unchanged(admitted: AuditToken, now: Result<AuditToken, String>) -> Result<(), String> {
    match now {
        Ok(now) if now == admitted => Ok(()),
        Ok(_) => Err("it changed (exec'ed) after it was checked".into()),
        Err(why) => Err(format!("its audit token: {why}")),
    }
}

#[cfg(target_os = "macos")]
fn token_of(pid: u32) -> Result<AuditToken, String> {
    crate::spawn::audit_token_of_child(pid).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn satisfies(token: &AuditToken, requirement: &str) -> bool {
    crate::code_signing::satisfies_hardened(token, requirement)
}

#[cfg(not(target_os = "macos"))]
fn token_of(_pid: u32) -> Result<AuditToken, String> {
    Err("no audit tokens on this platform".into())
}

#[cfg(not(target_os = "macos"))]
fn satisfies(_token: &AuditToken, _requirement: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const TEAM: &str = "AB12CD34EF";

    #[test]
    fn the_requirement_names_the_server_and_the_team() {
        assert_eq!(
            server_requirement(TEAM).unwrap(),
            "identifier \"com.ellosoft.agentic-org.org-server\" and anchor apple generic \
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
    fn a_child_is_admitted_only_with_a_token_that_satisfies_the_requirement() {
        let check = ServerCheck::Requirement("R".into());
        let asked = Cell::new(None);
        let yes = |_: &AuditToken, r: &str| {
            asked.set(Some(r.to_owned()));
            true
        };
        assert_eq!(admit(&check, || Ok([7; 32]), yes), Ok([7; 32]));
        assert_eq!(asked.take().as_deref(), Some("R"));
        assert!(admit(&check, || Ok([7; 32]), |_, _| false).is_err());
        // No token: refused, and the requirement isn't even checked.
        let err = admit(&check, || Err("gone".into()), |_, _| panic!("checked")).unwrap_err();
        assert!(err.contains("gone"), "{err}");
        // Debug skips only the requirement: the token is still taken, and needed.
        assert_eq!(
            admit(&ServerCheck::Skip, || Ok([7; 32]), |_, _| panic!()),
            Ok([7; 32])
        );
        assert!(admit(&ServerCheck::Skip, || Err("gone".into()), |_, _| panic!()).is_err());
        // No team id refuses, and asks for nothing.
        let never = || -> Result<AuditToken, String> { panic!("asked for a token") };
        assert!(admit(&ServerCheck::Refuse, never, |_, _| panic!()).is_err());
    }

    #[test]
    fn a_child_that_exec_ed_after_the_check_is_refused() {
        assert!(unchanged([7; 32], Ok([7; 32])).is_ok());
        assert!(unchanged([7; 32], Ok([8; 32])).is_err());
        assert!(unchanged([7; 32], Err("gone".into())).is_err());
    }

    /// The live check on a child as the host checks it: spawned suspended, so its loader hasn't
    /// run. Its code object is found and checked all the same: it meets a requirement it
    /// satisfies (Apple's anchor), and the office server's requirement refuses it.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_suspended_child_is_checked_as_a_running_one_is() {
        let args = [std::ffi::OsStr::new("30")];
        let mut child =
            crate::spawn::spawn_suspended(std::path::Path::new("/bin/sleep"), &args, &[]).unwrap();
        let pid = child.process.id();
        let token = crate::spawn::audit_token_of_child(pid).unwrap();
        let met = crate::code_signing::code_for_audit_token(&token)
            .is_some_and(|code| crate::code_signing::meets(&code, "anchor apple"));
        let check = ServerCheck::Requirement(server_requirement(TEAM).unwrap());
        let verdict = check.admit(pid);
        child.process.kill();
        assert!(
            met,
            "a suspended child's code object wasn't found or checked"
        );
        assert!(verdict.is_err(), "{verdict:?}");
    }

    /// The live check on a real child: `/bin/sleep` (Apple's, not the office server) is refused.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_real_child_that_isnt_the_signed_server_is_refused() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let check = ServerCheck::Requirement(server_requirement(TEAM).unwrap());
        let verdict = check.admit(child.id());
        // Its token is real, and it does satisfy a requirement it meets (so the plumbing works).
        let token = crate::spawn::audit_token_of_child(child.id()).unwrap();
        let met = crate::code_signing::code_for_audit_token(&token)
            .is_some_and(|code| crate::code_signing::meets(&code, "anchor apple"));
        let still = check.still(child.id(), token);
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(verdict.is_err(), "{verdict:?}");
        assert!(met, "the plumbing didn't accept /bin/sleep as Apple's");
        assert!(still.is_ok());
    }
}
