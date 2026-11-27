//! Supervised session I/O pump: drives sans-I/O `org-agent-runtime::Session` over child
//! pipes, drains stdout cleanly after exit, handles escalations, background wakes, stall
//! reaping, and durable `active-turn.v1.json` updates (`ADR-0025`, `ADR-0030`, `ADR-0032`).

use std::fs::File;
use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_agent_runtime::{Decision, Frame, LineFramer, PermissionPolicy, ToolName};
use org_office::organization_family::records::PermissionPreset;
use org_proto::sessions::{PendingApprovalView, SessionState, SessionView};
use org_supervise::StopMode;

use crate::binding::RunBindingRegistry;
use crate::credentials::rfc3339_now;
use crate::instructions::SAME_USER_ISOLATION_NOTICE;
use crate::recovery::{SessionStore, StallWatchdog};
use crate::subagents::BackgroundRegistry;

mod activity;
mod approval;
mod commands;
mod controls;
mod events;
#[cfg(test)]
mod tests;
mod turn;

use activity::settle_activity_idle;
use approval::check_and_handle_stall;
pub(crate) use approval::finalize_child;
use commands::{
    drain_commands, enqueue_background_wakes, mark_active_permit_release_pending,
    release_active_permit,
};
use controls::drive_turn_controls;
pub(crate) use controls::SteerRequest;
use events::apply_outputs;
#[cfg(test)]
pub(crate) use events::classify_turn_failover_reason;
use turn::{check_permit_heartbeat, dispatch_next_turn, maybe_deliver_mid_turn};

const POST_EXIT_DRAIN_GRACE: Duration = Duration::from_millis(500);

pub(crate) enum PumpCommand {
    SendTurn(String),
    /// A person's explicit interrupt of the running turn: the process lives on (`TASK-103`).
    InterruptTurn,
    /// A person's mid-turn steer (`TASK-103`), counted as a queued turn ([`queue_steer`]).
    SteerTurn(SteerRequest),
    NotifyPendingWakes,
    AnswerApproval {
        approval_id: String,
        decision: Decision,
        allow_always_tool: Option<String>,
    },
    ArmSuspend {
        deadline: Instant,
    },
    Stop {
        reason: String,
    },
}

/// One coalesced, fenced mid-turn delivery staged from `pending-wakes.v1.json` without yet
/// mutating disk (`TASK-115` / `doc-101` §4B).
#[derive(Clone, Debug)]
pub(crate) struct StagedMidTurnBatch {
    pub delivery_id: String,
    pub item_ids: Vec<String>,
    pub text: String,
    pub has_agent_item: bool,
    pub agent_sender_convs: Vec<(String, String)>,
}

/// Result of inspecting `pending-wakes.v1.json` for mid-turn delivery while a turn is active.
pub(crate) enum MidTurnStageOutcome {
    None,
    Wait(Duration),
    Ready(StagedMidTurnBatch),
}

/// Inspects eligible mid-turn wakes without removing them from disk (`TASK-115`).
pub(crate) type StageMidTurnWakesHook =
    Arc<dyn Fn(&str, Option<&str>, u32, u32) -> MidTurnStageOutcome + Send + Sync>;
/// Commits removal of only the runtime-acknowledged `item_ids` (preserving `seen_item_ids`)
/// after `SessionEvent::MidTurnDelivered` (`TASK-115` / `doc-101` §4B).
pub(crate) type MidTurnDeliveredHook = Arc<dyn Fn(&str, &StagedMidTurnBatch) + Send + Sync>;

pub(crate) type TurnEndedHook = Arc<dyn Fn(&str) + Send + Sync>;
pub(crate) type OpenRequiredAsksHook = Arc<dyn Fn(&str) -> bool + Send + Sync>;
/// Records normalized runtime usage from a supervised session turn into the office accounting ledger (`L2`/`L3`).
pub(crate) type UsageHook = Arc<dyn Fn(&str, &str, &org_agent_runtime::Usage) + Send + Sync>;
/// Acquires a turn permit synchronously before dispatching a follow-up turn inside a live pump
/// (`L4` / `doc-88` §6: eliminates permit-less follow-up & background-wake turns).
pub(crate) type PermitAcquireHook =
    Arc<dyn Fn(&str, Option<&str>) -> Result<(String, u64), String> + Send + Sync>;
