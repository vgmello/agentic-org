//! Caller-scoped Ask Tess turn dispatch, bounded post-redaction event hub (`Last-Event-ID`
//! resume), turn cancellation, and per-caller session reset (`TASK-80.4` Part B).

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use org_office::organization_family::records::EmployeeStatus;
use org_proto::sessions::ActivityPhase;
use org_proto::tess::{
    SuggestionView, TessAskParams, TessAskResult, TessAskStatus, TessSessionResetParams,
    TessSessionResetResult, TessToolState, TessTurnCancelParams, TessTurnCancelResult,
    TessTurnEvent, TessTurnEventKind, TessTurnEventsParams, TessTurnEventsResult, TessTurnOutcome,
    UiBlock, MAX_TESS_EVENTS_WAIT_MS,
};
use org_proto::{format_rfc3339_ms, ApiError, ErrorCode};

use crate::ports::{TessStreamInput, TessTurnEndOutcome};
use crate::viewport::{compose_ask_instruction, validate_and_render_viewport, validate_prompt};
use crate::{unavailable, CallerContext, TessService};

/// Hard upper bound on runtime-ephemeral Ask Tess turns retained per office process.
const MAX_STORED_TURNS: usize = 128;
/// Hard upper bound on runtime-ephemeral Ask Tess turns retained per caller.
const MAX_TURNS_PER_CALLER: usize = 16;
/// Hard upper bound on sequenced frames buffered per Ask Tess turn.
const MAX_EVENTS_PER_TURN: usize = 1_024;
/// Condvar wait step while honoring connection stop flags (`channel_stop`).
const WAIT_TICK: Duration = Duration::from_millis(50);

/// Canonical Chief of Staff partition key for one human caller (`tess:ask:<caller_id>`).
pub fn ask_partition(caller_id: &str) -> String {
    format!("tess:ask:{caller_id}")
}

#[derive(Debug, Clone)]
struct ActiveTool {
    id: String,
    label: String,
    started_at_ms: u64,
}

#[derive(Debug, Clone)]
struct StoredTurn {
    turn_id: String,
    caller_id: String,
    partition: String,
    session_id: Option<String>,
    active: bool,
    next_seq: u64,
    lagged: bool,
    events: VecDeque<TessTurnEvent>,
    active_tool: Option<ActiveTool>,
    tool_seq: u64,
    follow_ups: Vec<String>,
}

impl StoredTurn {
    fn new(turn_id: String, caller_id: String, at: String) -> Self {
        let partition = ask_partition(&caller_id);
        let mut events = VecDeque::new();
        events.push_back(TessTurnEvent {
            seq: 1,
            at,
            event: TessTurnEventKind::TurnStarted {
                turn_id: turn_id.clone(),
            },
        });
        Self {
            turn_id,
            caller_id,
            partition,
            session_id: None,
            active: true,
            next_seq: 2,
            lagged: false,
            events,
            active_tool: None,
            tool_seq: 0,
            follow_ups: Vec::new(),
        }
    }

    fn push_event(&mut self, at: String, event: TessTurnEventKind) {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.events.len() >= MAX_EVENTS_PER_TURN {
            self.events.pop_front();
            self.lagged = true;
        }
        self.events.push_back(TessTurnEvent { seq, at, event });
    }

    fn finish_active_tool(&mut self, at: String, now_ms: u64, state: TessToolState) {
        let Some(tool) = self.active_tool.take() else {
            return;
        };
        let duration_ms = now_ms.saturating_sub(tool.started_at_ms);
        self.push_event(
            at,
            TessTurnEventKind::ToolProgress {
                id: tool.id,
                label: tool.label,
                state,
                phase: None,
                started_at: Some(tool.started_at_ms),
                duration_ms: Some(duration_ms),
            },
        );
    }

    fn record_ui_follow_ups(&mut self, ui_block: &UiBlock) {
        for item in &ui_block.follow_ups {
            if self.follow_ups.len() >= 3 {
                break;
            }
            if !self.follow_ups.contains(&item.prompt) {
                self.follow_ups.push(item.prompt.clone());
            }
        }
    }

    fn head_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }

    fn slice_after(&self, after_seq: u64) -> (Vec<TessTurnEvent>, bool) {
        let oldest = self.events.front().map_or(self.next_seq, |ev| ev.seq);
        let lagged = self.lagged && after_seq.saturating_add(1) < oldest;
        let events = self
            .events
            .iter()
            .filter(|ev| ev.seq > after_seq)
            .cloned()
            .collect();
        (events, lagged)
    }
}

