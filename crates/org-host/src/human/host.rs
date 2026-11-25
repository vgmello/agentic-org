//! What a command knows about the host it reached on `host.sock`, and what it may send it.
//!
//! On a system that identifies a peer by its image (macOS), a command accepts only the
//! signed host, or in a debug build its own executable. On a system that names a peer by its
//! user alone (GNU/Linux, ADR-0017 decision 11) no fact identifies the program that listens:
//! any process of this user can bind a socket at that path (Linux host design, section 5).
//! A command still talks to such a host, because nothing it is given there is a credential
//! the real host would accept without a session a person approved. It never sends such a
//! host a password or a recovery secret: there, command-line sign-in is approved in the
//! browser.
use std::os::unix::net::UnixStream;

/// How much of the host a command could establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifiedHost {
    /// Its running image: the signed host, or in a debug build this very executable.
    Image,
    /// Only the user it runs as: this system has no code identity for it.
    UserAlone,
}

impl VerifiedHost {
    /// Whether a command may send this host a password or a recovery secret. A host known
    /// only by its user may be any program of that user: it is sent neither.
    pub(crate) fn may_receive_secrets(self) -> Result<(), String> {
        match self {
            VerifiedHost::Image => Ok(()),
            VerifiedHost::UserAlone => Err(
                "this system cannot identify the service's program, only its user, so no \
                 password or recovery secret is sent to it from the command line; sign in and \
                 approve this in the browser instead"
                    .into(),
            ),
        }
    }
}

/// Checks the host at the other end of `stream` and says how much of it is established.
/// A host run by another user, and one that is neither the signed host, nor known by this
/// user alone on a system that offers nothing more, nor a debug build's own executable, is
/// refused.
pub(crate) fn verify_host(stream: &UnixStream) -> Result<VerifiedHost, String> {
    if !org_socket::peer_allowed(stream, org_socket::effective_uid()) {
        return Err("host peer refused".into());
    }
    let host = match crate::peer::identity(stream) {
        Ok(host) => host,
        // A system that cannot identify the host has no identity to accept.
        Err(error) if crate::peer::unidentifiable(&error) => {
            return Err("host signing identity refused".into())
        }
        Err(_) => return Err("host identity unavailable".into()),
    };
    let signed = match crate::APP_TEAM_ID {
        Some(team) => {
            let requirement =
                crate::code_signing::developer_id_requirement(crate::HOST_SIGNING_ID, team)?;
            crate::code_signing::satisfies_hardened(&host, &requirement)
        }
        None => false,
    };
    host_kind(signed, crate::app_auth::is_this_user(&host), || {
        #[cfg(debug_assertions)]
        let own = crate::peer::executable(stream)
            .ok()
            .and_then(|p| p.canonicalize().ok())
            .zip(
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.canonicalize().ok()),
            )
            .is_some_and(|(peer_exe, own_exe)| peer_exe == own_exe);
        #[cfg(not(debug_assertions))]
        let own = false;
        own
    })
}

/// What root, acting for the service user `uid`, accepts as that user's host on
/// `host.sock`: a peer the system names by its user alone, and that user is `uid`. Where
/// the system identifies a peer's image instead, root has no route to another user's host
/// (SIGN-33 is the dialog at the console there), and this refuses.
pub(crate) fn verify_service_host(stream: &UnixStream, uid: u32) -> Result<(), String> {
    match crate::peer::identity(stream) {
        Ok(host) if host.uid == Some(uid) => Ok(()),
        Ok(_) => Err("the socket is not served by that user's service".into()),
        Err(_) => Err("host identity unavailable".into()),
    }
}

/// Whether this system names a peer by its user alone, asked of a connection of this
/// process to itself: what a command needs to know to say who may reset the administrator.
pub(crate) fn host_is_named_by_user_alone() -> bool {
    UnixStream::pair().is_ok_and(|(one, _other)| {
        crate::peer::identity(&one).is_ok_and(|identity| identity.uid.is_some())
    })
}

/// The decision: `signed` is whether the host's image satisfies the signed host's
/// requirement; `this_user_alone` whether the system names it by its user alone, and that
/// user is this one; `own_executable` whether a debug build finds its own executable there
/// (asked only when neither holds).
fn host_kind(
    signed: bool,
    this_user_alone: bool,
    own_executable: impl FnOnce() -> bool,
) -> Result<VerifiedHost, String> {
    if signed {
        return Ok(VerifiedHost::Image);
    }
    // A system that names a peer by its user alone has no code identity for the host: the
    // socket's owner is all a command can know of it there.
    if this_user_alone {
        return Ok(VerifiedHost::UserAlone);
    }
    if own_executable() {
        return Ok(VerifiedHost::Image);
    }
    Err("host signing identity refused".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never() -> bool {
        panic!("asked for the executable")
    }

    #[test]
    fn a_host_is_known_by_its_image_or_by_its_user_alone_or_refused() {
        // The signed host is known by its image, whatever else holds.
        assert_eq!(host_kind(true, false, never), Ok(VerifiedHost::Image));
        assert_eq!(host_kind(true, true, never), Ok(VerifiedHost::Image));
        // Named by this user alone: accepted as that, and nothing more is asked.
        assert_eq!(host_kind(false, true, never), Ok(VerifiedHost::UserAlone));
        // Neither: a debug build's own executable, or refused.
        assert_eq!(host_kind(false, false, || true), Ok(VerifiedHost::Image));
        assert_eq!(
            host_kind(false, false, || false),
            Err("host signing identity refused".into())
        );
    }

    #[test]
    fn only_a_host_known_by_its_image_is_sent_a_password_or_a_recovery_secret() {
        assert_eq!(VerifiedHost::Image.may_receive_secrets(), Ok(()));
        let refusal = VerifiedHost::UserAlone.may_receive_secrets().unwrap_err();
        assert!(
            refusal.contains("no password or recovery secret"),
            "{refusal}"
        );
        assert!(refusal.contains("browser"), "{refusal}");
    }

    /// A real connection of this process to itself, judged as a command judges its host:
    /// known by its image where the system identifies one (this debug test binary is its own
    /// executable), by its user alone where that is all the system names. Exactly one of the
    /// two per system, by the platform's fact.
    #[cfg(debug_assertions)]
    #[test]
    fn this_process_as_a_host_is_known_as_this_system_can_know_it() {
        use org_platform_host::admission::ImageIdentity;
        // A listener and a connection to it, as a command reaches `host.sock`.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let command = UnixStream::connect(&path).unwrap();
        let (_host, _) = listener.accept().unwrap();
        let known = verify_host(&command).unwrap();
        assert_eq!(
            known,
            match crate::test_programs::image_identity() {
                ImageIdentity::CodeSignature => VerifiedHost::Image,
                ImageIdentity::VerifiedBytes => VerifiedHost::UserAlone,
            }
        );
        assert_eq!(
            known.may_receive_secrets().is_ok(),
            known == VerifiedHost::Image
        );
    }
}