/// Renews an active turn permit in memory (`TASK-2` / `L4`, zero durable writes).
pub(crate) type PermitRenewHook = Arc<dyn Fn(&str, u64) -> bool + Send + Sync>;
/// Releases a turn permit after verified process/turn cleanup (`ASK-4`, `LIVE-2`).
pub(crate) type PermitReleaseHook = Arc<dyn Fn(&str) + Send + Sync>;
/// Marks a turn permit `release_pending` when child cleanup cannot be verified (`AE-07`).
pub(crate) type PermitReleasePendingHook = Arc<dyn Fn(&str) + Send + Sync>;
/// Records a rate-limit cooldown on a provider connection (`L4` / `FC-099`).
pub(crate) type RateLimitHook = Arc<dyn Fn(&str, Option<&str>) + Send + Sync>;
/// Request to relaunch a failed turn on the next eligible connection in the frozen pool (`doc-88` §7).
#[derive(Clone, Debug)]
pub(crate) struct FailoverRequest {
    pub employee_id: String,
    pub failed_run_id: String,
    pub failed_connection_id: String,
    pub objective: String,
    pub workspace: Option<String>,
    pub task_ref: Option<String>,
    pub tried_connections: Vec<String>,
    pub oauth_refresh_attempted: bool,
    pub oauth_token_fp: Option<String>,
    pub reason: &'static str,
    pub rate_limit_resets_at: Option<String>,
    /// The failed run's [`PumpContext::stop_epoch`]: a later explicit stop abandons the failover.
    pub stop_epoch: u64,
    /// The Chief of Staff partition the failed run served (`TASK-80.1`): its relaunch serves the
    /// same one and so acts for exactly the same caller ([`Self::acting_for_caller_id`]).
    pub partition: Option<crate::partition::PartitionKey>,
}

impl FailoverRequest {
    /// The member the failed run (and so its relaunch) acts for; `None` for nobody.
    pub(crate) fn acting_for_caller_id(&self) -> Option<&str> {
        self.partition
            .as_ref()
            .and_then(crate::partition::PartitionKey::caller_id)
    }
}
pub(crate) type FailoverHook = Arc<dyn Fn(FailoverRequest) + Send + Sync>;
/// Returns `true` when this Linux `bwrap` session's masked sibling set is stale because a new
/// sibling employee appeared after launch (`TASK-166`).
pub(crate) type BwrapTopologyStaleHook = Arc<dyn Fn() -> bool + Send + Sync>;
/// Returns `true` when durable suspension was recorded (non-empty required asks), `false` if
/// all required asks were answered/withdrawn before suspension took effect (`High #3`).
pub(crate) type SuspendedHook =
    Arc<dyn Fn(&str, Vec<String>, Option<String>, Option<String>) -> bool + Send + Sync>;

pub(crate) struct PumpHooks {
    pub has_open_required_asks: Option<OpenRequiredAsksHook>,
    pub on_turn_ended: Option<TurnEndedHook>,
    pub on_suspended: Option<SuspendedHook>,
    pub on_usage: Option<UsageHook>,
    pub on_permit_acquire: Option<PermitAcquireHook>,
    pub on_permit_renew: Option<PermitRenewHook>,
    pub on_permit_release: Option<PermitReleaseHook>,
    pub on_permit_release_pending: Option<PermitReleasePendingHook>,
    pub on_rate_limit: Option<RateLimitHook>,
    pub on_failover: Option<FailoverHook>,
    pub stage_mid_turn_wakes: Option<StageMidTurnWakesHook>,
    pub on_mid_turn_delivered: Option<MidTurnDeliveredHook>,
    pub is_bwrap_topology_stale: Option<BwrapTopologyStaleHook>,
}