#[derive(Default)]
struct TurnRegistry {
    turns: VecDeque<StoredTurn>,
    reset_pending: HashSet<String>,
}

impl TurnRegistry {
    fn remove_first(&mut self, pred: impl Fn(&StoredTurn) -> bool) -> bool {
        self.turns
            .iter()
            .position(pred)
            .is_some_and(|idx| self.turns.remove(idx).is_some())
    }

    fn enforce_bounds(&mut self, caller_id: &str) {
        while self
            .turns
            .iter()
            .filter(|t| t.caller_id == caller_id)
            .count()
            >= MAX_TURNS_PER_CALLER
        {
            let evicted = self.remove_first(|t| t.caller_id == caller_id && !t.active)
                || self.remove_first(|t| t.caller_id == caller_id);
            if !evicted {
                break;
            }
        }
        while self.turns.len() >= MAX_STORED_TURNS {
            let evicted = self.remove_first(|t| t.caller_id == caller_id && !t.active)
                || self.remove_first(|t| !t.active)
                || self.remove_first(|t| t.caller_id == caller_id);
            if !evicted {
                break;
            }
        }
    }

    fn latest_active_mut(&mut self, caller_id: &str) -> Option<&mut StoredTurn> {
        self.turns
            .iter_mut()
            .rev()
            .find(|t| t.caller_id == caller_id && t.active)
    }

    fn find_for_caller(&self, caller_id: &str, turn_id: &str) -> Option<&StoredTurn> {
        self.turns
            .iter()
            .rev()
            .find(|t| t.turn_id == turn_id && t.caller_id == caller_id)
    }

    fn find_for_caller_mut(&mut self, caller_id: &str, turn_id: &str) -> Option<&mut StoredTurn> {
        self.turns
            .iter_mut()
            .rev()
            .find(|t| t.turn_id == turn_id && t.caller_id == caller_id)
    }
}

/// Thread-safe bounded registry of caller-scoped Ask Tess turns and their live event streams.
#[derive(Default)]
pub(crate) struct TurnHub {
    state: Mutex<TurnRegistry>,
    wake: Condvar,
}

impl TurnHub {
    fn begin_turn(&self, caller_id: &str, turn_id: String, at: String) -> (String, bool) {
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let after_reset = reg.reset_pending.remove(caller_id);
        reg.enforce_bounds(caller_id);
        let turn = StoredTurn::new(turn_id, caller_id.to_owned(), at);
        let partition = turn.partition.clone();
        reg.turns.push_back(turn);
        drop(reg);
        self.wake.notify_all();
        (partition, after_reset)
    }

    fn attach_session(&self, caller_id: &str, turn_id: &str, session_id: String) {
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(turn) = reg.find_for_caller_mut(caller_id, turn_id) {
            turn.session_id = Some(session_id);
        }
    }

    fn abort_unstarted(&self, caller_id: &str, turn_id: &str, restore_reset: bool) {
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        reg.turns
            .retain(|t| !(t.turn_id == turn_id && t.caller_id == caller_id));
        if restore_reset {
            reg.reset_pending.insert(caller_id.to_owned());
        }
        drop(reg);
        self.wake.notify_all();
    }

    pub(crate) fn ingest_stream(
        &self,
        caller_id: &str,
        at: String,
        now_ms: u64,
        input: TessStreamInput,
    ) {
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let Some(turn) = reg.latest_active_mut(caller_id) else {
            return;
        };
        apply_stream_input(turn, at, now_ms, input);
        drop(reg);
        self.wake.notify_all();
    }

    pub(crate) fn publish_ui_block(&self, caller_id: &str, at: String, ui_block: UiBlock) {
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let Some(turn) = reg.latest_active_mut(caller_id) else {
            return;
        };
        turn.record_ui_follow_ups(&ui_block);
        turn.push_event(at, TessTurnEventKind::UiBlock { ui_block });
        drop(reg);
        self.wake.notify_all();
    }

    pub(crate) fn publish_suggestion_created(
        &self,
        caller_id: &str,
        at: String,
        suggestion: SuggestionView,
        ui_block: UiBlock,
    ) {
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let Some(turn) = reg.latest_active_mut(caller_id) else {
            return;
        };
        turn.push_event(
            at,
            TessTurnEventKind::SuggestionCreated {
                suggestion: Box::new(suggestion),
                ui_block,
            },
        );
        drop(reg);
        self.wake.notify_all();
    }

