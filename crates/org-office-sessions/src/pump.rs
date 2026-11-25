//! Supervised session I/O pump: drives sans-I/O `org-agent-runtime::Session` over child
//! pipes, drains stdout cleanly after exit, handles escalations, background wakes, stall
//! reaping, and durable `active-turn.v1.json` updates (`ADR-0025`, `ADR-0030`, `ADR-0032`).

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    ApprovalId, ApprovalRequest, Control, Decision, Escalation, ExitInfo, Frame, LineFramer,
    PermissionPolicy, Rule, RuleScope, RuleTool, SessionEvent, ToolName,
};
use org_office::organization_family::records::PermissionPreset;
use org_proto::sessions::{PendingApprovalView, SessionState, SessionView};
use org_supervise::StopMode;

use crate::binding::RunBindingRegistry;
use crate::credentials::{format_rfc3339_ms, now_ms, redact_secrets, rfc3339_now};
use crate::instructions::{compose_turn_objective, TurnTrigger, SAME_USER_ISOLATION_NOTICE};
use crate::preset::{decide_escalation_with_tools, EscalationDisposition};
use crate::recovery::{SessionStore, StallWatchdog};
use crate::subagents::{BackgroundKind, BackgroundRegistry};

const POST_EXIT_DRAIN_GRACE: Duration = Duration::from_millis(500);

pub(crate) enum PumpCommand {
    SendTurn(String),
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
    pub reason: &'static str,
    pub rate_limit_resets_at: Option<String>,
}
pub(crate) type FailoverHook = Arc<dyn Fn(FailoverRequest) + Send + Sync>;
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
    pub suspended_on_asks: Vec<String>,
    pub waiting_for_member_id: Option<String>,
    pub waiting_for_name: Option<String>,
    pub suspended_since: Option<String>,
    pub pending_wake_count: u32,
    pub pending_wake_reason: Option<String>,
}

impl SessionRuntimeState {
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
        }
    }
}

pub(crate) struct PumpContext {
    pub employee_id: String,
    pub run_id: String,
    pub connection_id: Option<String>,
    pub active_permit: Arc<Mutex<Option<(String, u64)>>>,
    pub workspace: Option<String>,
    pub task_ref: Option<String>,
    pub tried_connections: Vec<String>,
    pub oauth_refresh_attempted: bool,
    pub store: SessionStore,
    pub bindings: RunBindingRegistry,
    pub state: Arc<Mutex<SessionRuntimeState>>,
    pub stop_plan: org_agent_runtime::StopPlan,
    pub cleanup_paths: Vec<org_agent_runtime::CleanupPath>,
    pub binding_file: std::path::PathBuf,
    pub hooks: PumpHooks,
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
    failover_pending: Option<FailoverRequest>,
}

impl PumpLoopState {
    fn new() -> Self {
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
            failover_pending: None,
        }
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
    let mut loop_state = PumpLoopState::new();

    apply_outputs(
        session.start(Instant::now()),
        &mut stdin,
        &ctx,
        &mut loop_state,
        &mut session,
    );

