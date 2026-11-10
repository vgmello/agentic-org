//! Who may get an office's database key: the check of every office server the host starts,
//! before it is resumed and its first line is written, and of the connection the key is handed
//! over on (`handoff`).
//!
//! The threat: a same-user process runs the signed host from a copy, a symlink or a hard link,
//! next to its own `org-office-server` (the host starts the server next to its own executable),
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
//! identifier "com.ellosoft.agentic-org.org-office-server" and anchor apple generic
//!   and certificate 1[field.1.2.840.113635.100.6.2.6] exists
//!   and certificate leaf[field.1.2.840.113635.100.6.1.13] exists
//!   and certificate leaf[subject.OU] = "<ORG_APP_TEAM_ID>"
//! ```
//!
//! **The key never travels on an inherited descriptor.** Another same-user process can resume a
//! suspended child itself before the host takes its token (stopping or starving the host makes
//! that easy), let its image fork a helper that keeps stdin, then turn itself into the genuine
//! server, so the check sees the genuine one (see `spawn`'s module docs). So the check alone
//! doesn't decide who gets the secrets: the first line on stdin carries none, and the host hands
//! the app token and the key only over a connection the admitted server opens itself, after its
//! exec, to a one-shot socket (`handoff`). That connection's peer must be the admitted image:
//! [`PeerRule`], decided at accept by [`PeerRule::admits`]. In a release build its audit token
//! (`LOCAL_PEERTOKEN`) must equal the admitted one byte for byte, pid version included, and the
//! code it names must still satisfy the requirement; a helper forked before the exec carries
//! another token, and connects as itself. Debug builds check `LOCAL_PEERPID` is the child's pid
//! and the uid is ours instead. Either way, a peer that has already sent anything is refused.
//! The peer token names the pid that last used the connection, looked up when asked, so the rule
//! means something only because the socket is bound after the admission, the server touches no
//! socket it didn't create, and the host's peer-fd check finds the connection on one fd of the
//! admitted pid, close-on-exec and not shared (`handoff`'s module docs), after which the peer is
//! sampled again ([`PeerRule::still`]). The debug pid rule rests on the same facts.
//!
//! Only a child that passes is resumed. Right before the first line is written, its audit token
//! is taken again and must be unchanged, so an `exec` after the check is refused. A child that
//! fails, or whose token can't be taken, is killed and reaped (still suspended if it failed the
//! first check, so it never ran), and gets nothing; the start counts as a `storage_error`
//! (retried with backoff, up to the give-up rule). So does a hand-off whose peer [`PeerRule`]
//! refuses.
//!
//! A release host with no team id compiled in can't check anything, so it starts no server at
//! all (logged loudly). **Debug builds don't check signatures** (their servers are unsigned
//! development and test builds), which is logged at startup; their hand-off still checks the
//! peer's pid and uid ([`PeerRule::Pid`]).

/// The office server's signing identifier (ADR-0003 §2.1): a release host gives an office's key
/// only to a server signed with it by the app's team (`server_check`). The bundle must sign
/// `Contents/MacOS/org-office-server` with it.
pub const SERVER_SIGNING_ID: &str = "com.ellosoft.agentic-org.org-office-server";

use crate::peer::AuditToken;

/// How the host checks the office servers it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServerCheck {
    /// Debug builds: no check.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Skip,
    /// Every server must satisfy this requirement and run hardened code.
    Requirement(String),
    /// No team id: no server may be started.
    Refuse,
    /// Tests only: the admission and the re-check's token come from the test.
    #[cfg(test)]
    Scripted(Scripted),
}

/// Which step a [`Scripted`] check is asked for.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// [`ServerCheck::admit`]: the admitted token (`None`: not checked), or a refusal.
    Admit,
    /// [`ServerCheck::still`]: the child's token now.
    Still,
}

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
                                  before they get an office's key"
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

    /// After the spawn of child `pid`: its audit token if it may get a key (`None`: not
    /// checked, a debug build), or why not.
    pub(crate) fn admit(&self, pid: u32) -> Result<Option<AuditToken>, String> {
        #[cfg(test)]
        if let ServerCheck::Scripted(scripted) = self {
            return (scripted.0)(Step::Admit, pid);
        }
        admit(self, || token_of(pid), satisfies)
    }

    /// Right before child `pid`'s first line: whether it is still the image `admitted` (what
    /// [`ServerCheck::admit`] returned).
    pub(crate) fn still(&self, pid: u32, admitted: Option<AuditToken>) -> Result<(), String> {
        let now = || {
            #[cfg(test)]
            if let ServerCheck::Scripted(scripted) = self {
                return (scripted.0)(Step::Still, pid)
                    .and_then(|token| token.ok_or_else(|| "no token".to_owned()));
            }
            token_of(pid)
        };
        match admitted {
            None => Ok(()),
            Some(admitted) => unchanged(admitted, now()),
        }
    }
}

