//! Consumer-owned runtime and dispatch ports for [`crate::TessService`] (`TASK-80.4` Part B).
//!
//! Keeps `org-tess` decoupled from `org-office-sessions` and `org-office-server` internals while
//! preserving a single choke-point for Tess method classification and human-grant requirements.

use org_proto::sessions::ActivityPhase;
use org_proto::tess::{
    SuggestionApply, SuggestionCreate, SuggestionDismiss, SuggestionInspect, SuggestionList,
    SuggestionWithdraw, TessAsk, TessSessionReset, TessTurnCancel, TessTurnEvents, UiPresent,
};
use org_proto::{ApiError, Request, Response};

/// Session handle returned when a live Ask Tess turn starts on `tess:ask:<caller_id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TessStartedSession {
    pub session_id: String,
}

/// Coarse terminal outcome forwarded from `SessionStreamHub` into an Ask Tess turn buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TessTurnEndOutcome {
    Completed,
    Interrupted,
    Failed,
}

/// Post-redaction session stream event forwarded from `SessionStreamHub` for `tess:ask:<caller_id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TessStreamInput {
    Activity {
        phase: Option<ActivityPhase>,
        active_tool_name: Option<String>,
        active_tool_started_at: Option<String>,
    },
    Delta {
        text: String,
    },
    TurnEnded {
        outcome: TessTurnEndOutcome,
    },
}

/// Consumer-owned port bridging `org-tess` to the office's `SessionManager`.
pub trait TessRuntimePort: Send + Sync {
    /// Checks whether the Chief of Staff currently has a configured, ready runtime/provider route.
    /// Returns `Err(human_readable_reason)` when Ask Tess should return an honest `fallback`.
    fn check_readiness(&self, caller_id: &str, tess_id: &str) -> Result<(), String>;

    /// Dispatches a turn for `caller_id` on Tess's isolated `tess:ask:<caller_id>` partition.
    fn start_ask_turn(
        &self,
        caller_id: &str,
        tess_id: &str,
        command_id: &str,
        instruction: &str,
    ) -> Result<TessStartedSession, ApiError>;

    /// Interrupts Tess's live turn ONLY if Tess is currently serving `caller_id`'s partition.
    /// Returns `Ok(true)` if an interrupt was delivered, `Ok(false)` if no live turn for
    /// `caller_id` was running.
    fn cancel_ask_turn(&self, caller_id: &str, tess_id: &str) -> Result<bool, ApiError>;

    /// Stops Tess's warm process ONLY if Tess is currently serving `caller_id`'s partition, and
    /// clears `caller_id`'s stored partition thread so the next turn starts fresh.
    /// Never touches another caller's partition or `tess:wake`.
    fn reset_caller_session(&self, caller_id: &str, tess_id: &str) -> Result<bool, ApiError>;

    /// Returns `true` when Tess still has a live or starting session serving `caller_id`'s
    /// `tess:ask:<caller_id>` partition. Used by `tess.turn_events` to avoid stranding SSE
    /// streams if the underlying session crashes or stops without emitting `TurnEnded`.
    fn is_partition_active(&self, _caller_id: &str, _tess_id: &str) -> bool {
        true
    }
}

/// Callback executing a single target RPC strictly under the verified human caller's own
/// `via_grant: true` HostChannel authority during two-phase `suggestion.apply`.
pub type ApplyExecFn<'a> = dyn Fn(&str, &Request) -> Response + Send + Sync + 'a;

/// Whether `request` belongs to the Chief of Staff `suggestion.*`, `ui.present`, or `tess.*`
/// RPC family.
pub fn is_tess_method(request: &Request) -> bool {
    request.is::<SuggestionCreate>()
        || request.is::<SuggestionList>()
        || request.is::<SuggestionInspect>()
        || request.is::<SuggestionWithdraw>()
        || request.is::<SuggestionDismiss>()
        || request.is::<SuggestionApply>()
        || request.is::<UiPresent>()
        || request.is::<TessAsk>()
        || request.is::<TessTurnEvents>()
        || request.is::<TessTurnCancel>()
        || request.is::<TessSessionReset>()
}

/// Whether `request` requires a verified human HostChannel grant (`via_grant: true`) and may
/// NEVER execute over an agent socket or delegated run.
pub fn requires_human_grant(request: &Request) -> bool {
    request.is::<SuggestionApply>()
        || request.is::<TessAsk>()
        || request.is::<TessTurnEvents>()
        || request.is::<TessTurnCancel>()
        || request.is::<TessSessionReset>()
}