pub(crate) struct SessionRuntimeState {
    pub employee_id: String,
    pub office_id: String,
    pub state: SessionState,
    pub run_id: Option<String>,
    pub session_id: Option<String>,
    pub runtime_id: Option<String>,
    pub runtime_session_id: Option<String>,
    pub private_home: String,
    pub permission_preset: String,
    pub sandbox_enabled: bool,
    pub spec_hash: String,
    pub watchdog: StallWatchdog,
    pub recovery_required: bool,
    pub last_reply: Option<String>,
    pub last_error: Option<String>,
    pub backgrounds: BackgroundRegistry,
    pub pending_approvals: Vec<PendingApprovalView>,
    pub policy: PermissionPolicy,
    pub preset: PermissionPreset,
    pub allowed_tools: Option<Vec<ToolName>>,
    pub writable_roots: Vec<org_agent_runtime::SpecPath>,
    pub suspended_on_asks: Vec<String>,
    pub waiting_for_member_id: Option<String>,
    pub waiting_for_name: Option<String>,
    pub suspended_since: Option<String>,
    pub pending_wake_count: u32,
    pub pending_wake_reason: Option<String>,
    /// Turns queued on the pump (sent on its channel or waiting in its queue) that have not yet
    /// been dispatched, refused or carried into a failover. A session with a queued turn is
    /// never idle: only [`queue_turn`] raises it, and the pump lowers it (`TASK-80.1`).
    pub queued_turns: usize,
    /// Coarse, ephemeral activity of the running turn (`TASK-103`).
    pub activity: crate::stream_hub::SessionActivity,
}

impl SessionRuntimeState {
    /// Forgets `n` turns that left the pump's queue: dispatched, refused or carried into a
    /// failover objective.
    pub(crate) fn dequeue_turns(&mut self, n: usize) {
        self.queued_turns = self.queued_turns.saturating_sub(n);
    }

    /// Moves a busy session back to idle once no queued turn is left behind it, so a turn that
    /// ends (or never starts) ahead of a queued one never shows the session idle in between.
    pub(crate) fn settle_idle(&mut self) {
        if self.state == SessionState::Running && self.queued_turns == 0 {
            self.state = SessionState::Idle;
        }
    }

    pub(crate) fn to_view(&self) -> SessionView {
        SessionView {
            employee_id: self.employee_id.clone(),
            office_id: self.office_id.clone(),
            state: self.state,
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            runtime_id: self.runtime_id.clone(),
            runtime_session_id: self.runtime_session_id.clone(),
            private_home: self.private_home.clone(),
            same_user_isolation_notice: SAME_USER_ISOLATION_NOTICE.to_owned(),
            permission_preset: self.permission_preset.clone(),
            sandbox_enabled: self.sandbox_enabled,
            consecutive_stalls: self.watchdog.consecutive_stalls(),
            stall_locked: self.watchdog.is_locked(),
            recovery_required: self.recovery_required,
            last_reply: self.last_reply.clone(),
            last_error: self.last_error.clone(),
            background_tasks: self.backgrounds.views(),
            pending_approvals: self.pending_approvals.clone(),
            usage: self.backgrounds.usage(),
            suspended_on_asks: self.suspended_on_asks.clone(),
            waiting_for_member_id: self.waiting_for_member_id.clone(),
            waiting_for_name: self.waiting_for_name.clone(),
            suspended_since: self.suspended_since.clone(),
            pending_wake_count: self.pending_wake_count,
            pending_wake_reason: self.pending_wake_reason.clone(),
            activity_phase: self.activity.phase,
            active_tool_name: self.activity.active_tool_name.clone(),
            active_tool_started_at: self.activity.active_tool_started_at.clone(),
            supports_steer: self.activity.supports_steer,
        }
    }
}

/// Queues `text` on a live pump. In one critical section of the session's state lock the turn
/// is counted in [`SessionRuntimeState::queued_turns`], an idle session is marked busy and the
/// turn is sent, so the pump can never refuse, finish or fail the turn before it is counted and
/// leave the session wedged busy. A send to a dead pump undoes both. `Sender::send` on the
/// pump's unbounded channel never blocks (rules §3.4). Returns whether the pump received it.
pub(crate) fn queue_turn(
    state: &Mutex<SessionRuntimeState>,
    tx: &Sender<PumpCommand>,
    text: String,
) -> bool {
    queue_counted(state, tx, PumpCommand::SendTurn(text))
}

