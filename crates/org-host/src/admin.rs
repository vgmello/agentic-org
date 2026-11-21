//! Administrator links on `host.sock` (A2, ADR-0022, decisions D1-D4).
//!
//! `admin.link.initial` gives setup the one registration link, bound to the frozen setup
//! operation; a replay answers `already_issued` with no secret. `admin.reset` asks for local
//! presence and confirmation at the host, then issues a registration link that cancels the
//! previous one. An instance that has an administrator answers `administrator_present` and
//! issues nothing: a registration link is for an instance without one, and setting a new
//! administrator password (A3's branch) is not offered here (X5 review, fourth round,
//! P2-10). Both calls require the signing check `recovery` uses; hello, the
//! registered instance and the host's effective UID are enforced before dispatch.
//!
//! On a system that names a peer by its user alone (GNU/Linux, ADR-0017 decision 11) there
//! is no dialog and no code identity: `admin.reset` is authorized by root, the one user
//! besides the host's own that `host.sock` admits, and for that call only (SIGN-33). A peer
//! of the host's own user is refused there, whatever its program.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use org_host_control::admin::{AdminLinkInitialParams, AdminLinkResult};
use org_host_control::listen::ListenOrigin;
use org_host_control::OneTimeLinkUrl;
use org_proto::{ApiError, ErrorCode};

use crate::app_auth::AppPolicy;
use crate::hostdb::HostDb;
use crate::links::{Issued, LinkKind, OneTimeLinks, Subject};
use crate::peer::Received;
use crate::service::listener::Connection;

/// The single subject every registration link is for: the instance's administrator. Setup's
/// link and every `admin reset` link share it, so issuing one cancels the previous.
const SUBJECT: &str = "administrator";

/// Local presence at the host machine and the OS confirmation dialog. Consumer-owned so tests
/// inject a fake; production uses [`NativePresence`].
pub(crate) trait Presence: Send + Sync {
    fn local(&self, connection: &Connection) -> bool;
    fn confirm(&self, cancelled: &dyn Fn() -> bool) -> Result<(), ()>;
}

/// The platform presence bridge (Touch ID or the login password on macOS; refuses elsewhere).
pub(crate) struct NativePresence;
impl Presence for NativePresence {
    fn local(&self, connection: &Connection) -> bool {
        crate::recovery::presence::local(connection)
    }
    fn confirm(&self, cancelled: &dyn Fn() -> bool) -> Result<(), ()> {
        crate::recovery::presence::confirm(cancelled)
    }
}

pub(crate) struct Admin {
    db: Arc<HostDb>,
    setup_operation: String,
    origin: ListenOrigin,
    policy: AppPolicy,
    presence: Arc<dyn Presence>,
    resetting: AtomicBool,
    /// The instance's service mode is `foreground`: its setup never asks for the initial
    /// link, and its first link comes only from root (ADR-0017 decision 10).
    foreground: AtomicBool,
}

struct Resetting<'a>(&'a AtomicBool);
impl Drop for Resetting<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Admission order: only the local channel may administer, whatever the signing
/// policy would say; the policy is consulted only for a local caller.
pub(crate) fn admit_with(
    channel: org_office_launch::ChannelKind,
    policy_allows: impl FnOnce() -> bool,
) -> Result<(), ApiError> {
    if channel != org_office_launch::ChannelKind::Local {
        return Err(denied());
    }
    if policy_allows() {
        Ok(())
    } else {
        Err(denied())
    }
}

/// Whether the system names this connection's peer by its user alone (GNU/Linux), so that
/// no fact tells a command a person ran from any other process of that user.
fn named_by_user_alone(c: &Connection) -> bool {
    crate::peer::identity(c.socket()).is_ok_and(|identity| identity.uid.is_some())
}

fn denied() -> ApiError {
    ApiError::new(
        ErrorCode::NotAuthorized,
        "local administrator confirmation at the server machine is required",
    )
}
/// The caller sees one generic answer; the host log keeps the cause. Every error passed here
/// is a kind (a registry, link-store or URL-shape error), never token or secret bytes.
fn unavailable(cause: impl std::fmt::Debug) -> ApiError {
    crate::host_log::log(&format!("administrator link unavailable: {cause:?}"));
    ApiError::new(ErrorCode::Unavailable, "the host registry is unavailable")
}
fn wall() -> Result<i64, ApiError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|n| i64::try_from(n.as_millis()).ok())
        .ok_or_else(|| unavailable("clock"))
}

impl Admin {
    pub(crate) fn new(
        db: Arc<HostDb>,
        setup_operation: &str,
        origin: ListenOrigin,
        policy: AppPolicy,
        presence: Arc<dyn Presence>,
    ) -> Self {
        Self {
            db,
            setup_operation: setup_operation.to_owned(),
            origin,
            policy,
            presence,
            resetting: AtomicBool::new(false),
            foreground: AtomicBool::new(false),
        }
    }

