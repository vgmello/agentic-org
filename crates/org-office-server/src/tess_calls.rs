//! Thin office-server adapter wiring [`org_tess::TessService`] to [`crate::serve::delegation`]
//! and [`org_office_sessions::SessionManager`].

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use org_office::Office;
use org_office_sessions::partition::TESS_ASK_PREFIX;
use org_office_sessions::{
    SessionManager, SessionStreamEvent, SessionStreamHub, StreamFilter, TurnEnd,
};
use org_proto::providers::{AgentReadinessCode, AgentReadinessGetParams};
use org_proto::sessions::MemberSessionStartParams;
use org_proto::{ApiError, ErrorCode, Request, Response};
use org_tess::{
    ActingForFn, ApplyExecFn, DelegationAccess, DraftCallerFn, TessRuntimePort, TessStartedSession,
    TessStreamInput, TessTurnEndOutcome,
};

use crate::serve::delegation::Access;

fn delegation_policy(call: &str) -> DelegationAccess {
    match crate::serve::delegation::access(call) {
        Access::Open => DelegationAccess::Open,
        Access::CallerOwner => DelegationAccess::CallerOwner,
        Access::CallerHires => DelegationAccess::CallerHires,
        Access::Never => DelegationAccess::Never,
    }
}

struct SessionManagerTessRuntime {
    office: Arc<Office>,
    sessions: Arc<SessionManager>,
}

impl TessRuntimePort for SessionManagerTessRuntime {
    fn check_readiness(&self, caller_id: &str, tess_id: &str) -> Result<(), String> {
        let params = AgentReadinessGetParams {
            employee_id: tess_id.to_owned(),
            task_id: None,
        };
        let readiness = self
            .office
            .agent_readiness_get(caller_id, params)
            .map_err(|_| "Unable to verify Tess runtime readiness right now.".to_owned())?
            .map_err(|err| err.message)?;
        if readiness.ready {
            return Ok(());
        }
        Err(readiness_unavailable_reason(readiness.status).to_owned())
    }

    fn start_ask_turn(
        &self,
        caller_id: &str,
        tess_id: &str,
        command_id: &str,
        instruction: &str,
    ) -> Result<TessStartedSession, ApiError> {
        let params = MemberSessionStartParams {
            command_id: command_id.to_owned(),
            employee_id: tess_id.to_owned(),
            instruction: Some(instruction.to_owned()),
            workspace: None,
        };
        let view = self.sessions.start_session(caller_id, &params)?;
        let session_id = view
            .session_id
            .or(view.run_id)
            .unwrap_or_else(|| command_id.to_owned());
        Ok(TessStartedSession { session_id })
    }

    fn cancel_ask_turn(&self, caller_id: &str, tess_id: &str) -> Result<bool, ApiError> {
        if self
            .sessions
            .bindings()
            .acting_for_caller_id(tess_id)
            .as_deref()
            != Some(caller_id)
        {
            return Ok(false);
        }
        match self.sessions.interrupt_session(tess_id) {
            Ok(_) => Ok(true),
            Err(err) if err.code == ErrorCode::Conflict => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn reset_caller_session(&self, caller_id: &str, tess_id: &str) -> Result<bool, ApiError> {
        if self
            .sessions
            .bindings()
            .acting_for_caller_id(tess_id)
            .as_deref()
            != Some(caller_id)
        {
            return Ok(false);
        }
        match self.sessions.stop_session(tess_id, "tess_session_reset") {
            Ok(_) => Ok(true),
            Err(err) if err.code == ErrorCode::Conflict => Ok(false),
            Err(err) => Err(err),
        }
    }
}

fn readiness_unavailable_reason(status: AgentReadinessCode) -> &'static str {
    match status {
        AgentReadinessCode::NoExecutionConfig => {
            "No runtime or provider connection is configured for Tess yet."
        }
        AgentReadinessCode::ExecutionDisabled => "Tess execution is currently disabled.",
        AgentReadinessCode::ProfileArchived => "Tess's configured model profile is archived.",
        AgentReadinessCode::ModelUnverified => {
            "Tess's configured model is not verified on any bound provider connection."
        }
        AgentReadinessCode::BudgetHeld => "Tess is currently paused by a budget hold.",
        AgentReadinessCode::PoolExhausted => {
            "Provider capacity for Tess is temporarily full; try again shortly."
        }
        AgentReadinessCode::NoReadyConnection => {
            "No ready provider connection is bound on this office for Tess."
        }
        _ => "Tess runtime is not ready right now.",
    }
}

pub(crate) struct TessService(org_tess::TessService);

impl TessService {
    pub(crate) fn new(office: Arc<Office>, acting_for: ActingForFn) -> Self {
        Self(org_tess::TessService::new(
            office,
            acting_for,
            delegation_policy,
        ))
    }

