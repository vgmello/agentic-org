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
    PermissionPolicy, Rule, RuleScope, RuleTool, SessionEvent,
};
use org_office::organization_family::records::PermissionPreset;
use org_proto::sessions::{PendingApprovalView, SessionState, SessionView};
use org_supervise::StopMode;

use crate::binding::RunBindingRegistry;
use crate::credentials::{format_rfc3339_ms, now_ms, redact_secrets, rfc3339_now};
use crate::instructions::{compose_turn_objective, TurnTrigger, SAME_USER_ISOLATION_NOTICE};
use crate::preset::{decide_escalation, EscalationDisposition};
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
    Stop {
        reason: String,
    },
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
        }
    }
}

pub(crate) struct PumpContext {
    pub employee_id: String,
    pub run_id: String,
    pub store: SessionStore,
    pub bindings: RunBindingRegistry,
    pub state: Arc<Mutex<SessionRuntimeState>>,
    pub stop_plan: org_agent_runtime::StopPlan,
    pub cleanup_paths: Vec<org_agent_runtime::CleanupPath>,
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
    stalled: bool,
    exit_observed_at: Option<Instant>,
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
            stalled: false,
            exit_observed_at: None,
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

        if loop_state.turn_active && check_and_handle_stall(&ctx, Instant::now()) {
            loop_state.stalled = true;
            break;
        }
        if loop_state.stop_requested {
            break;
        }
        if should_finish_after_exit(&mut child, &mut loop_state) {
            break;
        }
    }

    let mode = if loop_state.stalled {
        StopMode::Fast
    } else {
        StopMode::Graceful
    };
    finalize_child(child, stdin, &mut session, &ctx, mode);
    let _ = out_reader.join();
    let _ = err_reader.join();
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
            PumpCommand::Stop { reason } => {
                loop_state.stop_requested = true;
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
    let text = loop_state.pending_turns.remove(0);
    let objective_for_disk = text.clone();
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

fn drive_deadlines(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    let now = Instant::now();
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
                event.redact(&mut |s| {
                    *s = redact_secrets(s, &refs);
                });
                if let Ok(json_val) = serde_json::to_value(&event) {
                    let _ = ctx.store.append_history(&ctx.employee_id, &json_val, &refs);
                }
                if let Some(ctrl) = handle_event(&event, ctx, loop_state) {
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
        SessionEvent::TurnEnded { errors, .. } => {
            handle_turn_ended(&mut state, loop_state, ctx, errors);
        }
        SessionEvent::StartFailed { reason } => {
            state.last_error = Some(format!("start failed: {reason:?}"));
            state.state = SessionState::Stopped;
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
            tool_use_id, name, ..
        } if name == "Task" || name == "Agent" => {
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
        SessionEvent::ApprovalRequested(req) => return handle_approval_request(state, req),
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
    errors: &[String],
) {
    loop_state.turn_active = false;
    state.watchdog.record_turn_completed(Instant::now());
    if !errors.is_empty() {
        state.last_error = Some(errors.join("; "));
    }
    if state.state == SessionState::Running {
        state.state = SessionState::Idle;
    }
    persist_turn_update(ctx, |rec| {
        rec.active = false;
        rec.consecutive_stalls = 0;
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
) -> Option<Control> {
    match decide_escalation(req, &state.policy, state.preset) {
        EscalationDisposition::AllowByPolicy => Some(Control::Answer {
            request_id: req.id.clone(),
            decision: Decision::AllowOnce,
        }),
        EscalationDisposition::AutoDeny { reason } => Some(Control::Answer {
            request_id: req.id.clone(),
            decision: Decision::Deny {
                message: Some(reason),
                stop_turn: false,
            },
        }),
        EscalationDisposition::EscalateToOwner { warning } => {
            state.state = SessionState::WaitingApproval;
            state.pending_approvals.push(PendingApprovalView {
                approval_id: req.id.as_str().to_owned(),
                tool: req.tool.clone(),
                escalation: escalation_label(req.escalation).to_owned(),
                reason: Some(warning),
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
    guard
        .pending_approvals
        .retain(|p| p.approval_id != approval_id);
    if let Some(tool_name) = allow_always_tool {
        if let Ok(tool) = RuleTool::try_from(tool_name) {
            guard.policy.allow.push(Rule {
                tool,
                scope: RuleScope::Any,
            });
            guard.policy.version = guard.policy.version.saturating_add(1);
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
) {
    let accepted = ctx
        .bindings
        .accepted_commands(&ctx.employee_id)
        .into_iter()
        .map(|c| c.summary)
        .collect::<Vec<_>>();
    ctx.bindings.revoke(&ctx.employee_id, Some(&ctx.run_id));
    if let Ok(policy) = org_supervise::runtime::stop_policy(&ctx.stop_plan) {
        let _ = child.stop(&stdin, &policy, mode);
    } else {
        let _ = stdin.close();
    }
    let sockets = org_supervise::runtime::socket_cleanup(&ctx.cleanup_paths).unwrap_or_default();
    let exit_info = match child.cleanup_and_reap(&sockets) {
        Ok(report) => ExitInfo {
            code: report.status.code(),
            signal: report.status.signal(),
        },
        Err(_) => ExitInfo {
            code: None,
            signal: None,
        },
    };
    let _ = session.on_exit(exit_info, Instant::now());
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    let _ = state.backgrounds.reap_all(rfc3339_now());
    if !matches!(
        state.state,
        SessionState::StallLocked | SessionState::RecoveryRequired
    ) {
        state.state = SessionState::Stopped;
    }
    drop(state);
    persist_turn_update(ctx, |rec| {
        rec.active = false;
        if rec.accepted_commands.len() < accepted.len() {
            rec.accepted_commands = accepted;
        }
    });
}