    /// Marks the instance as one that runs in the foreground (a container): from then on
    /// `admin.link.initial` is refused, whoever names the setup operation. Setup in that
    /// mode never asks for it, so the only caller would be another process of the service
    /// user, and the container's first link comes only from root's `admin reset`.
    pub(crate) fn foreground_instance(&self) {
        self.foreground.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn issues_for(&self, setup_operation: &str, origin: &str) -> bool {
        self.setup_operation == setup_operation && self.origin.as_str() == origin
    }

    pub(crate) fn origin(&self) -> &str {
        self.origin.as_str().trim_end_matches('/')
    }

    /// The admission of every administrator's host call (`tls.set` too): a local
    /// `host.sock` client that passes the host's signing check.
    pub(crate) fn admit_peer(&self, c: &Connection, received: Received) -> Result<(), ApiError> {
        self.admit(c, received)
    }

    /// The caller is a local `host.sock` client that passes the host's signing check.
    fn admit(&self, c: &Connection, received: Received) -> Result<(), ApiError> {
        admit_with(c.channel_kind(), || {
            // On a system that cannot identify a peer's image the peer has no token and is
            // refused (`app_auth::SocketPeer`).
            self.policy.allows(&crate::app_auth::SocketPeer {
                stream: c.socket(),
                accepted: c.accepted(),
                received,
            })
        })
    }

    pub(crate) fn initial(
        &self,
        c: &Connection,
        received: Received,
        params: AdminLinkInitialParams,
    ) -> Result<AdminLinkResult, ApiError> {
        self.admit(c, received)?;
        if self.foreground.load(Ordering::SeqCst) {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "this instance runs in the foreground: its first registration link comes \
                 from admin reset, run by root",
            ));
        }
        self.initial_link(&params.setup_operation, wall()?)
    }

    pub(crate) fn reset(
        &self,
        c: &Connection,
        received: Received,
    ) -> Result<AdminLinkResult, ApiError> {
        if c.root_authority() {
            // SIGN-33 on a system that names a peer by its user alone: root at the server
            // machine is the local authority, attested by the kernel when the connection
            // was accepted. There is no dialog to show; nothing else of root's is read.
            admit_with(c.channel_kind(), || true)?;
            if !c.recovery_peer_idle() {
                return Err(denied());
            }
            return self.reset_with(true, || Ok(()));
        }
        self.admit(c, received)?;
        if named_by_user_alone(c) {
            // The same user as the service is not an authority over its administrator:
            // any process of that user, an agent included, could ask. Whatever a later
            // presence adapter answers there, only root resets.
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "on this system the administrator is reset by root at the server machine",
            ));
        }
        if !c.recovery_peer_idle() {
            return Err(denied());
        }
        let local = self.presence.local(c);
        self.reset_with(local, || {
            self.presence
                .confirm(&|| !c.recovery_peer_idle())
                .and_then(|()| {
                    if c.recovery_peer_idle() {
                        Ok(())
                    } else {
                        Err(())
                    }
                })
        })
    }

    /// The initial link: the caller names the frozen setup operation, no administrator
    /// exists, and no registration link was ever issued.
    fn initial_link(
        &self,
        setup_operation: &str,
        now_ms: i64,
    ) -> Result<AdminLinkResult, ApiError> {
        if setup_operation != self.setup_operation {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "the setup operation does not match this instance",
            ));
        }
        if self.db.administrator_exists().map_err(unavailable)? {
            return Ok(AdminLinkResult::AdministratorPresent);
        }
        match self
            .db
            .issue_first(LinkKind::Registration, &subject()?, now_ms)
            .map_err(unavailable)?
        {
            Some(issued) => self.issued(issued),
            None => Ok(AdminLinkResult::AlreadyIssued),
        }
    }

    /// `admin reset` after presence: `local` is the presence check's answer and `confirm`
    /// shows the OS dialog. One reset runs at a time.
    fn reset_with(
        &self,
        local: bool,
        confirm: impl FnOnce() -> Result<(), ()>,
    ) -> Result<AdminLinkResult, ApiError> {
        if !local {
            return Err(denied());
        }
        self.resetting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "an admin reset is already in progress",
                )
            })?;
        let _resetting = Resetting(&self.resetting);
        if self.db.administrator_exists().map_err(unavailable)? {
            return Ok(AdminLinkResult::AdministratorPresent);
        }
        confirm().map_err(|()| denied())?;
        // Re-checked after the dialog, in the transaction that issues the link: an
        // administrator may have registered meanwhile.
        match self
            .db
            .issue_unless_administrator(LinkKind::Registration, &subject()?, wall()?)
            .map_err(unavailable)?
        {
            Some(issued) => self.issued(issued),
            None => Ok(AdminLinkResult::AdministratorPresent),
        }
    }

    /// The link URL is built from the instance's own configured origin; the token goes after
    /// `#` so it never reaches a request line or a log.
    fn issued(&self, issued: Issued) -> Result<AdminLinkResult, ApiError> {
        let token = issued.token.encode();
        let origin = self.origin.as_str().trim_end_matches('/');
        let mut url = String::with_capacity(origin.len() + "/register#".len() + token.len());
        url.push_str(origin);
        url.push_str("/register#");
        url.push_str(&token);
        let url = OneTimeLinkUrl::new(url).map_err(unavailable)?;
        Ok(AdminLinkResult::Issued {
            url,
            expires_at_ms: issued.expires_at_ms,
        })
    }
}

fn subject() -> Result<Subject, ApiError> {
    Subject::new(SUBJECT).map_err(unavailable)
}

#[cfg(test)]
pub(crate) mod tests;