/// Queues a person's steer on a live pump with exactly [`queue_turn`]'s accounting: until the
/// steer is delivered into the running turn, it is a queued turn (`TASK-103`).
pub(crate) fn queue_steer(
    state: &Mutex<SessionRuntimeState>,
    tx: &Sender<PumpCommand>,
    request: SteerRequest,
) -> bool {
    queue_counted(state, tx, PumpCommand::SteerTurn(request))
}

fn queue_counted(
    state: &Mutex<SessionRuntimeState>,
    tx: &Sender<PumpCommand>,
    command: PumpCommand,
) -> bool {
    let mut st = state.lock().unwrap_or_else(|p| p.into_inner());
    st.queued_turns = st.queued_turns.saturating_add(1);
    let flipped = st.state == SessionState::Idle;
    if flipped {
        st.state = SessionState::Running;
    }
    if tx.send(command).is_ok() {
        return true;
    }
    st.dequeue_turns(1);
    if flipped {
        st.settle_idle();
    }
    false
}

pub(crate) struct PumpContext {
    pub employee_id: String,
    pub run_id: String,
    /// The session's runtime adapter: classifies runtime-specific turn replies (`TASK-171`).
    pub runtime: Arc<dyn org_agent_runtime::AgentRuntime>,
    pub supports_mid_turn: bool,
    pub connection_id: Option<String>,
    pub active_permit: Arc<Mutex<Option<(String, u64)>>>,
    pub workspace: Option<String>,
    pub task_ref: Option<String>,
    pub tried_connections: Vec<String>,
    pub oauth_refresh_attempted: bool,
    pub oauth_expires_at_ms: Option<u64>,
    pub oauth_token_fp: Option<String>,
    pub store: SessionStore,
    pub bindings: RunBindingRegistry,
    pub state: Arc<Mutex<SessionRuntimeState>>,
    pub stop_plan: org_agent_runtime::StopPlan,
    pub cleanup_paths: Vec<org_agent_runtime::CleanupPath>,
    pub binding_file: std::path::PathBuf,
    pub hooks: PumpHooks,
    /// The employee's explicit-stop epoch when this run was installed
    /// ([`crate::manager::SessionRegistry::stop_epoch`]), carried into every [`FailoverRequest`].
    pub stop_epoch: u64,
    /// The Chief of Staff partition this run serves (`TASK-80.1`): the run records its
    /// conversation and context use there, and carries it into every [`FailoverRequest`].
    pub partition: Option<crate::partition::PartitionHandle>,
    /// Where the run publishes its coarse live activity (`TASK-103`); `None` publishes nothing.
    pub stream: Option<crate::stream_hub::StreamPublisher>,
}

impl PumpContext {
    /// The partition this run serves, for a [`FailoverRequest`].
    pub(crate) fn partition_key(&self) -> Option<crate::partition::PartitionKey> {
        self.partition.as_ref().map(|p| p.key().clone())
    }
}

enum ReaderMsg {
    Stdout(Vec<u8>),
    StdoutClosed,
    Stderr(Vec<u8>),
}