    fn verify_owned_turn(&self, caller_id: &str, turn_id: &str) -> Result<bool, ApiError> {
        let reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let turn = reg
            .find_for_caller(caller_id, turn_id)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "turn not found"))?;
        Ok(turn.active)
    }

    fn mark_cancelled(
        &self,
        caller_id: &str,
        turn_id: &str,
        at: String,
        now_ms: u64,
    ) -> Result<bool, ApiError> {
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let turn = reg
            .find_for_caller_mut(caller_id, turn_id)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "turn not found"))?;
        if !turn.active {
            return Ok(false);
        }
        turn.finish_active_tool(at.clone(), now_ms, TessToolState::Error);
        let id = turn.turn_id.clone();
        turn.push_event(
            at,
            TessTurnEventKind::TurnDone {
                turn_id: id,
                outcome: TessTurnOutcome::Cancelled,
                follow_ups: Vec::new(),
            },
        );
        turn.active = false;
        drop(reg);
        self.wake.notify_all();
        Ok(true)
    }

    fn reset_caller(&self, caller_id: &str, at: String, now_ms: u64) {
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut keep_cancelled = HashSet::new();
        for turn in reg.turns.iter_mut().filter(|t| t.caller_id == caller_id) {
            if turn.active {
                turn.finish_active_tool(at.clone(), now_ms, TessToolState::Error);
                let id = turn.turn_id.clone();
                turn.push_event(
                    at.clone(),
                    TessTurnEventKind::TurnDone {
                        turn_id: id.clone(),
                        outcome: TessTurnOutcome::Cancelled,
                        follow_ups: Vec::new(),
                    },
                );
                turn.active = false;
                keep_cancelled.insert(id);
            }
        }
        reg.turns
            .retain(|t| t.caller_id != caller_id || keep_cancelled.contains(&t.turn_id));
        reg.reset_pending.insert(caller_id.to_owned());
        drop(reg);
        self.wake.notify_all();
    }

    fn poll_events(
        &self,
        caller_id: &str,
        turn_id: &str,
        after_seq: u64,
        wait_ms: u64,
        stopping: &AtomicBool,
    ) -> Result<TessTurnEventsResult, ApiError> {
        let deadline = (wait_ms > 0)
            .then(|| Instant::now() + Duration::from_millis(wait_ms.min(MAX_TESS_EVENTS_WAIT_MS)));
        let mut reg = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            let turn = reg
                .find_for_caller(caller_id, turn_id)
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "turn not found"))?;
            let (events, lagged) = turn.slice_after(after_seq);
            let done = !turn.active;
            if !events.is_empty() || done || stopping.load(Ordering::SeqCst) {
                return Ok(TessTurnEventsResult {
                    turn_id: turn.turn_id.clone(),
                    partition: turn.partition.clone(),
                    events,
                    head_seq: turn.head_seq(),
                    done,
                    lagged,
                });
            }
            let Some(until) = deadline else {
                return Ok(TessTurnEventsResult {
                    turn_id: turn.turn_id.clone(),
                    partition: turn.partition.clone(),
                    events,
                    head_seq: turn.head_seq(),
                    done,
                    lagged,
                });
            };
            let now = Instant::now();
            if now >= until {
                return Ok(TessTurnEventsResult {
                    turn_id: turn.turn_id.clone(),
                    partition: turn.partition.clone(),
                    events,
                    head_seq: turn.head_seq(),
                    done,
                    lagged,
                });
            }
            let step = until.saturating_duration_since(now).min(WAIT_TICK);
            let (next_reg, _) = self
                .wake
                .wait_timeout(reg, step)
                .unwrap_or_else(|p| p.into_inner());
            reg = next_reg;
        }
    }
}

