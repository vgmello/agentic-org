//! Looking up an operation by its id (X-02 §4.2), after a dropped channel left its outcome
//! unknown.
//!
//! The operation id is the write's existing field: `command_id` for organization commands
//! ([`crate::org`]), `change_id` for work writes ([`OperationKind::Work`]) and `msg.send`'s `id`
//! for messages ([`crate::msg`]). The lookup names the kind so a client can retrieve an outcome
//! without keeping the request body when that lookup is served. See [`OperationGet`] for
//! current availability.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::Call;
use crate::error::ApiError;

/// `operation.get`: the stored outcome of an operation, for the principal that made it (in v1.0,
/// the owner).
///
/// The current office worker returns [`crate::ErrorCode::Unsupported`] for valid,
/// grant-authorized organization lookups until B2 restores them. Retrying the original
/// organization command with the same `command_id` and parameters still returns its retained
/// answer.
///
/// When lookup is served, errors include `not_found` (no such operation for this principal),
/// `expired` (past its dedup window), `invalid_request` (an unknown `kind`), and
/// `not_authorized`. A queued outcome arrives with relay (X-05).
pub struct OperationGet;

/// Which table an operation id is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// An organization command, by its `command_id`.
    Org,
    /// A work write, by its `change_id`.
    Work,
    /// A message, by `msg.send`'s `id`.
    Msg,
}

/// `operation.get`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationGetParams {
    pub kind: OperationKind,
    pub id: String,
}

/// `operation.get`'s result: `{"status": "committed", "result": ...}` with the stored answer
/// (the call's own result type, e.g. [`crate::CommandAccepted`]), or
/// `{"status": "rejected", "refusal": {"code", "message"}}` with the stored refusal.
///
/// A status this build doesn't know (e.g. `queued`, once relay adds it) decodes as
/// [`OperationOutcome::Unknown`]; look again later. It is never sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[non_exhaustive]
pub enum OperationOutcome {
    Committed {
        result: Value,
    },
    Rejected {
        refusal: ApiError,
    },
    /// A status this build doesn't know. It is never sent: serializing it fails.
    #[serde(other, skip_serializing)]
    Unknown,
}

impl Call for OperationGet {
    const METHOD: &'static str = "operation.get";
    type Params = OperationGetParams;
    type Result = OperationOutcome;
}