struct PumpLoopState {
    out_framer: LineFramer,
    err_framer: LineFramer,
    pending_turns: Vec<String>,
    ready: bool,
    turn_active: bool,
    stop_requested: bool,
    suspend_deadline: Option<Instant>,
    suspend_now: bool,
    turn_just_ended: bool,
    stalled: bool,
    exit_observed_at: Option<Instant>,
    had_external_effect: bool,
    accepted_commands_baseline: usize,
    rate_limited: bool,
    rate_limit_resets_at: Option<String>,
    last_permit_renew: Instant,
    permit_lease_deadline: Option<Instant>,
    current_objective: String,
    turn_reply: Option<String>,
    turns_dispatched: u64,
    oauth_refresh_attempted: bool,
    failover_pending: Option<FailoverRequest>,
    mid_turn_dirty: bool,
    mid_turn_next_check: Option<Instant>,
    in_flight_delivery: Option<StagedMidTurnBatch>,
    mid_turn_total_this_turn: u32,
    mid_turn_agent_this_turn: u32,
    /// `pending_turns[0]` is a person's steer: a later steer folds into it (`TASK-103`).
    front_is_steer: bool,
    /// A person asked to interrupt the running turn; [`controls`] sends it once it can.
    interrupt_wanted: bool,
    /// The interrupt reached the runtime: the turn it ends never fails over.
    interrupt_sent: bool,
    /// A native steer the runtime was not ready for, retried every pass (one queued turn).
    pending_steer: Option<SteerRequest>,
    /// Steers delivered natively and not yet confirmed: `(delivery_id, turn text)`.
    in_flight_steers: Vec<(String, String)>,
}

impl PumpLoopState {
    fn new(oauth_refresh_attempted: bool) -> Self {
        Self {
            out_framer: LineFramer::new(),
            err_framer: LineFramer::new(),
            pending_turns: Vec::new(),
            ready: false,
            turn_active: false,
            stop_requested: false,
            suspend_deadline: None,
            suspend_now: false,
            turn_just_ended: false,
            stalled: false,
            exit_observed_at: None,
            had_external_effect: false,
            accepted_commands_baseline: 0,
            rate_limited: false,
            rate_limit_resets_at: None,
            last_permit_renew: Instant::now(),
            permit_lease_deadline: None,
            current_objective: String::new(),
            turn_reply: None,
            turns_dispatched: 0,
            oauth_refresh_attempted,
            failover_pending: None,
            mid_turn_dirty: false,
            mid_turn_next_check: None,
            in_flight_delivery: None,
            mid_turn_total_this_turn: 0,
            mid_turn_agent_this_turn: 0,
            front_is_steer: false,
            interrupt_wanted: false,
            interrupt_sent: false,
            pending_steer: None,
            in_flight_steers: Vec::new(),
        }
    }

    /// Whether any turn waits to run (a steer the runtime was not ready for included).
    fn has_queued_turns(&self) -> bool {
        !self.pending_turns.is_empty() || self.pending_steer.is_some()
    }

    /// Takes every turn waiting to run, steers first: each is one of
    /// [`SessionRuntimeState::queued_turns`], which the caller lowers by the count taken.
    fn take_queued_turns(&mut self) -> Vec<String> {
        let mut taken: Vec<String> = self
            .pending_steer
            .take()
            .map(|s| s.turn)
            .into_iter()
            .collect();
        taken.append(&mut self.pending_turns);
        self.front_is_steer = false;
        taken
    }
}

pub(crate) fn run_pump(
    spawned: org_supervise::Spawned,
    mut session: Box<dyn org_agent_runtime::Session>,
    commands: Receiver<PumpCommand>,
    ctx: PumpContext,
) {
    let org_supervise::Spawned {
        mut child,
        mut stdin,
        stdout,
        stderr,
    } = spawned;
    let (io_tx, io_rx) = mpsc::channel::<ReaderMsg>();
    let out_reader = spawn_pipe_reader(stdout, io_tx.clone(), false);
    let err_reader = spawn_pipe_reader(stderr, io_tx, true);
    let mut loop_state = PumpLoopState::new(ctx.oauth_refresh_attempted);

    apply_outputs(
        session.start(Instant::now()),
        &mut stdin,
        &ctx,
        &mut loop_state,
        &mut session,
    );

    loop {
        drain_commands(&commands, &mut stdin, &ctx, &mut loop_state, &mut session);
        drive_turn_controls(&mut stdin, &ctx, &mut loop_state, &mut session);
        enqueue_background_wakes(&ctx, &mut loop_state);
        dispatch_next_turn(&mut stdin, &ctx, &mut loop_state, &mut session);
        maybe_deliver_mid_turn(&mut stdin, &ctx, &mut loop_state, &mut session);
        if loop_state.stop_requested {
            break;
        }

        if poll_io(&io_rx, &mut stdin, &ctx, &mut loop_state, &mut session) {
            break;
        }
        drive_deadlines(&mut stdin, &ctx, &mut loop_state, &mut session);
        if should_exit_loop(&mut child, &ctx, &mut loop_state) {
            break;
        }
    }

    // Drop the command receiver and mark state Stopping BEFORE child teardown so concurrent
    // callers (`reserve_start_or_send`) can never push turns into a dead pump (`High #3`).
    drop(commands);
    {
        let mut guard = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        // Turns still queued die with the pump (a preserved failover carries its own objective).
        guard.queued_turns = 0;
        if !matches!(
            guard.state,
            SessionState::StallLocked | SessionState::RecoveryRequired
        ) {
            guard.state = SessionState::Stopping;
        }
    }
    // Whatever the turn was doing is over with the run.
    settle_activity_idle(&ctx);

    finish_pump(
        child,
        stdin,
        &mut session,
        &ctx,
        &loop_state,
        out_reader,
        err_reader,
    );
}

