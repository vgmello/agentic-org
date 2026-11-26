//! `msg.bind`: binding a connection as a member.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::calls::Call;

/// `msg.bind`: bind this connection as a member (see [Binding](super#binding)). Every other
/// `msg.*` call (and `who`, `status.set`) needs it first.
///
/// Any local client may call it. An agent binds without a credential (an accepted risk until
/// the office runner spawns every agent, ADR-0003 M-A). A person or the owner never binds
/// here: they act through an owner grant on a host channel.
///
/// It fails closed, checking in this order:
/// 1. The input alone: params that don't decode (including a present `app_token`, which is
///    retired, or both `member` and `agent`), `office` as `member`, or no `member`, are
///    `invalid_request`; a `member` over [`MAX_HANDLE_BYTES`] is `too_large`. None of this
///    depends on who the member is.
/// 2. The member: one that isn't found, a person or the owner, an agent not placed in this
///    office, an [`Offboarded`](crate::org::AgentStatus::Offboarded) agent, one whose status
///    is [`Unknown`](crate::org::AgentStatus::Unknown), or a lookup that fails, is
///    `not_authorized`, whatever else the call sends. A
///    [`Paused`](crate::org::AgentStatus::Paused) agent may still bind.
///
/// Each call replaces the previous binding and is checked only on its own params. A failed
/// call leaves the connection unbound. Any re-bind, failed or not, ends the previous
/// binding's held `msg.wait` and `msg.ask` calls with `not_bound`, and ends the connection's
/// `subscribe`: no event follows the bind's answer.
///
/// Errors: `not_authorized` (step 2), `invalid_request` (step 1), `too_large` (step 1).
///
/// [`MAX_HANDLE_BYTES`]: super::MAX_HANDLE_BYTES
pub struct MsgBind;

/// `msg.bind`'s params.
///
/// Office-run agents also send their per-run `session_token` (`AGENT_OS_SESSION_TOKEN`).
#[derive(Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgBindParams {
    /// Who to act as (see [Addressing](super#addressing)); at most [`MAX_HANDLE_BYTES`]. Also
    /// read from `agent`, the ADR's name for it; params with both don't decode
    /// (`invalid_request`).
    ///
    /// [`MAX_HANDLE_BYTES`]: super::MAX_HANDLE_BYTES
    #[serde(default, alias = "agent", skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// Per-run cryptographic session token issued by the office for managed agent runs
    /// (`AGENT_OS_SESSION_TOKEN`). Never logged or echoed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// Retired: the app token no longer exists. Any present value, `null` included, makes the
    /// params fail to decode ([`crate::retired::app_token`]), so a server answers
    /// `invalid_request`. It is never written. Build params with [`MsgBindParams::member`].
    #[serde(
        default,
        rename = "app_token",
        skip_serializing,
        deserialize_with = "crate::retired::app_token"
    )]
    pub retired_token: Option<crate::retired::Retired>,
}

impl fmt::Debug for MsgBindParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MsgBindParams")
            .field("member", &self.member)
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .field("retired_token", &self.retired_token)
            .finish()
    }
}

impl MsgBindParams {
    /// Params to bind as `member` (a handle or member id).
    pub fn member(member: impl Into<String>) -> Self {
        MsgBindParams {
            member: Some(member.into()),
            session_token: None,
            retired_token: None,
        }
    }

    /// Params to bind as `member` carrying a managed run's session binding token.
    pub fn member_with_token(member: impl Into<String>, session_token: impl Into<String>) -> Self {
        MsgBindParams {
            member: Some(member.into()),
            session_token: Some(session_token.into()),
            retired_token: None,
        }
    }
}

/// `msg.bind`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsgBindResult {
    /// The bound member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    pub office_id: String,
}

impl Call for MsgBind {
    const METHOD: &'static str = "msg.bind";
    type Params = MsgBindParams;
    type Result = MsgBindResult;
}