    pub(crate) fn with_draft_caller(self, draft_caller: DraftCallerFn) -> Self {
        Self(self.0.with_draft_caller(draft_caller))
    }

    pub(crate) fn with_session_manager(
        self,
        office: Arc<Office>,
        sessions: Arc<SessionManager>,
    ) -> Self {
        let runtime = Arc::new(SessionManagerTessRuntime { office, sessions });
        Self(self.0.with_runtime(runtime))
    }

    #[cfg(test)]
    pub(crate) fn with_runtime(self, runtime: Arc<dyn TessRuntimePort>) -> Self {
        Self(self.0.with_runtime(runtime))
    }

    pub(crate) fn ingest_stream_event(&self, caller_id: &str, at: String, input: TessStreamInput) {
        self.0.ingest_stream_event(caller_id, at, input);
    }

    #[cfg(test)]
    pub(crate) fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        self.0.dispatch(member, request)
    }

    pub(crate) fn dispatch_with_context(
        &self,
        member: Option<&str>,
        via_grant: bool,
        stopping: &AtomicBool,
        apply_exec: Option<&ApplyExecFn<'_>>,
        request: &Request,
    ) -> Option<Response> {
        self.0
            .dispatch_with_context(member, via_grant, stopping, apply_exec, request)
    }
}

/// Bridges post-redaction `SessionStreamHub` frames from `tess:ask:<caller_id>` partitions into
/// `TessService`'s caller-scoped Ask Tess turn buffers. Holds `svc` weakly so it unsubscribes
/// automatically when the service drops.
pub(crate) fn follow_tess_stream(
    svc: &Arc<TessService>,
    office: &Arc<Office>,
    hub: &SessionStreamHub,
) -> bool {
    let weak = Arc::downgrade(svc);
    let office = Arc::clone(office);
    hub.add_sink(StreamFilter::All, move |frame| {
        let Some(svc) = weak.upgrade() else {
            return false;
        };
        let Some(caller_id) = frame
            .key
            .partition
            .as_deref()
            .and_then(|p| p.strip_prefix(TESS_ASK_PREFIX))
        else {
            return true;
        };
        let is_tess = office
            .chief_of_staff()
            .ok()
            .flatten()
            .is_some_and(|tess| tess.id.as_str() == frame.key.employee_id);
        if !is_tess {
            return true;
        }
        let input = match &frame.event {
            SessionStreamEvent::Activity(act) => TessStreamInput::Activity {
                phase: act.phase,
                active_tool_name: act.active_tool_name.clone(),
                active_tool_started_at: act.active_tool_started_at.clone(),
            },
            SessionStreamEvent::Delta { text } => TessStreamInput::Delta { text: text.clone() },
            SessionStreamEvent::TurnEnded { outcome } => TessStreamInput::TurnEnded {
                outcome: match outcome {
                    TurnEnd::Completed => TessTurnEndOutcome::Completed,
                    TurnEnd::Interrupted => TessTurnEndOutcome::Interrupted,
                    TurnEnd::Failed => TessTurnEndOutcome::Failed,
                },
            },
            _ => return true,
        };
        svc.ingest_stream_event(caller_id, frame.at.clone(), input);
        true
    })
}

#[cfg(test)]
#[path = "tess_calls/tests.rs"]
mod tests;