fn should_exit_loop(
    child: &mut org_supervise::OwnedChild,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
) -> bool {
    let has_open_asks = ctx
        .hooks
        .has_open_required_asks
        .as_ref()
        .is_some_and(|check| check(&ctx.employee_id));
    if !has_open_asks {
        // Ask was answered or withdrawn; disarm immediately so a later ask never inherits a
        // stale deadline (ADR-0020 d10).
        loop_state.suspend_deadline = None;
    } else if loop_state
        .suspend_deadline
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        loop_state.suspend_now = true;
    }
    if loop_state.turn_active && check_and_handle_stall(ctx, Instant::now()) {
        loop_state.stalled = true;
        return true;
    }
    if loop_state.turn_just_ended {
        loop_state.turn_just_ended = false;
        if has_open_asks {
            loop_state.suspend_now = true;
        } else if !loop_state.stop_requested && !should_finish_after_exit(child, loop_state) {
            // Always release the completed turn's permit; any follow-up turn in `pending_turns`
            // acquires/verifies its own permit right before `Control::Send` (`doc-88` §6).
            release_active_permit(ctx);
            loop_state.permit_lease_deadline = None;
            // A turn queued behind this one keeps the session busy: the hook fires once the
            // last queued turn ends.
            let nothing_queued = loop_state.pending_turns.is_empty()
                && ctx
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .queued_turns
                    == 0;
            if let Some(hook) = ctx.hooks.on_turn_ended.as_ref().filter(|_| nothing_queued) {
                hook(&ctx.employee_id);
            }
        }
    }
    loop_state.suspend_now
        || loop_state.stop_requested
        || should_finish_after_exit(child, loop_state)
}

fn finish_pump(
    child: org_supervise::OwnedChild,
    stdin: org_supervise::ChildStdin,
    session: &mut Box<dyn org_agent_runtime::Session>,
    ctx: &PumpContext,
    loop_state: &PumpLoopState,
    out_reader: JoinHandle<()>,
    err_reader: JoinHandle<()>,
) {
    let (reaped_bg, session_id_snap, rsid_snap) = if loop_state.suspend_now {
        let guard = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        (
            guard.backgrounds.running_labels(),
            guard.session_id.clone(),
            guard.runtime_session_id.clone(),
        )
    } else {
        (Vec::new(), None, None)
    };
    let mode = if loop_state.stalled {
        StopMode::Fast
    } else {
        StopMode::Graceful
    };
    let cleanup_verified = finalize_child(child, stdin, session, ctx, mode, false);
    let _ = out_reader.join();
    let _ = err_reader.join();
    if loop_state.suspend_now && cleanup_verified {
        let persisted_suspended = ctx
            .hooks
            .on_suspended
            .as_ref()
            .is_some_and(|hook| hook(&ctx.employee_id, reaped_bg, session_id_snap, rsid_snap));
        if persisted_suspended {
            let mut guard = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
            if !matches!(
                guard.state,
                SessionState::StallLocked | SessionState::RecoveryRequired
            ) {
                guard.state = SessionState::Suspended;
            }
        }
    }
    if cleanup_verified {
        // Release permit AFTER verified process cleanup & suspension persistence (`ASK-4`, `LIVE-2`).
        release_active_permit(ctx);
        if let (Some(req), Some(cb)) = (
            loop_state.failover_pending.as_ref(),
            ctx.hooks.on_failover.as_ref(),
        ) {
            cb(req.clone());
        }
    } else {
        // Unproven cleanup (`AE-07`): keep slot occupied in `release_pending` & require recovery.
        mark_active_permit_release_pending(ctx);
    }
}

