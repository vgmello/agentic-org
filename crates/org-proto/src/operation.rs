//! Looking up an operation by its id (X-02 §4.2), after a dropped channel left its outcome
//! unknown.
//!
//! The operation id is the write's existing field: `command_id` for organization commands
//! ([`crate::org`]), `change_id` for work writes ([`crate::work`]) and `msg.send`'s `id` for
//! messages ([`crate::msg`]). Each lives in its own table, so the lookup names the kind. A retry
//! with the same id and params gets the stored answer too; a lookup lets a client learn it without
//! keeping the request body.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::Call;
use crate::error::ApiError;

/// `operation.get`: the stored outcome of an operation, for the principal that made it (in v1.0,
/// the owner). Errors: `not_found` (no such operation for this principal), `expired` (past its
/// dedup window), `invalid_request` (an unknown `kind`), `not_authorized`. A queued outcome
/// arrives with relay (X-05); until then a lookup answers only `committed` or `rejected`.
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
    #[serde(other)]
    Unknown,
}

impl Call for OperationGet {
    const METHOD: &'static str = "operation.get";
    type Params = OperationGetParams;
    type Result = OperationOutcome;
}