    loop {
        drain_commands(&commands, &mut stdin, &ctx, &mut loop_state, &mut session);
        enqueue_background_wakes(&ctx, &mut loop_state);
        dispatch_next_turn(&mut stdin, &ctx, &mut loop_state, &mut session);

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
        if !matches!(
            guard.state,
            SessionState::StallLocked | SessionState::RecoveryRequired
        ) {
            guard.state = SessionState::Stopping;
        }
    }

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

fn release_active_permit(ctx: &PumpContext) {
    let taken = ctx
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let (Some((permit_id, _)), Some(cb)) = (taken, ctx.hooks.on_permit_release.as_ref()) {
        cb(&permit_id);
    }
}

fn mark_active_permit_release_pending(ctx: &PumpContext) {
    let taken = ctx
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let (Some((permit_id, _)), Some(cb)) = (taken, ctx.hooks.on_permit_release_pending.as_ref())
    {
        cb(&permit_id);
    }
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
            if let Some(hook) = &ctx.hooks.on_turn_ended {
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

fn drain_commands(
    commands: &Receiver<PumpCommand>,
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    while let Ok(cmd) = commands.try_recv() {
        match cmd {
            PumpCommand::SendTurn(text) => loop_state.pending_turns.push(text),
            PumpCommand::AnswerApproval {
                approval_id,
                decision,
                allow_always_tool,
            } => {
                apply_approval_resolution(&ctx.state, &approval_id, allow_always_tool.as_deref());
                let ctrl = Control::Answer {
                    request_id: ApprovalId::new(approval_id),
                    decision,
                };
                if let Ok(outs) = session.control(ctrl, Instant::now()) {
                    apply_outputs(outs, stdin, ctx, loop_state, session);
                }
            }
            PumpCommand::ArmSuspend { deadline } => {
                loop_state.suspend_deadline = Some(match loop_state.suspend_deadline {
                    Some(existing) => existing.min(deadline),
                    None => deadline,
                });
            }
            PumpCommand::Stop { reason } => {
                loop_state.stop_requested = true;
                loop_state.failover_pending = None;
                if let Ok(outs) = session.control(Control::Close, Instant::now()) {
                    apply_outputs(outs, stdin, ctx, loop_state, session);
                }
                record_stop_note(ctx, &reason);
            }
        }
    }
}

fn enqueue_background_wakes(ctx: &PumpContext, loop_state: &mut PumpLoopState) {
    let wakes = {
        let mut guard = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        guard.backgrounds.drain_wakes()
    };
    for wake in wakes {
        let text = compose_turn_objective(&TurnTrigger::BackgroundWake {
            task_id: wake.task_id,
            label: wake.label,
            summary: wake.summary,
        });
        loop_state.pending_turns.push(text);
    }
}

fn ensure_permit_for_turn(ctx: &PumpContext, loop_state: &mut PumpLoopState) -> bool {
    let Some(conn_id) = ctx.connection_id.as_deref() else {
        return true;
    };
    let already_held = ctx
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_some();
    if already_held {
        loop_state
            .permit_lease_deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(60));
        return true;
    }
    let Some(acquire) = ctx.hooks.on_permit_acquire.as_ref() else {
        return true;
    };
    match acquire(conn_id, ctx.task_ref.as_deref()) {
        Ok(permit) => {
            *ctx.active_permit.lock().unwrap_or_else(|p| p.into_inner()) = Some(permit);
            loop_state.permit_lease_deadline = Some(Instant::now() + Duration::from_secs(60));
            true
        }
        Err(reason) => {
            let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
            st.last_error = Some(reason);
            if st.state == SessionState::Running {
                st.state = SessionState::Idle;
            }
            false
        }
    }
}

fn dispatch_next_turn(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    if !loop_state.ready
        || loop_state.turn_active
        || loop_state.stop_requested
        || loop_state.pending_turns.is_empty()
    {
        return;
    }
    if !ensure_permit_for_turn(ctx, loop_state) {
        loop_state.pending_turns.clear();
        return;
    }
    let text = loop_state.pending_turns.remove(0);
    let objective_for_disk = text.clone();
    loop_state.current_objective = text.clone();
    loop_state.had_external_effect = false;
    loop_state.accepted_commands_baseline = ctx.bindings.accepted_commands(&ctx.employee_id).len();
    loop_state.rate_limited = false;
    loop_state.rate_limit_resets_at = None;
    loop_state.last_permit_renew = Instant::now();
    if let Ok(outs) = session.control(
        Control::Send {
            text,
            urgent: false,
        },
        Instant::now(),
    ) {
        loop_state.turn_active = true;
        persist_turn_update(ctx, |rec| {
            rec.objective = objective_for_disk;
            rec.active = true;
        });
        apply_outputs(outs, stdin, ctx, loop_state, session);
    } else {
        release_active_permit(ctx);
        loop_state.permit_lease_deadline = None;
    }
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

const LEASE_RENEW_INTERVAL: Duration = Duration::from_secs(10);
const LEASE_SAFETY_MARGIN: Duration = Duration::from_secs(20);
const LEASE_WINDOW: Duration = Duration::from_secs(60);

fn check_permit_heartbeat(ctx: &PumpContext, loop_state: &mut PumpLoopState, now: Instant) {
    if !loop_state.turn_active {
        return;
    }
    if loop_state.last_permit_renew.elapsed() >= LEASE_RENEW_INTERVAL {
        loop_state.last_permit_renew = now;
        let current = ctx
            .active_permit
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let (Some((pid, generation)), Some(cb)) = (current, ctx.hooks.on_permit_renew.as_ref()) {
            if cb(&pid, generation) {
                loop_state.permit_lease_deadline = Some(now + LEASE_WINDOW);
            } else {
                let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
                st.last_error = Some("turn permit lease renewal failed; stopping turn".into());
                loop_state.stop_requested = true;
                return;
            }
        }
    }
    if loop_state
        .permit_lease_deadline
        .is_some_and(|d| d.saturating_duration_since(now) <= LEASE_SAFETY_MARGIN)
    {
        let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        st.last_error = Some("turn permit lease approaching expiry; stopping turn".into());
        loop_state.stop_requested = true;
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

fn apply_outputs(
    outputs: Vec<org_agent_runtime::Output>,
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    let mut follow_up = Vec::new();
    for output in outputs {
        match output {
            org_agent_runtime::Output::Write(line) => {
                let _ = stdin.write_all(line.as_bytes());
                let _ = stdin.flush();
            }
            org_agent_runtime::Output::CloseStdin => {
                let _ = stdin.close();
            }
            org_agent_runtime::Output::Event(mut event) => {
                let tokens = ctx.bindings.active_tokens();
                let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
                // Evaluate approval requests against the ORIGINAL unredacted structured input
                // (`TASK-94`) so secret redaction can never mask shell metacharacters or denied
                // paths before policy checks; redact the event copy used for history and display.
                let unredacted_approval = match &event {
                    SessionEvent::ApprovalRequested(req) => Some(req.clone()),
                    _ => None,
                };
                event.redact(&mut |s| {
                    *s = redact_secrets(s, &refs);
                });
                if let Ok(json_val) = serde_json::to_value(&event) {
                    let _ = ctx.store.append_history(&ctx.employee_id, &json_val, &refs);
                }
                let ctrl = if let Some(raw_req) = unredacted_approval {
                    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.watchdog.record_activity(Instant::now());
                    handle_approval_request(&mut state, &raw_req, &refs)
                } else {
                    handle_event(&event, ctx, loop_state)
                };
                if let Some(ctrl) = ctrl {
                    follow_up.push(ctrl);
                }
            }
        }
    }
    for ctrl in follow_up {
        if let Ok(outs) = session.control(ctrl, Instant::now()) {
            apply_outputs(outs, stdin, ctx, loop_state, session);
        }
    }
}

fn handle_event(
    event: &SessionEvent,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
) -> Option<Control> {
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    state.watchdog.record_activity(Instant::now());
    match event {
        SessionEvent::Ready { .. } => {
            loop_state.ready = true;
            state.state = SessionState::Running;
        }
        SessionEvent::SessionStarted { runtime_session_id } => {
            let id = runtime_session_id.as_str().to_owned();
            state.runtime_session_id = Some(id.clone());
            drop(state);
            persist_turn_update(ctx, |rec| rec.runtime_session_id = Some(id));
        }
        SessionEvent::TurnStarted { .. } => {
            loop_state.turn_active = true;
            state.state = SessionState::Running;
            drop(state);
            persist_turn_update(ctx, |rec| rec.active = true);
        }
        SessionEvent::AssistantText { text, .. } => state.last_reply = Some(text.clone()),
        SessionEvent::TurnEnded {
            outcome, errors, ..
        } => {
            handle_turn_ended(&mut state, loop_state, ctx, outcome, errors);
        }
        SessionEvent::StartFailed { reason } => {
            state.last_error = Some(format!("start failed: {reason:?}"));
            state.state = SessionState::Stopped;
        }
        SessionEvent::Usage(usage) => {
            state.backgrounds.record_usage(usage);
            let run_id = ctx.run_id.clone();
            let emp_id = ctx.employee_id.clone();
            let hook = ctx.hooks.on_usage.clone();
            drop(state);
            if let Some(cb) = hook {
                cb(&emp_id, &run_id, usage);
            }
        }
        SessionEvent::RateLimit(rl) => {
            if rl.status == org_agent_runtime::LimitStatus::Rejected {
                loop_state.rate_limited = true;
                if rl.resets_at.is_some() {
                    loop_state.rate_limit_resets_at = rl.resets_at.clone();
                }
                let conn_id = ctx.connection_id.clone();
                let hook = ctx.hooks.on_rate_limit.clone();
                drop(state);
                if let (Some(cid), Some(cb)) = (conn_id, hook) {
                    cb(&cid, rl.resets_at.as_deref());
                }
            }
        }
        SessionEvent::ToolStarted { .. } => {
            loop_state.had_external_effect = true;
            return handle_tool_or_approval_event(&mut state, event);
        }
        other => return handle_tool_or_approval_event(&mut state, other),
    }
    None
}

fn handle_tool_or_approval_event(
    state: &mut SessionRuntimeState,
    event: &SessionEvent,
) -> Option<Control> {
    match event {
        SessionEvent::ToolStarted {
            tool_use_id,
            name,
            capability: Some(org_agent_runtime::ToolCapability::Subagent),
            ..
        } => {
            state.backgrounds.register(
                tool_use_id.clone(),
                BackgroundKind::Subagent,
                name.clone(),
                None,
                rfc3339_now(),
            );
        }
        SessionEvent::ToolFinished {
            tool_use_id,
            is_error,
        } => {
            let _ = state.backgrounds.complete(
                tool_use_id,
                !*is_error,
                "sub-agent finished",
                rfc3339_now(),
            );
        }
        SessionEvent::Usage(usage) => state.backgrounds.record_usage(usage),
        SessionEvent::ApprovalRequested(req) => return handle_approval_request(state, req, &[]),
        SessionEvent::ApprovalCancelled { id } => {
            state
                .pending_approvals
                .retain(|p| p.approval_id != id.as_str());
        }
        _ => {}
    }
    None
}

fn handle_turn_ended(
    state: &mut SessionRuntimeState,
    loop_state: &mut PumpLoopState,
    ctx: &PumpContext,
    outcome: &org_agent_runtime::TurnOutcome,
    errors: &[String],
) {
    loop_state.turn_active = false;
    loop_state.turn_just_ended = true;
    state.watchdog.record_turn_completed(Instant::now());
    if !errors.is_empty() {
        state.last_error = Some(errors.join("; "));
    }
    let failover_blocked_recovery = evaluate_turn_failover(state, loop_state, ctx, outcome, errors);
    if state.state == SessionState::Running {
        state.state = SessionState::Idle;
    }
    persist_turn_update(ctx, |rec| {
        rec.active = false;
        rec.consecutive_stalls = 0;
        if failover_blocked_recovery {
            rec.recovery_required = true;
        }
        let mem_cmds: Vec<String> = ctx
            .bindings
            .accepted_commands(&ctx.employee_id)
            .into_iter()
            .map(|c| c.summary)
            .collect();
        if rec.accepted_commands.len() < mem_cmds.len() {
            rec.accepted_commands = mem_cmds;
        }
    });
}

pub(crate) fn is_claude_auth_expired_reply(reply: &str) -> bool {
    let lower = reply.trim().to_ascii_lowercase();
    (lower.starts_with("failed to authenticate")
        && (lower.contains("401")
            || lower.contains("oauth")
            || lower.contains("authentication_error")))
        || (lower.starts_with("api error: 401")
            && (lower.contains("oauth") || lower.contains("authentication_error")))
        || lower.starts_with("401 oauth access token has expired")
        || lower == "oauth access token has expired"
}

fn evaluate_turn_failover(
    state: &mut SessionRuntimeState,
    loop_state: &mut PumpLoopState,
    ctx: &PumpContext,
    outcome: &org_agent_runtime::TurnOutcome,
    errors: &[String],
) -> bool {
    let turn_cmds = ctx.bindings.accepted_commands(&ctx.employee_id).len();
    let has_effects =
        loop_state.had_external_effect || turn_cmds > loop_state.accepted_commands_baseline;
    let reply_auth_expired = !has_effects
        && state
            .last_reply
            .as_deref()
            .is_some_and(is_claude_auth_expired_reply);
    if matches!(outcome, org_agent_runtime::TurnOutcome::Completed) && !reply_auth_expired {
        return false;
    }
    let reason = if reply_auth_expired {
        "auth_expired"
    } else if let Some(r) = crate::router::classify_failover_error(errors, loop_state.rate_limited)
    {
        r
    } else {
        return false;
    };
    let Some(conn_id) = ctx.connection_id.as_ref() else {
        return false;
    };
    if has_effects {
        state.state = SessionState::RecoveryRequired;
        state.recovery_required = true;
        state.last_error = Some(format!("failover blocked after side effects ({reason})"));
        loop_state.stop_requested = true;
        return true;
    }
    let mut tried = ctx.tried_connections.clone();
    if !tried.contains(conn_id) {
        tried.push(conn_id.clone());
    }
    loop_state.failover_pending = Some(FailoverRequest {
        employee_id: ctx.employee_id.clone(),
        failed_run_id: ctx.run_id.clone(),
        failed_connection_id: conn_id.clone(),
        objective: loop_state.current_objective.clone(),
        workspace: ctx.workspace.clone(),
        task_ref: ctx.task_ref.clone(),
        tried_connections: tried,
        oauth_refresh_attempted: ctx.oauth_refresh_attempted,
        reason,
        rate_limit_resets_at: loop_state.rate_limit_resets_at.clone(),
    });
    loop_state.stop_requested = true;
    false
}

fn persist_turn_update(
    ctx: &PumpContext,
    mutate: impl FnOnce(&mut crate::recovery::ActiveTurnRecord),
) {
    let _ = ctx.store.update_active_turn(&ctx.employee_id, |persisted| {
        mutate(persisted);
        persisted.updated_at = rfc3339_now();
    });
}

fn handle_approval_request(
    state: &mut SessionRuntimeState,
    req: &ApprovalRequest,
    active_tokens: &[&str],
) -> Option<Control> {
    match decide_escalation_with_tools(
        req,
        &state.policy,
        state.preset,
        state.allowed_tools.as_deref(),
    ) {
        EscalationDisposition::AllowByPolicy => Some(Control::Answer {
            request_id: req.id.clone(),
            decision: Decision::AllowOnce,
        }),
        EscalationDisposition::AutoDeny { reason } => Some(Control::Answer {
            request_id: req.id.clone(),
            decision: Decision::Deny {
                message: Some(redact_secrets(&reason, active_tokens)),
                stop_turn: false,
            },
        }),
        EscalationDisposition::EscalateToOwner { warning } => {
            state.state = SessionState::WaitingApproval;
            state.pending_approvals.push(PendingApprovalView {
                approval_id: req.id.as_str().to_owned(),
                tool: redact_secrets(&req.tool, active_tokens),
                escalation: escalation_label(req.effective_escalation()).to_owned(),
                reason: Some(redact_secrets(&warning, active_tokens)),
                requested_at: format_rfc3339_ms(now_ms()),
            });
            None
        }
    }
}

fn escalation_label(e: Escalation) -> &'static str {
    match e {
        Escalation::None => "none",
        Escalation::SandboxBypass => "sandbox_bypass",
        Escalation::PermissionChange => "permission_change",
        _ => "unknown",
    }
}

fn apply_approval_resolution(
    state: &Arc<Mutex<SessionRuntimeState>>,
    approval_id: &str,
    allow_always_tool: Option<&str>,
) {
    let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
    let matched_escalation = guard
        .pending_approvals
        .iter()
        .find(|p| p.approval_id == approval_id)
        .map(|p| p.escalation.clone());
    guard
        .pending_approvals
        .retain(|p| p.approval_id != approval_id);
    if let Some(tool_name) = allow_always_tool {
        if matched_escalation.as_deref() == Some("none") {
            if let Ok(tool) = RuleTool::try_from(tool_name) {
                guard.policy.allow.push(Rule {
                    tool,
                    scope: RuleScope::Any,
                });
                guard.policy.version = guard.policy.version.saturating_add(1);
            }
        }
    }
    if guard.pending_approvals.is_empty() && guard.state == SessionState::WaitingApproval {
        guard.state = SessionState::Running;
    }
}

fn check_and_handle_stall(ctx: &PumpContext, now: Instant) -> bool {
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    if !state.watchdog.is_stalled(now) {
        return false;
    }
    let locked = state.watchdog.record_stall(now);
    let _ = state.backgrounds.reap_all(rfc3339_now());
    state.state = if locked {
        SessionState::StallLocked
    } else {
        SessionState::Stopped
    };
    state.last_error = Some("turn stalled and was reaped by watchdog".to_owned());
    let stalls = state.watchdog.consecutive_stalls();
    drop(state);
    persist_turn_update(ctx, |rec| {
        rec.active = false;
        rec.consecutive_stalls = stalls;
        rec.stall_locked = locked;
    });
    true
}

fn record_stop_note(ctx: &PumpContext, reason: &str) {
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    state.state = SessionState::Stopping;
    let _ = state.backgrounds.reap_all(rfc3339_now());
    let note = serde_json::json!({
        "type": "office_stop",
        "reason": reason,
        "at": rfc3339_now(),
    });
    let _ = ctx.store.append_history(&ctx.employee_id, &note, &[]);
}

pub(crate) fn finalize_child(
    mut child: org_supervise::OwnedChild,
    stdin: org_supervise::ChildStdin,
    session: &mut Box<dyn org_agent_runtime::Session>,
    ctx: &PumpContext,
    mode: StopMode,
    suspended: bool,
) -> bool {
    let accepted = ctx
        .bindings
        .accepted_commands(&ctx.employee_id)
        .into_iter()
        .map(|c| c.summary)
        .collect::<Vec<_>>();
    ctx.bindings.revoke(&ctx.employee_id, Some(&ctx.run_id));
    let _ = std::fs::remove_file(&ctx.binding_file);
    if let Ok(policy) = org_supervise::runtime::stop_policy(&ctx.stop_plan) {
        let _ = child.stop(&stdin, &policy, mode);
    } else {
        let _ = stdin.close();
    }
    let sockets = org_supervise::runtime::socket_cleanup(&ctx.cleanup_paths).unwrap_or_default();
    let (cleanup_verified, exit_info) = match child.cleanup_and_reap(&sockets) {
        Ok(report) => (
            true,
            ExitInfo {
                code: report.status.code(),
                signal: report.status.signal(),
            },
        ),
        Err(_) => (
            false,
            ExitInfo {
                code: None,
                signal: None,
            },
        ),
    };
    let _ = session.on_exit(exit_info, Instant::now());
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    let _ = state.backgrounds.reap_all(rfc3339_now());
    if !cleanup_verified {
        state.state = SessionState::RecoveryRequired;
        state.recovery_required = true;
        state.last_error = Some("child process cleanup could not be verified".into());
    } else if suspended {
        state.state = SessionState::Suspended;
    } else if !matches!(
        state.state,
        SessionState::StallLocked | SessionState::RecoveryRequired
    ) {
        state.state = SessionState::Stopped;
    }
    drop(state);
    persist_turn_update(ctx, |rec| {
        rec.active = false;
        if !cleanup_verified {
            rec.recovery_required = true;
        }
        if rec.accepted_commands.len() < accepted.len() {
            rec.accepted_commands = accepted;
        }
    });
    cleanup_verified
}