/// Applies `mutate` to this run's durable `active-turn.v1.json` record (a newer run's record is
/// never touched) and stamps `updated_at`.
fn persist_turn_update(
    ctx: &PumpContext,
    mutate: impl FnOnce(&mut crate::recovery::ActiveTurnRecord),
) {
    let _ = ctx.store.update_active_turn(&ctx.employee_id, |persisted| {
        if persisted.run_id != ctx.run_id {
            return;
        }
        mutate(persisted);
        persisted.updated_at = rfc3339_now();
    });
}

fn should_finish_after_exit(
    child: &mut org_supervise::OwnedChild,
    loop_state: &mut PumpLoopState,
) -> bool {
    if loop_state.exit_observed_at.is_none() && matches!(child.try_exit(), Ok(Some(_))) {
        loop_state.exit_observed_at = Some(Instant::now());
    }
    loop_state
        .exit_observed_at
        .is_some_and(|at| at.elapsed() >= POST_EXIT_DRAIN_GRACE)
}

fn poll_io(
    io_rx: &Receiver<ReaderMsg>,
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) -> bool {
    match io_rx.recv_timeout(Duration::from_millis(25)) {
        Ok(ReaderMsg::Stdout(chunk)) => {
            let frames = loop_state.out_framer.push(&chunk);
            for frame in frames {
                let outs = match frame {
                    Frame::Line(line) => session.on_line(&line, Instant::now()),
                    Frame::Truncated(len) => session.on_truncated(len, Instant::now()),
                };
                apply_outputs(outs, stdin, ctx, loop_state, session);
            }
            false
        }
        Ok(ReaderMsg::Stderr(chunk)) => {
            let frames = loop_state.err_framer.push(&chunk);
            for frame in frames {
                if let Frame::Line(line) = frame {
                    let outs = session.on_stderr_line(&line, Instant::now());
                    apply_outputs(outs, stdin, ctx, loop_state, session);
                }
            }
            false
        }
        Ok(ReaderMsg::StdoutClosed) => {
            if let Some(Frame::Line(line)) = loop_state.out_framer.finish() {
                let outs = session.on_line(&line, Instant::now());
                apply_outputs(outs, stdin, ctx, loop_state, session);
            }
            let outs = session.on_stdout_closed(Instant::now());
            apply_outputs(outs, stdin, ctx, loop_state, session);
            true
        }
        Err(mpsc::RecvTimeoutError::Timeout) => false,
        Err(mpsc::RecvTimeoutError::Disconnected) => true,
    }
}

fn drive_deadlines(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    let now = Instant::now();
    check_permit_heartbeat(ctx, loop_state, now);
    if session.next_deadline().is_some_and(|d| now >= d) {
        let outs = session.on_tick(now);
        apply_outputs(outs, stdin, ctx, loop_state, session);
    }
}

fn spawn_pipe_reader(mut file: File, tx: Sender<ReaderMsg>, is_stderr: bool) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match file.read(&mut buf) {
                Ok(0) => {
                    if !is_stderr {
                        let _ = tx.send(ReaderMsg::StdoutClosed);
                    }
                    break;
                }
                Ok(n) => {
                    let msg = if is_stderr {
                        ReaderMsg::Stderr(buf[..n].to_vec())
                    } else {
                        ReaderMsg::Stdout(buf[..n].to_vec())
                    };
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    })
}