/// Who may take a spawn's secrets on its key socket (`handoff`): what the peer of the one
/// connection the host accepts must be. Made by [`ServerCheck::peer_rule`] from what the
/// admission returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PeerRule {
    /// Release builds: the peer's audit token must be the admitted one, byte for byte (pid
    /// version included), and the code it names must still satisfy `requirement`.
    Token {
        admitted: AuditToken,
        requirement: String,
    },
    /// Debug builds (and tests), which don't check signatures: the peer must be the child
    /// (`LOCAL_PEERPID`), running as `uid`. A pid can be reused, a token can't: release builds
    /// never use this.
    #[cfg(any(debug_assertions, test))]
    Pid { pid: u32, uid: u32 },
}

/// What the host read about a hand-off connection's peer as it accepted it: identity first,
/// then how many bytes it had already sent. `None`: not read (the rule doesn't need it) or
/// couldn't be read; a fact the rule needs that is `None` refuses the peer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PeerSample {
    pub(crate) token: Option<AuditToken>,
    pub(crate) pid: Option<u32>,
    pub(crate) uid: Option<u32>,
    /// Bytes waiting on the connection (`FIONREAD`): the host reads nothing from the peer, so any
    /// bytes it has sent refuse it. (It counts only bytes that arrived: a failed or zero-length
    /// write that made someone the socket's last user leaves it at 0. The peer-fd check, not
    /// this, rules those out: `handoff`.)
    pub(crate) waiting: Option<u64>,
}

impl ServerCheck {
    /// The rule for the hand-off of child `pid`, admitted as `admitted` (what
    /// [`ServerCheck::admit`] returned), or why no connection may get its secrets.
    pub(crate) fn peer_rule(
        &self,
        admitted: Option<AuditToken>,
        pid: u32,
    ) -> Result<PeerRule, String> {
        match (self, admitted) {
            (ServerCheck::Requirement(requirement), Some(admitted)) => Ok(PeerRule::Token {
                admitted,
                requirement: requirement.clone(),
            }),
            #[cfg(any(debug_assertions, test))]
            (ServerCheck::Skip, None) => Ok(PeerRule::Pid {
                pid,
                uid: crate::home::euid(),
            }),
            // A scripted admission names no real token: the debug rule.
            #[cfg(test)]
            (ServerCheck::Scripted(_), _) => Ok(PeerRule::Pid {
                pid,
                uid: crate::home::euid(),
            }),
            _ => {
                let _ = pid;
                Err("no rule says who may take this server's key".into())
            }
        }
    }
}

impl PeerRule {
    /// Whether the connection `sample` describes may take the secrets.
    pub(crate) fn admits(&self, sample: &PeerSample) -> Result<(), String> {
        admits_peer(self, sample, satisfies)
    }

    /// The admitted server's pid: whose fds the hand-off's peer-fd check scans (`handoff`). In a
    /// release build, the pid the admitted audit token names.
    pub(crate) fn pid(&self) -> u32 {
        match self {
            PeerRule::Token { admitted, .. } => token_pid(admitted),
            #[cfg(any(debug_assertions, test))]
            PeerRule::Pid { pid, .. } => *pid,
        }
    }

    /// The re-check after the peer-fd check, with a fresh `sample` of the same connection: it is
    /// still the admitted peer (the same token byte for byte, or the same pid and uid) and has
    /// still sent nothing; in a release build, the admitted pid must also still be the admitted
    /// image (its task's token, as [`ServerCheck::still`] reads it). So the fds that were scanned
    /// were the admitted server's: a pid that died or was reused meanwhile can't pass. (The code
    /// requirement isn't asked again: the token is the one that met it a moment ago, and an image
    /// can't change without changing the token.)
    pub(crate) fn still(&self, sample: &PeerSample) -> Result<(), String> {
        still_peer(self, sample, token_of)
    }
}

/// [`PeerRule::still`], pure: `token_now` gives the task token of a pid.
fn still_peer(
    rule: &PeerRule,
    sample: &PeerSample,
    token_now: impl FnOnce(u32) -> Result<AuditToken, String>,
) -> Result<(), String> {
    admits_peer(rule, sample, |_, _| true)?;
    match rule {
        PeerRule::Token { admitted, .. } => unchanged(*admitted, token_now(rule.pid())),
        #[cfg(any(debug_assertions, test))]
        PeerRule::Pid { .. } => Ok(()),
    }
}

/// The pid an audit token names (`audit_token_t` word 5).
fn token_pid(token: &AuditToken) -> u32 {
    u32::from_ne_bytes([token[20], token[21], token[22], token[23]])
}

