//! The Antigravity ACP session: [`AgySession`], its state, and the helpers every part of the
//! protocol shares. The protocol's phases live in the submodules.

mod control;
mod driver;
mod handshake;
mod responses;
mod updates;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::Value;

use org_agent_runtime::{
    Activity, ApprovalId, CostBasis, Output, SessionEvent, SessionId, Usage, UsageScope,
};

use crate::plan::AgySessionConfig;
use crate::protocol::AcpUsage;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    OpeningInit,
    OpeningSession,
    OpeningMode,
    OpeningModel,
    Idle,
    Busy,
    Failed,
    Closing,
    Ended,
}

#[derive(Debug, Clone)]
struct PendingApproval {
    raw_id: Value,
    allow_option_id: Option<String>,
    deny_option_id: Option<String>,
    tool_call_id: Option<String>,
}

/// One live `session/prompt` RPC slot in an active turn (initial prompt + any mid-turn steers).
#[derive(Debug, Clone)]
struct PromptSlot {
    req_id: u64,
    delivery_id: Option<String>,
    acked: bool,
}

/// Pure sans-I/O ACP JSON-RPC 2.0 protocol state machine for `agy acp --stdio`.
pub(crate) struct AgySession {
    cfg: AgySessionConfig,
    phase: Phase,
    turn_open: bool,
    next_id: u64,
    init_req_id: Option<u64>,
    session_req_id: Option<u64>,
    mode_req_id: Option<u64>,
    model_req_id: Option<u64>,
    handshake_deadline: Option<Instant>,
    session_id: Option<SessionId>,
    runtime_version: Option<String>,
    reported_mode: Option<String>,
    active_model: Option<String>,
    /// Ordered chain of live `session/prompt` slots for the open turn (`[0]` is oldest,
    /// `last()` is newest owning prompt). Replacing split maps prevents chained-steer corruption.
    prompt_chain: Vec<PromptSlot>,
    pending_approvals: BTreeMap<ApprovalId, PendingApproval>,
    turn_text_buf: String,
    /// Authoritative finalized token usage summed across `session/prompt` `result.usage` payloads.
    confirmed_usage: AcpUsage,
    saw_confirmed_usage: bool,
    /// Current-segment streaming `usage_update` snapshot (cleared when prompt `result.usage`
    /// finalizes the segment; only folded into `flush_turn_usage` if unfinalized on crash/EOF).
    streamed_usage: Option<AcpUsage>,
}

impl AgySession {
    pub(crate) fn new(cfg: AgySessionConfig) -> Self {
        Self {
            cfg,
            phase: Phase::OpeningInit,
            turn_open: false,
            next_id: 1,
            init_req_id: None,
            session_req_id: None,
            mode_req_id: None,
            model_req_id: None,
            handshake_deadline: None,
            session_id: None,
            runtime_version: None,
            reported_mode: None,
            active_model: None,
            prompt_chain: Vec::new(),
            pending_approvals: BTreeMap::new(),
            turn_text_buf: String::new(),
            confirmed_usage: AcpUsage::default(),
            saw_confirmed_usage: false,
            streamed_usage: None,
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn busy_status(&self) -> Activity {
        if self.pending_approvals.is_empty() {
            Activity::Busy
        } else {
            Activity::WaitingOnApproval
        }
    }

    fn cancel_pending_approvals(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.pending_approvals)
            .into_keys()
            .map(|id| Output::Event(SessionEvent::ApprovalCancelled { id }))
            .collect()
    }

    fn fail_pending_mid_turn_deliveries(&mut self, reason: &str) -> Vec<Output> {
        std::mem::take(&mut self.prompt_chain)
            .into_iter()
            .filter_map(|slot| (!slot.acked).then_some(slot.delivery_id).flatten())
            .map(|delivery_id| {
                Output::Event(SessionEvent::MidTurnDeliveryFailed {
                    delivery_id,
                    reason: reason.to_owned(),
                })
            })
            .collect()
    }

    fn flush_assistant_text(&mut self) -> Option<Output> {
        if self.turn_text_buf.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.turn_text_buf);
        Some(Output::Event(SessionEvent::AssistantText {
            message_id: None,
            text,
        }))
    }

    fn flush_turn_usage(&mut self) -> Option<Output> {
        let mut total = std::mem::take(&mut self.confirmed_usage);
        let saw_confirmed = std::mem::replace(&mut self.saw_confirmed_usage, false);
        let streamed = self.streamed_usage.take();
        if let Some(est) = &streamed {
            total.add_assign(est);
        }
        if !saw_confirmed && streamed.is_none() && total.is_zero() {
            return None;
        }
        let mut u = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
        u.input = total.input;
        u.output = total.output;
        u.cache_read = total.cache_read;
        u.cache_write = total.cache_write;
        u.reasoning = total.reasoning;
        u.cost_basis = CostBasis::None;
        u.model = self.active_model.clone().or_else(|| self.cfg.model.clone());
        Some(Output::Event(SessionEvent::Usage(u)))
    }
}

fn is_opening(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::OpeningInit | Phase::OpeningSession | Phase::OpeningMode | Phase::OpeningModel
    )
}
