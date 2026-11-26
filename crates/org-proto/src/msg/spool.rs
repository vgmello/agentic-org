//! The agent spool: a `msg.send` written to disk while the office server is unreachable.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::message::MessagePriority;
use super::{BodySize, Meta, MetaSize};

/// A `msg.send` written to the office's spool by `agent-os` while its office server is
/// unreachable (ADR-0003 §3.5), and ingested by the office when it starts. Only
/// fire-and-forget sends are spooled, never asks, so it has no deadline.
///
/// `from` is unauthenticated (whoever wrote the file), so ingest applies every `msg.send`
/// rule as if `from` had bound as an agent without a token: the limits apply, kinds `request`
/// and `delivery_failed`, an `Unknown` priority and an `office` recipient are refused, the
/// recipient rules of [Addressing](super#addressing) apply, and `reply_to` must be visible to
/// `from`. On top of that, `from` must resolve to an **active agent that the office doesn't
/// run** (one with no run mode, per the agent-sessions design): agents the office runs will
/// bind with a session token (see [`MsgBindParams`]), which a file can't carry. A `from` that
/// is a person, the owner, `office`, a `run:` address (agent-sessions design), an office-run
/// agent, an agent whose status isn't [`Active`](crate::org::AgentStatus::Active)
/// ([`Paused`](crate::org::AgentStatus::Paused),
/// [`Offboarded`](crate::org::AgentStatus::Offboarded) or
/// [`Unknown`](crate::org::AgentStatus::Unknown)), or a handle or id that names no member is
/// refused.
///
/// A `created_at` in the future is clamped to the ingest time. A backdated `created_at` is
/// accepted: like `from`, it's the writer's word (the agent-identity risk, ADR-0003 M-A). A
/// file whose `office_id` isn't this office, with an unknown `v`, or that fails a rule, is
/// moved to `spool/dead/` and never delivered. The office logs the file's id and the reason,
/// never its body or meta.
///
/// Its `Debug` prints only the sizes of `body` and `meta`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpooledSend {
    /// [`SPOOL_VERSION`].
    pub v: u32,
    /// The office it was spooled for; any other office refuses it.
    pub office_id: String,
    /// The sender, as the writer claims (see above).
    pub from: String,
    /// Client-made (a ULID); ingest dedups on it like a `msg.send` retry.
    pub id: String,
    pub to: String,
    pub kind: String,
    pub priority: MessagePriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// When `agent-os` wrote it, as the writer claims (see above).
    pub created_at: String,
}

impl fmt::Debug for SpooledSend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpooledSend")
            .field("v", &self.v)
            .field("office_id", &self.office_id)
            .field("from", &self.from)
            .field("id", &self.id)
            .field("to", &self.to)
            .field("kind", &self.kind)
            .field("priority", &self.priority)
            .field("subject", &self.subject)
            .field("body", &BodySize(&self.body))
            .field("meta", &MetaSize(&self.meta))
            .field("reply_to", &self.reply_to)
            .field("created_at", &self.created_at)
            .finish()
    }
}