/// The hand-off decision, pure: `satisfies` checks the code a token names against a requirement,
/// and is asked only for a token equal to the admitted one.
fn admits_peer(
    rule: &PeerRule,
    sample: &PeerSample,
    satisfies: impl FnOnce(&AuditToken, &str) -> bool,
) -> Result<(), String> {
    match rule {
        PeerRule::Token {
            admitted,
            requirement,
        } => match sample.token {
            None => return Err("its audit token couldn't be read".into()),
            Some(token) if token != *admitted => {
                return Err("it isn't the admitted server (another audit token)".into())
            }
            Some(token) => {
                if !satisfies(&token, requirement) {
                    return Err("it no longer satisfies the office server's requirement".into());
                }
            }
        },
        #[cfg(any(debug_assertions, test))]
        PeerRule::Pid { pid, uid } => {
            if sample.pid != Some(*pid) {
                return Err("it isn't the server the host started (another pid)".into());
            }
            if sample.uid != Some(*uid) {
                return Err("it runs as another user".into());
            }
        }
    }
    match sample.waiting {
        Some(0) => Ok(()),
        Some(_) => Err("it sent something before the key".into()),
        None => Err("what it sent couldn't be read".into()),
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
) -> Result<Option<AuditToken>, String> {
    match check {
        ServerCheck::Skip => Ok(None),
        ServerCheck::Refuse => Err("this build has no app team id to check servers with".into()),
        #[cfg(test)]
        ServerCheck::Scripted(_) => Err("a scripted check isn't decided here".into()),
        ServerCheck::Requirement(requirement) => {
            let token = token().map_err(|why| format!("its audit token: {why}"))?;
            if satisfies(&token, requirement) {
                Ok(Some(token))
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
            "identifier \"com.ellosoft.agentic-org.org-office-server\" and anchor apple generic \
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
        assert_eq!(admit(&check, || Ok([7; 32]), yes), Ok(Some([7; 32])));
        assert_eq!(asked.take().as_deref(), Some("R"));
        assert!(admit(&check, || Ok([7; 32]), |_, _| false).is_err());
        // No token: refused, and the requirement isn't even checked.
        let err = admit(&check, || Err("gone".into()), |_, _| panic!("checked")).unwrap_err();
        assert!(err.contains("gone"), "{err}");
        // Debug skips; no team id refuses; neither asks for anything.
        let never = || -> Result<AuditToken, String> { panic!("asked for a token") };
        assert_eq!(admit(&ServerCheck::Skip, never, |_, _| panic!()), Ok(None));
        assert!(admit(&ServerCheck::Refuse, never, |_, _| panic!()).is_err());
    }

    /// The hand-off's peer: in a release build, only the admitted token, byte for byte, whose
    /// code still meets the requirement; nothing sent first.
    #[test]
    fn a_hand_off_peer_must_carry_the_admitted_token_and_meet_the_requirement() {
        let admitted = [7u8; 32];
        let rule = PeerRule::Token {
            admitted,
            requirement: "R".into(),
        };
        let sample = |token: Option<AuditToken>, waiting: Option<u64>| PeerSample {
            token,
            waiting,
            ..PeerSample::default()
        };
        let yes = |t: &AuditToken, r: &str| {
            assert_eq!((t, r), (&admitted, "R"));
            true
        };
        assert_eq!(
            admits_peer(&rule, &sample(Some(admitted), Some(0)), yes),
            Ok(())
        );
        // One byte off anywhere (the pid version is word 7) refuses it, and the requirement
        // isn't even asked.
        for at in [0, 20, 28, 31] {
            let mut other = admitted;
            other[at] ^= 1;
            let err =
                admits_peer(&rule, &sample(Some(other), Some(0)), |_, _| panic!()).unwrap_err();
            assert!(err.contains("another audit token"), "{err}");
        }
        // The right token whose code no longer meets the requirement.
        assert!(admits_peer(&rule, &sample(Some(admitted), Some(0)), |_, _| false).is_err());
        // No token; a peer that sent something; or one whose bytes can't be counted.
        assert!(admits_peer(&rule, &sample(None, Some(0)), |_, _| panic!()).is_err());
        let err = admits_peer(&rule, &sample(Some(admitted), Some(1)), |_, _| true).unwrap_err();
        assert!(err.contains("sent something"), "{err}");
        assert!(admits_peer(&rule, &sample(Some(admitted), None), |_, _| true).is_err());
        // A pid and a uid don't stand in for the token.
        let by_pid = PeerSample {
            pid: Some(42),
            uid: Some(501),
            waiting: Some(0),
            ..PeerSample::default()
        };
        assert!(admits_peer(&rule, &by_pid, |_, _| true).is_err());
    }

    /// Debug builds: the child's pid and our uid; nothing sent first.
    #[test]
    fn a_debug_hand_off_peer_must_be_the_child_as_this_user() {
        let rule = PeerRule::Pid { pid: 42, uid: 501 };
        let sample = |pid, uid, waiting| PeerSample {
            pid,
            uid,
            waiting,
            ..PeerSample::default()
        };
        let never = |_: &AuditToken, _: &str| -> bool { panic!("asked for a signature") };
        assert_eq!(
            admits_peer(&rule, &sample(Some(42), Some(501), Some(0)), never),
            Ok(())
        );
        let err = admits_peer(&rule, &sample(Some(43), Some(501), Some(0)), never).unwrap_err();
        assert!(err.contains("another pid"), "{err}");
        assert!(admits_peer(&rule, &sample(None, Some(501), Some(0)), never).is_err());
        assert!(admits_peer(&rule, &sample(Some(42), Some(0), Some(0)), never).is_err());
        assert!(admits_peer(&rule, &sample(Some(42), None, Some(0)), never).is_err());
        assert!(admits_peer(&rule, &sample(Some(42), Some(501), Some(3)), never).is_err());
        assert!(admits_peer(&rule, &sample(Some(42), Some(501), None), never).is_err());
    }

    #[test]
    fn the_peer_rule_follows_the_check() {
        let requirement = ServerCheck::Requirement("R".into());
        assert_eq!(
            requirement.peer_rule(Some([7; 32]), 42),
            Ok(PeerRule::Token {
                admitted: [7; 32],
                requirement: "R".into()
            })
        );
        // A release check that admitted no token gives no rule at all.
        assert!(requirement.peer_rule(None, 42).is_err());
        assert!(ServerCheck::Refuse.peer_rule(None, 42).is_err());
        assert_eq!(
            ServerCheck::Skip.peer_rule(None, 42),
            Ok(PeerRule::Pid {
                pid: 42,
                uid: crate::home::euid()
            })
        );
        assert!(ServerCheck::Skip.peer_rule(Some([7; 32]), 42).is_err());
    }

    /// The fd check scans the admitted server's pid: the one in the admitted token (word 5), or
    /// the debug rule's.
    #[test]
    fn the_peer_rule_names_the_admitted_pid() {
        let mut admitted = [0u8; 32];
        admitted[20..24].copy_from_slice(&4242u32.to_ne_bytes());
        let rule = PeerRule::Token {
            admitted,
            requirement: "R".into(),
        };
        assert_eq!(rule.pid(), 4242);
        assert_eq!(PeerRule::Pid { pid: 42, uid: 501 }.pid(), 42);
    }

    /// After the fd check: the same peer, nothing sent, and (release) the admitted pid still the
    /// admitted image. The requirement isn't asked again.
    #[test]
    fn the_re_check_wants_the_same_peer_and_the_same_image() {
        let mut admitted = [7u8; 32];
        admitted[20..24].copy_from_slice(&4242u32.to_ne_bytes());
        let rule = PeerRule::Token {
            admitted,
            requirement: "R".into(),
        };
        let sample = |token: Option<AuditToken>, waiting| PeerSample {
            token,
            waiting: Some(waiting),
            ..PeerSample::default()
        };
        let asked = Cell::new(None);
        let same = |pid| {
            asked.set(Some(pid));
            Ok(admitted)
        };
        assert_eq!(still_peer(&rule, &sample(Some(admitted), 0), same), Ok(()));
        assert_eq!(asked.get(), Some(4242));
        // The pid is now another image (it exec'd, or it died and the pid was reused), or gone.
        let mut other = admitted;
        other[28] ^= 1;
        assert!(still_peer(&rule, &sample(Some(admitted), 0), |_| Ok(other)).is_err());
        assert!(still_peer(&rule, &sample(Some(admitted), 0), |_| Err("gone".into())).is_err());
        // The connection's peer changed, or it sent something meanwhile.
        let never = |_| -> Result<AuditToken, String> { panic!("asked for the task's token") };
        assert!(still_peer(&rule, &sample(Some(other), 0), never).is_err());
        assert!(still_peer(&rule, &sample(None, 0), never).is_err());
        let err = still_peer(&rule, &sample(Some(admitted), 1), never).unwrap_err();
        assert!(err.contains("sent something"), "{err}");
        // Debug: the pid and uid again.
        let rule = PeerRule::Pid { pid: 42, uid: 501 };
        let by_pid = |pid, waiting| PeerSample {
            pid: Some(pid),
            uid: Some(501),
            waiting: Some(waiting),
            ..PeerSample::default()
        };
        assert_eq!(still_peer(&rule, &by_pid(42, 0), never), Ok(()));
        assert!(still_peer(&rule, &by_pid(43, 0), never).is_err());
        assert!(still_peer(&rule, &by_pid(42, 2), never).is_err());
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
        let still = check.still(child.id(), Some(token));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(verdict.is_err(), "{verdict:?}");
        assert!(met, "the plumbing didn't accept /bin/sleep as Apple's");
        assert!(still.is_ok());
    }
}
