//! How a session starts and turns end, and what it reports doing: [`StartFailure`],
//! [`Activity`], [`TurnOutcome`], [`LimitKind`] and [`NoticeKind`].

use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::SessionEvent;

/// Why a session couldn't start. Unknown kinds decode as [`StartFailure::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum StartFailure {
    /// The runtime doesn't know the spec's agent. (Claude would silently run its default
    /// agent instead, so the session refuses to go on.)
    AgentMismatch {
        /// The agent the spec asked for.
        requested: String,
        /// The agents the runtime reported.
        available: Vec<String>,
    },
    /// The runtime reports a different permission mode from the one the spec asked for, so
    /// the session refuses to go on rather than run looser than asked.
    ModeMismatch {
        /// The mode the adapter asked for, in the runtime's words.
        requested: String,
        /// The mode the runtime reported, in its words.
        reported: String,
    },
    /// Resuming the conversation failed (e.g. its transcript was pruned). The office starts
    /// a fresh one.
    ResumeFailed {
        /// What the runtime said.
        detail: String,
    },
    /// The runtime isn't signed in.
    AuthRequired,
    /// The runtime didn't finish the opening handshake in time.
    ProtocolTimeout,
    /// Anything else.
    Other {
        /// What happened.
        detail: String,
    },
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

impl StartFailure {
    pub(super) fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        match self {
            StartFailure::AgentMismatch {
                requested,
                available,
            } => {
                f(requested);
                for item in available.iter_mut() {
                    f(item);
                }
            }
            StartFailure::ModeMismatch {
                requested,
                reported,
            } => {
                f(requested);
                f(reported);
            }
            StartFailure::ResumeFailed { detail } | StartFailure::Other { detail } => f(detail),
            StartFailure::AuthRequired | StartFailure::ProtocolTimeout | StartFailure::Unknown => {}
        }
    }
}

/// What the session is doing. Unknown values decode as [`Activity::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Activity {
    /// A turn is running.
    Busy,
    /// Between turns.
    Idle,
    /// A turn is waiting for an approval answer.
    WaitingOnApproval,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// How a turn ended. Unknown values decode as [`TurnOutcome::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnOutcome {
    /// The agent finished.
    Completed,
    /// The turn was interrupted (by the office, or by a deny that stopped it).
    Interrupted,
    /// The turn failed.
    Failed {
        /// Why, as the runtime said.
        reason: String,
    },
    /// The model refused.
    Refused,
    /// A per-turn limit stopped it.
    Limit {
        /// Which limit.
        kind: LimitKind,
    },
    /// The account's quota ran out.
    Quota,
    /// The runtime lost its sign-in during the turn (e.g. Antigravity's "authentication
    /// required").
    SignedOut,
    /// The runtime ended the turn in a way this build doesn't know.
    #[serde(other)]
    Unknown,
}

impl TurnOutcome {
    pub(super) fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        match self {
            TurnOutcome::Failed { reason } => f(reason),
            TurnOutcome::Completed
            | TurnOutcome::Interrupted
            | TurnOutcome::Refused
            | TurnOutcome::Limit { kind: _ }
            | TurnOutcome::Quota
            | TurnOutcome::SignedOut
            | TurnOutcome::Unknown => {}
        }
    }
}

/// Which per-turn limit ended a turn. Unknown values decode as [`LimitKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LimitKind {
    /// The model's output limit.
    MaxTokens,
    /// The runtime's limit on agentic turns.
    MaxTurns,
    /// The runtime's own time limit.
    Timeout,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// What kind of [`SessionEvent::Notice`]. Unknown values decode as [`NoticeKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NoticeKind {
    /// The runtime is retrying (e.g. an API call).
    Retry,
    /// A warning.
    Warning,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}
