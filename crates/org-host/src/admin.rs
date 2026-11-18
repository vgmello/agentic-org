//! Administrator links on `host.sock` (A2, ADR-0022, decisions D1-D4).
//!
//! `admin.link.initial` gives setup the one registration link, bound to the frozen setup
//! operation; a replay answers `already_issued` with no secret. `admin.reset` asks for local
//! presence and confirmation at the host, then issues a registration link that cancels the
//! previous one. Setting a new administrator password is A3's branch and answers
//! `unavailable` here. Both calls require the signing check `recovery` uses; hello, the
//! registered instance and the host's effective UID are enforced before dispatch.
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
}

struct Resetting<'a>(&'a AtomicBool);
impl Drop for Resetting<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn denied() -> ApiError {
    ApiError::new(
        ErrorCode::NotAuthorized,
        "local administrator confirmation at the server machine is required",
    )
}
fn unavailable(_: impl std::fmt::Debug) -> ApiError {
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
        }
    }

    #[cfg(test)]
    pub(crate) fn issues_for(&self, setup_operation: &str, origin: &str) -> bool {
        self.setup_operation == setup_operation && self.origin.as_str() == origin
    }

    /// The caller is a local `host.sock` client that passes the host's signing check.
    fn admit(&self, c: &Connection, received: Received) -> Result<(), ApiError> {
        if c.channel_kind() != org_office_launch::v3::ChannelKind::Local {
            return Err(denied());
        }
        #[cfg(target_os = "macos")]
        if self.policy.allows(&crate::app_auth::SocketPeer {
            stream: c.socket(),
            accepted: c.accepted(),
            received,
        }) {
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (&self.policy, received);
        Err(denied())
    }

    pub(crate) fn initial(
        &self,
        c: &Connection,
        received: Received,
        params: AdminLinkInitialParams,
    ) -> Result<AdminLinkResult, ApiError> {
        self.admit(c, received)?;
        self.initial_link(&params.setup_operation, wall()?)
    }

    pub(crate) fn reset(
        &self,
        c: &Connection,
        received: Received,
    ) -> Result<AdminLinkResult, ApiError> {
        self.admit(c, received)?;
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
            return Err(set_password_unavailable());
        }
        confirm().map_err(|()| denied())?;
        // Re-checked after the dialog: an administrator may have registered meanwhile.
        if self.db.administrator_exists().map_err(unavailable)? {
            return Err(set_password_unavailable());
        }
        let issued = self
            .db
            .issue(LinkKind::Registration, &subject()?, wall()?)
            .map_err(unavailable)?;
        self.issued(issued)
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

fn set_password_unavailable() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "an administrator exists; setting a new administrator password is not available yet",
    )
}

#[cfg(test)]
mod tests;
