use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{GatewayError, Milliseconds, Scope, SessionReason};
use crate::OrgSnapshot;

/// Opaque nonsecret resume position. Authorization, scope/epoch binding and expiry must be
/// checked by the gateway on every reconnect; a syntactically valid cursor is not authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ResumeCursor(String);

impl ResumeCursor {
    pub fn parse(value: String) -> Result<Self, &'static str> {
        (!value.is_empty())
            .then_some(Self(value))
            .ok_or("a resume cursor must not be empty")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ResumeCursor {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Feed {
    InstanceStatus,
    Organization,
    Work,
    Messaging,
    Presence,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub scope: Scope,
    pub feed: Feed,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ResumeCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamRequest {
    pub subscriptions: Vec<Subscription>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionCursor {
    pub scope: Scope,
    pub feed: Feed,
    pub cursor: ResumeCursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub snapshot: OrgSnapshot,
    /// Produced with the snapshot at exactly its position. Do not derive this in the browser.
    pub resume_cursor: ResumeCursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResyncReason {
    SnapshotRequired,
    SlowConsumer,
    #[serde(other)]
    Unknown,
}

/// SSE uses `event: frame` and JSON data of this shape. No global SSE ID replaces the scoped
/// cursors. Short polling returns exactly the same frames. Unknown future frames are explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum EventFrame {
    Ready {
        subscriptions: Vec<SubscriptionCursor>,
    },
    Change {
        scope: Scope,
        feed: Feed,
        cursor: ResumeCursor,
        event: String,
        data: Value,
    },
    Resync {
        scope: Scope,
        feed: Feed,
        reason: ResyncReason,
    },
    Heartbeat {
        server_time_ms: Milliseconds,
    },
    SessionEnded {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<SessionReason>,
    },
    Unavailable {
        scope: Scope,
        feed: Feed,
        error: GatewayError,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollResponse {
    pub frames: Vec<EventFrame>,
    pub retry_after_ms: Milliseconds,
}