fn apply_stream_input(turn: &mut StoredTurn, at: String, now_ms: u64, input: TessStreamInput) {
    match input {
        TessStreamInput::Activity {
            phase,
            active_tool_name,
            ..
        } => {
            if phase == Some(ActivityPhase::RunningTool) {
                let label = active_tool_name
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "Running tool".to_owned());
                if turn
                    .active_tool
                    .as_ref()
                    .is_some_and(|cur| cur.label == label)
                {
                    return;
                }
                turn.finish_active_tool(at.clone(), now_ms, TessToolState::Done);
                turn.tool_seq = turn.tool_seq.saturating_add(1);
                let id = format!("tool-{}", turn.tool_seq);
                turn.active_tool = Some(ActiveTool {
                    id: id.clone(),
                    label: label.clone(),
                    started_at_ms: now_ms,
                });
                turn.push_event(
                    at,
                    TessTurnEventKind::ToolProgress {
                        id,
                        label,
                        state: TessToolState::Running,
                        phase: Some("running_tool".to_owned()),
                        started_at: Some(now_ms),
                        duration_ms: None,
                    },
                );
            } else {
                turn.finish_active_tool(at, now_ms, TessToolState::Done);
            }
        }
        TessStreamInput::Delta { text } => {
            if !text.is_empty() {
                turn.push_event(at, TessTurnEventKind::TextDelta { delta: text });
            }
        }
        TessStreamInput::TurnEnded { outcome } => {
            let tool_state = match outcome {
                TessTurnEndOutcome::Completed => TessToolState::Done,
                TessTurnEndOutcome::Interrupted | TessTurnEndOutcome::Failed => {
                    TessToolState::Error
                }
            };
            turn.finish_active_tool(at.clone(), now_ms, tool_state);
            let id = turn.turn_id.clone();
            match outcome {
                TessTurnEndOutcome::Completed => {
                    let follow_ups = std::mem::take(&mut turn.follow_ups);
                    turn.push_event(
                        at,
                        TessTurnEventKind::TurnDone {
                            turn_id: id,
                            outcome: TessTurnOutcome::Completed,
                            follow_ups,
                        },
                    );
                }
                TessTurnEndOutcome::Interrupted => {
                    turn.push_event(
                        at,
                        TessTurnEventKind::TurnDone {
                            turn_id: id,
                            outcome: TessTurnOutcome::Interrupted,
                            follow_ups: Vec::new(),
                        },
                    );
                }
                TessTurnEndOutcome::Failed => {
                    turn.push_event(
                        at,
                        TessTurnEventKind::TurnError {
                            message: "Tess could not complete this turn.".to_owned(),
                        },
                    );
                }
            }
            turn.active = false;
        }
    }
}

fn require_human_grant<'a>(
    context: &'a CallerContext,
    via_grant: bool,
    verb: &str,
) -> Result<&'a str, ApiError> {
    if !via_grant {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            format!("{verb} requires a verified human HostChannel grant"),
        ));
    }
    match context {
        CallerContext::HumanCaller { caller_id, .. } => Ok(caller_id.as_str()),
        CallerContext::TessActingFor { .. } | CallerContext::TessWake => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            format!("only an active human caller may invoke {verb}"),
        )),
    }
}

fn resolve_active_tess_id(svc: &TessService) -> Result<Result<String, String>, ApiError> {
    let Some(tess) = svc.office().chief_of_staff().map_err(|e| {
        unavailable(
            "tess_calls.chief_of_staff",
            &e,
            "unable to resolve Chief of Staff",
        )
    })?
    else {
        return Ok(Err(
            "Chief of Staff (Tess) is not configured in this organization.".to_owned(),
        ));
    };
    if tess.status != EmployeeStatus::Active {
        return Ok(Err(
            "Chief of Staff (Tess) is currently inactive in this office.".to_owned(),
        ));
    }
    Ok(Ok(tess.id.to_string()))
}

fn fallback_result(partition: String, reason: impl Into<String>) -> TessAskResult {
    TessAskResult {
        status: TessAskStatus::Fallback,
        turn_id: None,
        session_id: None,
        partition,
        unavailable_reason: Some(reason.into()),
        fallback_answer: None,
    }
}

