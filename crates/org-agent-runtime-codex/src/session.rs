//! The Codex `app-server` session: [`CodexSession`], its state, and the helpers every part
//! of the protocol shares. The protocol's phases live in the submodules.

mod control;
mod driver;
mod handshake;
mod notifications;
mod responses;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::Value;

use org_agent_runtime::{
    Activity, ApprovalId, CostBasis, Output, SessionEvent, SessionId, Usage, UsageScope,
};

use crate::plan::CodexThreadConfig;
use crate::protocol::CodexTokens;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    OpeningInit,
    OpeningThread,
    Idle,
    Busy,
    Failed,
    Closing,
    Ended,
}

/// Pure sans-I/O JSON-RPC 2.0 protocol state machine for `codex app-server --stdio`.
pub(crate) struct CodexSession {
    cfg: CodexThreadConfig,
    phase: Phase,
    turn_open: bool,
    next_id: u64,
    init_req_id: Option<u64>,
    thread_req_id: Option<u64>,
    turn_start_req_id: Option<u64>,
    handshake_deadline: Option<Instant>,
    thread_id: Option<SessionId>,
    active_turn_id: Option<String>,
    runtime_version: Option<String>,
    pending_approvals: BTreeMap<ApprovalId, Value>,
    pending_file_changes: BTreeMap<String, Value>,
    active_subagents: BTreeSet<String>,
    turn_tokens: Option<CodexTokens>,
    last_total_tokens: Option<CodexTokens>,
    /// Tracks in-flight `turn/steer` JSON-RPC request ids -> office `delivery_id` (`doc-101`).
    in_flight_deliveries: BTreeMap<u64, String>,
}

impl CodexSession {
    pub(crate) fn new(cfg: CodexThreadConfig) -> Self {
        Self {
            cfg,
            phase: Phase::OpeningInit,
            turn_open: false,
            next_id: 1,
            init_req_id: None,
            thread_req_id: None,
            turn_start_req_id: None,
            handshake_deadline: None,
            thread_id: None,
            active_turn_id: None,
            runtime_version: None,
            pending_approvals: BTreeMap::new(),
            pending_file_changes: BTreeMap::new(),
            active_subagents: BTreeSet::new(),
            turn_tokens: None,
            last_total_tokens: None,
            in_flight_deliveries: BTreeMap::new(),
        }
    }

    fn fail_in_flight_deliveries(&mut self, reason: &str) -> Vec<Output> {
        std::mem::take(&mut self.in_flight_deliveries)
            .into_values()
            .map(|delivery_id| {
                Output::Event(SessionEvent::MidTurnDeliveryFailed {
                    delivery_id,
                    reason: reason.to_owned(),
                })
            })
            .collect()
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

    fn flush_turn_usage(&mut self) -> Option<Output> {
        let tokens = self.turn_tokens.take()?;
        let mut usage = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
        usage.cost_basis = CostBasis::None;
        usage.input = tokens.input;
        usage.output = tokens.output;
        usage.cache_read = tokens.cache_read;
        usage.cache_write = tokens.cache_write;
        usage.reasoning = tokens.reasoning;
        usage.model = self.cfg.model.clone();
        Some(Output::Event(SessionEvent::Usage(usage)))
    }
}