pub(crate) fn handle_ask(
    svc: &TessService,
    context: &CallerContext,
    via_grant: bool,
    params: TessAskParams,
) -> Result<TessAskResult, ApiError> {
    let caller_id = require_human_grant(context, via_grant, "tess.ask")?;
    let prompt = validate_prompt(&params.prompt)?;
    let viewport_block = params
        .viewport_context
        .as_ref()
        .map(validate_and_render_viewport)
        .transpose()?;
    let partition = ask_partition(caller_id);

    let tess_id = match resolve_active_tess_id(svc)? {
        Ok(id) => id,
        Err(reason) => return Ok(fallback_result(partition, reason)),
    };
    let Some(runtime) = svc.runtime.as_deref() else {
        return Ok(fallback_result(
            partition,
            "No live runtime is attached for Tess on this office.",
        ));
    };
    if let Err(reason) = runtime.check_readiness(caller_id, &tess_id) {
        return Ok(fallback_result(partition, reason));
    }

    let turn_id = svc.next_uuid_v7();
    let now_rfc = timestamp_rfc3339(svc.now_ms());
    let (partition, after_reset) = svc.turns.begin_turn(caller_id, turn_id.clone(), now_rfc);
    let instruction = compose_ask_instruction(&prompt, viewport_block.as_deref(), after_reset);
    let command_id = svc.next_uuid_v7();

    match runtime.start_ask_turn(caller_id, &tess_id, &command_id, &instruction) {
        Ok(started) => {
            svc.turns
                .attach_session(caller_id, &turn_id, started.session_id.clone());
            Ok(TessAskResult {
                status: TessAskStatus::Streaming,
                turn_id: Some(turn_id),
                session_id: Some(started.session_id),
                partition,
                unavailable_reason: None,
                fallback_answer: None,
            })
        }
        Err(err) if matches!(err.code, ErrorCode::Unavailable | ErrorCode::NotReady) => {
            svc.turns.abort_unstarted(caller_id, &turn_id, after_reset);
            Ok(fallback_result(partition, err.message))
        }
        Err(err) => {
            svc.turns.abort_unstarted(caller_id, &turn_id, after_reset);
            Err(err)
        }
    }
}

pub(crate) fn handle_turn_events(
    svc: &TessService,
    context: &CallerContext,
    via_grant: bool,
    stopping: &AtomicBool,
    params: TessTurnEventsParams,
) -> Result<TessTurnEventsResult, ApiError> {
    let caller_id = require_human_grant(context, via_grant, "tess.turn_events")?;
    let turn_id = params.turn_id.trim();
    if turn_id.is_empty() || turn_id.len() > 128 {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid `turn_id`",
        ));
    }
    svc.turns.poll_events(
        caller_id,
        turn_id,
        params.after_seq.unwrap_or(0),
        params.wait_ms.unwrap_or(0),
        stopping,
    )
}

pub(crate) fn handle_turn_cancel(
    svc: &TessService,
    context: &CallerContext,
    via_grant: bool,
    params: TessTurnCancelParams,
) -> Result<TessTurnCancelResult, ApiError> {
    let caller_id = require_human_grant(context, via_grant, "tess.turn_cancel")?;
    let turn_id = params.turn_id.trim();
    if turn_id.is_empty() || turn_id.len() > 128 {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid `turn_id`",
        ));
    }
    let active = svc.turns.verify_owned_turn(caller_id, turn_id)?;
    let partition = ask_partition(caller_id);
    if !active {
        return Ok(TessTurnCancelResult {
            turn_id: turn_id.to_owned(),
            cancelled: false,
            partition,
        });
    }
    if let (Ok(Ok(tess_id)), Some(runtime)) = (resolve_active_tess_id(svc), svc.runtime.as_deref())
    {
        let _ = runtime.cancel_ask_turn(caller_id, &tess_id)?;
    }
    let now_ms = svc.now_ms();
    let cancelled =
        svc.turns
            .mark_cancelled(caller_id, turn_id, timestamp_rfc3339(now_ms), now_ms)?;
    Ok(TessTurnCancelResult {
        turn_id: turn_id.to_owned(),
        cancelled,
        partition,
    })
}

pub(crate) fn handle_session_reset(
    svc: &TessService,
    context: &CallerContext,
    via_grant: bool,
    _params: TessSessionResetParams,
) -> Result<TessSessionResetResult, ApiError> {
    let caller_id = require_human_grant(context, via_grant, "tess.session_reset")?;
    let partition = ask_partition(caller_id);
    let mut stopped_live_session = false;
    if let (Ok(Ok(tess_id)), Some(runtime)) = (resolve_active_tess_id(svc), svc.runtime.as_deref())
    {
        stopped_live_session = runtime.reset_caller_session(caller_id, &tess_id)?;
    }
    let now_ms = svc.now_ms();
    svc.turns
        .reset_caller(caller_id, timestamp_rfc3339(now_ms), now_ms);
    Ok(TessSessionResetResult {
        reset: true,
        stopped_live_session,
        partition,
    })
}

fn timestamp_rfc3339(ms: u64) -> String {
    format_rfc3339_ms(ms).unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}
