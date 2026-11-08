//! The port from an adapter's side: a trivial runtime and session written against the public
//! API only (so `#[non_exhaustive]` applies as it will to real adapters), driven the way the
//! pump will drive them.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use org_runtime::testkit::{self, Script};
use org_runtime::{
    encode_line, Activity, AgentIdentity, AgentName, AgentRuntime, ApprovalId, ApprovalKind,
    ApprovalRequest, Capabilities, CommandPrefix, Control, ControlError, CostBasis, Decision,
    EnvName, ExitInfo, FileLocation, Frame, LaunchContext, LaunchPlan, LineFramer, Output,
    PathPattern, PermissionMode, PermissionPolicy, PlanError, PlannedDir, PlannedFile, Probe,
    ProbeContext, ProbeOutput, ProbePlan, ProbeResult, RelPath, Rule, RuleScope, RuleTool,
    RuntimeId, SandboxSpec, Session, SessionEvent, SessionId, SessionKind, SessionSpec,
    SignalTarget, SpecPath, StartFailure, Stdio, StopPlan, StopSignal, StopStep, TurnOutcome,
    Unavailable, UsageScope, VersionRange,
};
use serde_json::{json, Value};

const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// A made-up JSON-lines protocol: `hello` opens, `ready` answers, `say` is a turn,
/// `approve?` asks, `done` ends a turn.
///
/// `bug` plants one contract violation, to prove the testkit catches it.
struct EchoSession {
    state: State,
    opened_at: Option<Instant>,
    pending: Vec<ApprovalId>,
    bug: Option<Bug>,
    queued: Option<String>,
    eof_at: Option<Instant>,
    nudge: Option<Instant>,
    armed: bool,
}

/// Contract violations the testkit must catch.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Bug {
    /// Rule 1: a normal Send during a turn is accepted.
    NoBusy,
    /// Rule 2: a refused Send is still queued, and sent when the turn ends.
    QueuesRefusedSend,
    /// Rule 2: a refused Answer ends the turn.
    RefusedAnswerEndsTurn,
    /// Rule 3: Ended from on_stdout_closed.
    EndedOnStdoutClose,
    /// Rule 3: Ended from the opening timeout.
    EndedOnTimeout,
    /// Rule 3: Ended from a tick while idle.
    EndedOnIdleTick,
    /// Rule 3: Ended again on a second on_exit.
    EndedTwice,
    /// Rule 3: Ended always reports exit code 0.
    FixedExit,
    /// Rule 3: Ended from a tick some time after stdout closed.
    EndedAfterStdoutGrace,
    /// Rule 3: Ended isn't on_exit's last output.
    EndedNotLast,
    /// Rule 3: Ended from a stderr line.
    EndedOnStderr,
    /// Rule 3: output after Ended.
    TalksAfterEnded,
    /// Rule 3: a control after Ended returns something other than Closed.
    ControlAfterEndedNotClosed,
    /// Rule 3: a deadline after Ended (and ticks that then talk).
    DeadlineAfterEnded,
    /// Rule 3: a deadline after Ended, while ticks stay silent.
    SilentDeadlineAfterEnded,
    /// Rule 3: Ended early (on stdout close) and again from on_exit.
    EndedEarlyAndAgain,
    /// Rule 2: a refused control moves the deadline.
    RefusedControlMovesDeadline,
    /// Rule 2: a refused control arms a deadline that only shows after the turn ends.
    RefusedControlArmsLaterDeadline,
}

#[derive(PartialEq)]
enum State {
    Opening,
    Idle,
    Busy,
    Closed,
    Ended,
}

impl EchoSession {
    fn new() -> Self {
        EchoSession {
            state: State::Opening,
            opened_at: None,
            pending: Vec::new(),
            bug: None,
            queued: None,
            eof_at: None,
            nudge: None,
            armed: false,
        }
    }

    fn with_bug(bug: Bug) -> Self {
        EchoSession {
            bug: Some(bug),
            ..Self::new()
        }
    }

    fn ended(&mut self, exit: ExitInfo) -> Vec<Output> {
        self.state = State::Ended;
        vec![Output::Event(SessionEvent::Ended { exit })]
    }

    fn write(value: Value) -> Output {
        Output::Write(encode_line(&value).unwrap())
    }
}

impl Session for EchoSession {
    fn start(&mut self, now: Instant) -> Vec<Output> {
        self.opened_at = Some(now);
        vec![Self::write(json!({"type": "hello"}))]
    }

    fn on_line(&mut self, line: &[u8], now: Instant) -> Vec<Output> {
        if self.state == State::Ended {
            if self.bug == Some(Bug::TalksAfterEnded) {
                return vec![Output::Event(SessionEvent::Unknown {
                    type_name: "late".into(),
                })];
            }
            return vec![];
        }
        let msg: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(e) => {
                return vec![Output::Event(SessionEvent::ProtocolError {
                    detail: e.to_string(),
                })]
            }
        };
        let event = match msg["type"].as_str().unwrap_or_default() {
            "ready" => {
                // Only the opening moves to Idle: a stray line never reopens a closed session.
                if self.state == State::Opening {
                    self.state = State::Idle;
                }
                SessionEvent::Ready {
                    runtime_version: Some("1.0".into()),
                    pid: None,
                    agents: vec!["echo".into()],
                    permission_mode: Some("ask".into()),
                }
            }
            "say" => SessionEvent::AssistantText {
                message_id: None,
                text: msg["text"].as_str().unwrap_or_default().to_owned(),
            },
            "approve?" => {
                let id = ApprovalId::new(msg["id"].as_str().unwrap_or_default());
                self.pending.push(id.clone());
                SessionEvent::ApprovalRequested(ApprovalRequest {
                    id,
                    kind: ApprovalKind::Command,
                    tool: "Bash".into(),
                    display_name: None,
                    input: msg["input"].clone(),
                    justification_untrusted: None,
                    blocked_path: None,
                    tool_use_id: None,
                })
            }
            "done" => {
                if self.state == State::Busy {
                    self.state = State::Idle;
                }
                if std::mem::take(&mut self.armed) {
                    // Far beyond every tick the check makes, so only the deadline shows it.
                    self.nudge = Some(now + Duration::from_secs(365 * 24 * 3600));
                }
                let ended = SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Completed,
                    errors: vec![],
                };
                if let Some(text) = self.queued.take() {
                    self.state = State::Busy;
                    return vec![
                        Output::Event(ended),
                        Self::write(json!({"type": "user", "text": text})),
                    ];
                }
                ended
            }
            other => SessionEvent::Unknown {
                type_name: other.to_owned(),
            },
        };
        vec![Output::Event(event)]
    }

    fn on_truncated(&mut self, len: usize, _now: Instant) -> Vec<Output> {
        if self.state == State::Ended {
            return vec![];
        }
        vec![Output::Event(SessionEvent::Truncated { len })]
    }

    fn on_stderr_line(&mut self, _line: &[u8], _now: Instant) -> Vec<Output> {
        if self.bug == Some(Bug::EndedOnStderr) && self.state != State::Ended {
            return self.ended(ExitInfo {
                code: None,
                signal: None,
            });
        }
        // This protocol says nothing useful on stderr.
        vec![]
    }

    fn on_stdout_closed(&mut self, now: Instant) -> Vec<Output> {
        if self.bug == Some(Bug::EndedAfterStdoutGrace) {
            self.eof_at = Some(now);
        }
        if self.bug == Some(Bug::EndedEarlyAndAgain) {
            // Says Ended without being ended, so on_exit says it again.
            return vec![Output::Event(SessionEvent::Ended {
                exit: ExitInfo {
                    code: None,
                    signal: None,
                },
            })];
        }
        if self.bug == Some(Bug::EndedOnStdoutClose) && self.state != State::Ended {
            return self.ended(ExitInfo {
                code: None,
                signal: None,
            });
        }
        // Nothing to report: the process may still be running. Ended comes from on_exit.
        vec![]
    }

    fn on_exit(&mut self, exit: ExitInfo, _now: Instant) -> Vec<Output> {
        if self.state == State::Ended && self.bug != Some(Bug::EndedTwice) {
            return vec![];
        }
        if self.bug == Some(Bug::FixedExit) {
            return self.ended(ExitInfo {
                code: Some(0),
                signal: None,
            });
        }
        let mut out = self.ended(exit);
        if self.bug == Some(Bug::EndedNotLast) {
            out.push(Output::Event(SessionEvent::Unknown {
                type_name: "after".into(),
            }));
        }
        out
    }

    fn on_tick(&mut self, now: Instant) -> Vec<Output> {
        if self.state == State::Ended && self.bug != Some(Bug::DeadlineAfterEnded) {
            return vec![];
        }
        if let Some(eof) = self.eof_at {
            if self.state != State::Ended && now >= eof + OPEN_TIMEOUT {
                return self.ended(ExitInfo {
                    code: None,
                    signal: None,
                });
            }
        }
        if self.bug == Some(Bug::EndedOnIdleTick) && self.state == State::Idle {
            return self.ended(ExitInfo {
                code: None,
                signal: None,
            });
        }
        match self.next_deadline() {
            Some(deadline) if now >= deadline => {
                let mut out = vec![
                    Output::Event(SessionEvent::StartFailed {
                        reason: StartFailure::ProtocolTimeout,
                    }),
                    Output::CloseStdin,
                ];
                self.state = State::Closed;
                if self.bug == Some(Bug::EndedOnTimeout) {
                    out.extend(self.ended(ExitInfo {
                        code: None,
                        signal: None,
                    }));
                }
                out
            }
            _ => vec![],
        }
    }

    fn control(&mut self, control: Control, now: Instant) -> Result<Vec<Output>, ControlError> {
        match self.state {
            State::Ended if self.bug == Some(Bug::ControlAfterEndedNotClosed) => {
                return Err(ControlError::NotReady)
            }
            State::Closed | State::Ended => return Err(ControlError::Closed),
            State::Opening => return Err(ControlError::NotReady),
            State::Idle | State::Busy => {}
        }
        match control {
            Control::Send { urgent: true, .. } if self.state == State::Busy => {
                Err(ControlError::NeedsInterrupt)
            }
            Control::Send { .. } if self.state == State::Busy && self.bug == Some(Bug::NoBusy) => {
                Ok(vec![])
            }
            Control::Send { text, .. } if self.state == State::Busy => {
                if self.bug == Some(Bug::QueuesRefusedSend) {
                    self.queued = Some(text);
                }
                if self.bug == Some(Bug::RefusedControlArmsLaterDeadline) {
                    self.armed = true;
                }
                if self.bug == Some(Bug::RefusedControlMovesDeadline) {
                    self.nudge = Some(now + Duration::from_secs(1));
                }
                Err(ControlError::Busy)
            }
            Control::Send { text, .. } => {
                self.state = State::Busy;
                Ok(vec![
                    Self::write(json!({"type": "user", "text": text})),
                    Output::Event(SessionEvent::Status(Activity::Busy)),
                ])
            }
            Control::Interrupt => Ok(vec![Self::write(json!({"type": "interrupt"}))]),
            Control::Answer {
                request_id,
                decision,
            } => {
                // A decision this adapter doesn't know: refuse, never guess (lib.rs rules).
                let allow = match decision {
                    Decision::AllowOnce => true,
                    Decision::Deny { .. } => false,
                    _ => return Err(ControlError::Unsupported),
                };
                let Some(at) = self.pending.iter().position(|p| *p == request_id) else {
                    if self.bug == Some(Bug::RefusedAnswerEndsTurn) {
                        self.state = State::Idle;
                    }
                    return Err(ControlError::UnknownApproval);
                };
                self.pending.remove(at);
                Ok(vec![Self::write(
                    json!({"type": "answer", "id": request_id.as_str(), "allow": allow}),
                )])
            }
            Control::Close => {
                self.state = State::Closed;
                Ok(vec![Output::CloseStdin])
            }
            // Any command this session doesn't know: refuse, never guess.
            _ => Err(ControlError::Unsupported),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        if self.state == State::Ended
            && matches!(
                self.bug,
                Some(Bug::DeadlineAfterEnded | Bug::SilentDeadlineAfterEnded)
            )
        {
            return self.opened_at;
        }
        if let Some(nudge) = self.nudge.filter(|_| self.state != State::Ended) {
            return Some(nudge);
        }
        if let Some(eof) = self.eof_at.filter(|_| self.state != State::Ended) {
            return Some(eof + OPEN_TIMEOUT);
        }
        match self.state {
            State::Opening => self.opened_at.map(|t| t + OPEN_TIMEOUT),
            _ => None,
        }
    }
}

struct EchoRuntime;

impl EchoRuntime {
    fn plan(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
        stdio: Stdio,
    ) -> Result<LaunchPlan, PlanError> {
        let caps = self.capabilities();
        // Accepting edits narrows to asking when a deny was dropped (deny wins over allow).
        let mode = match spec.effective_permission_mode(&caps) {
            PermissionMode::Ask => "ask",
            PermissionMode::AcceptEdits => "accept-edits",
            PermissionMode::Plan => "plan",
            _ => return Err(PlanError::Unsupported("permission mode")),
        };
        let mut args = vec![
            "--agent".into(),
            spec.agent.name.as_str().to_owned(),
            "--mode".into(),
            mode.into(),
        ];
        // Deny rules: enforced here (caps.enforces_deny_rules), so a scope this adapter
        // doesn't know means it can't enforce the spec: refuse.
        for rule in &spec.policy.deny {
            let scope = scope_text(&rule.scope).ok_or(PlanError::Unsupported("rule scope"))?;
            args.extend(["--deny".into(), format!("{}({scope})", rule.tool.as_str())]);
        }
        // Allow rules: grants, so only when deny wins nothing away, and an unknown scope is
        // simply dropped.
        if spec.keeps_allow_rules(&caps) {
            for rule in &spec.policy.allow {
                if let Some(scope) = scope_text(&rule.scope) {
                    args.extend(["--allow".into(), format!("{}({scope})", rule.tool.as_str())]);
                }
            }
        }
        // The sandbox lists this runtime enforces; deny_read isn't one (caps), so it's dropped.
        if spec.sandbox.enabled {
            for root in &spec.sandbox.writable_roots {
                args.extend(["--write".into(), root.as_str().to_owned()]);
            }
            args.extend(["--write".into(), ctx.tmpdir.as_str().to_owned()]);
            for pattern in &spec.sandbox.deny_write {
                args.extend(["--deny-write".into(), pattern.as_str().to_owned()]);
            }
        }
        Ok(LaunchPlan {
            program: ctx.runtime_path.clone(),
            args,
            cwd: spec.workspace.clone(),
            env_add: vec![(EnvName::try_from("ECHO_NO_UPDATE").unwrap(), "1".into())],
            dirs: vec![PlannedDir {
                location: FileLocation::RuntimeDir,
                rel_path: RelPath::try_from("agents").unwrap(),
            }],
            files: vec![PlannedFile {
                location: FileLocation::RuntimeDir,
                rel_path: RelPath::try_from("instructions.md").unwrap(),
                contents: spec.instructions.clone().into_bytes(),
                git_exclude: false,
            }],
            stdio,
            runtime_session_id: ctx.resume.clone().or_else(|| ctx.new_session_id.clone()),
            stop: StopPlan {
                steps: vec![
                    StopStep::CloseStdin { grace_ms: 30_000 },
                    StopStep::Signal {
                        signal: StopSignal::Terminate,
                        target: SignalTarget::Leader,
                        grace_ms: 5_000,
                    },
                ],
            },
            cleanup_after_exit: vec![],
            secret_env_names: vec![],
        })
    }
}

/// How this fake writes a rule's scope, or `None` for a scope it doesn't know.
fn scope_text(scope: &RuleScope) -> Option<String> {
    match scope {
        RuleScope::Any => Some("*".into()),
        RuleScope::Path(pattern) => Some(pattern.as_str().into()),
        RuleScope::CommandPrefix(prefix) => Some(format!("{}*", prefix.as_str())),
        RuleScope::McpServer(server) => Some(format!("mcp:{}", server.as_str())),
        _ => None,
    }
}

impl AgentRuntime for EchoRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("echo").unwrap()
    }

    fn binary_name(&self) -> &'static str {
        "echo-agent"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            steer: false,
            interrupt_in_band: true,
            idle_interrupt_ends_turn: false,
            approval_bridge: true,
            deny_message_reaches_model: true,
            runtime_cancels_approvals: false,
            unsolicited_turns: false,
            session_id_known_up_front: true,
            resume_ignores_cwd: false,
            resume_applies_instructions: true,
            reads_workspace_instruction_files: false,
            tools_detach_from_group: false,
            take_over: true,
            enforces_writable_roots: true,
            enforces_deny_write: true,
            enforces_deny_read: false,
            enforces_unix_socket_allowlist: true,
            enforces_tool_allowlist: false,
            enforces_deny_rules: true,
            usage_scope: UsageScope::TurnDelta,
            cost: CostBasis::None,
            tested_versions: VersionRange {
                min: "1.0".into(),
                max: "1.9".into(),
            },
        }
    }

    fn plan_session(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
    ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
        match spec.kind {
            SessionKind::Home {} | SessionKind::Task { .. } | SessionKind::OneOff { .. } => {}
            // A kind this adapter doesn't know: refuse the spec (lib.rs rules).
            _ => return Err(PlanError::Unsupported("session kind")),
        }
        if ctx.resume.is_none() && ctx.new_session_id.is_none() {
            return Err(PlanError::InvalidSpec(
                "a new session needs new_session_id".into(),
            ));
        }
        Ok((
            self.plan(spec, ctx, Stdio::Piped)?,
            Box::new(EchoSession::new()),
        ))
    }

    fn plan_take_over(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
        cols: u16,
        rows: u16,
    ) -> Result<LaunchPlan, PlanError> {
        if matches!(spec.kind, SessionKind::OneOff { .. }) {
            return Err(PlanError::InvalidSpec(
                "a one-off can't be taken over".into(),
            ));
        }
        self.plan(spec, ctx, Stdio::Pty { cols, rows })
    }

    fn plan_probe(&self, probe: Probe, ctx: &ProbeContext) -> Option<ProbePlan> {
        match probe {
            Probe::Version => Some(ProbePlan {
                program: ctx.runtime_path.clone(),
                args: vec!["--version".into()],
                env_add: vec![],
                stdin_lines: vec![],
                keep_stdin_open_until_exit: false,
                timeout_ms: 10_000,
            }),
            Probe::Auth => Some(ProbePlan {
                program: ctx.runtime_path.clone(),
                args: vec!["auth".into(), "status".into()],
                env_add: vec![],
                stdin_lines: vec![],
                keep_stdin_open_until_exit: false,
                timeout_ms: 10_000,
            }),
            // This runtime documents its quota probe as optional (quota comes in-stream),
            // which is the only reason to return None for a probe it knows.
            Probe::Quota => None,
            // A probe this adapter doesn't know: no plan. The office treats None for a probe it
            // requires as the runtime being unusable, so this fails closed.
            _ => None,
        }
    }

    fn parse_probe(&self, probe: Probe, out: &ProbeOutput) -> ProbeResult {
        match probe {
            Probe::Version if out.exit_code == Some(0) && !out.timed_out => ProbeResult::Version {
                raw: String::from_utf8_lossy(&out.stdout).trim().to_owned(),
                in_tested_range: None,
            },
            Probe::Version => ProbeResult::Unavailable {
                reason: Unavailable::NotInstalled,
            },
            _ => ProbeResult::Unavailable {
                reason: Unavailable::Other {
                    detail: "not probed".into(),
                },
            },
        }
    }
}

fn spec() -> SessionSpec {
    SessionSpec {
        runtime: RuntimeId::try_from("echo").unwrap(),
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: AgentName::try_from("echo").unwrap(),
            description: "Echoes".into(),
            prompt: "Echo.".into(),
        },
        model: None,
        effort: None,
        workspace: SpecPath::try_from("/homes/echo").unwrap(),
        read_only_dirs: vec![],
        instructions: "Be brief.".into(),
        permission_mode: PermissionMode::Ask,
        policy: PermissionPolicy {
            version: 1,
            allow: vec![],
            deny: vec![],
        },
        sandbox: SandboxSpec {
            enabled: true,
            writable_roots: vec![SpecPath::try_from("/homes/echo").unwrap()],
            deny_write: vec![],
            deny_read: vec![],
            allowed_unix_sockets: vec![],
        },
        mcp_servers: BTreeMap::new(),
        tools: None,
    }
}

fn ctx() -> LaunchContext {
    LaunchContext {
        runtime_path: PathBuf::from("/opt/echo/bin/echo-agent"),
        runtime_version: Some("1.0".into()),
        runtime_dir: SpecPath::try_from("/office/agents/echo/runtime").unwrap(),
        tmpdir: SpecPath::try_from("/private/tmp/agentic-org/s1").unwrap(),
        office_env_names: vec![
            EnvName::try_from("ORG_SESSION_TOKEN").unwrap(),
            EnvName::try_from("ORG_AGENT").unwrap(),
        ],
        new_session_id: Some(SessionId::try_from("00000000-0000-0000-0000-000000000001").unwrap()),
        resume: None,
    }
}

/// What a pump does with stdout bytes: frame them and feed each frame in.
fn feed(
    session: &mut dyn Session,
    framer: &mut LineFramer,
    bytes: &[u8],
    now: Instant,
) -> Vec<Output> {
    let mut out = Vec::new();
    for frame in framer.push(bytes) {
        out.extend(match frame {
            Frame::Line(line) => session.on_line(&line, now),
            // Exhaustive: no wildcard, so a new frame kind breaks this build.
            Frame::Truncated(len) => session.on_truncated(len, now),
        });
    }
    out
}

fn events(outputs: Vec<Output>) -> Vec<SessionEvent> {
    outputs
        .into_iter()
        .filter_map(|o| match o {
            Output::Event(e) => Some(e),
            Output::Write(_) | Output::CloseStdin => None,
        })
        .collect()
}

#[test]
fn a_session_runs_a_turn_through_the_port() {
    let runtime: Box<dyn AgentRuntime> = Box::new(EchoRuntime);
    let (plan, mut session) = runtime.plan_session(&spec(), &ctx()).unwrap();
    assert_eq!(plan.program, PathBuf::from("/opt/echo/bin/echo-agent"));
    assert_eq!(plan.stdio, Stdio::Piped);
    assert_eq!(
        plan.runtime_session_id.as_ref().map(SessionId::as_str),
        Some("00000000-0000-0000-0000-000000000001")
    );

    let t0 = Instant::now();
    let mut framer = LineFramer::new();

    // Opening: one line out, a deadline set, controls refused until ready.
    let opening = session.start(t0);
    let [Output::Write(hello)] = &opening[..] else {
        panic!("expected one write, got {opening:?}")
    };
    assert_eq!(hello.as_bytes(), b"{\"type\":\"hello\"}\n");
    assert_eq!(session.next_deadline(), Some(t0 + OPEN_TIMEOUT));
    assert_eq!(
        session.control(
            Control::Send {
                text: "hi".into(),
                urgent: false
            },
            t0
        ),
        Err(ControlError::NotReady)
    );

    // The runtime answers, split across reads.
    assert!(feed(session.as_mut(), &mut framer, b"{\"type\":\"rea", t0).is_empty());
    let out = feed(session.as_mut(), &mut framer, b"dy\"}\n", t0);
    assert!(matches!(&events(out)[..], [SessionEvent::Ready { .. }]));
    assert_eq!(session.next_deadline(), None);

    // A send goes out as a line and the session turns busy.
    let out = session
        .control(
            Control::Send {
                text: "line\u{2028}two".into(),
                urgent: false,
            },
            t0,
        )
        .unwrap();
    let Output::Write(line) = &out[0] else {
        panic!("expected a write, got {out:?}")
    };
    let line = line.as_bytes();
    assert_eq!(line.last(), Some(&b'\n'));
    // The person's U+2028 can't split the message, and there's no raw newline inside.
    assert!(!line[..line.len() - 1].contains(&b'\n'));
    assert_eq!(out[1], Output::Event(SessionEvent::Status(Activity::Busy)));

    // While busy: normal input waits, urgent input needs an interrupt on this runtime.
    let busy = |urgent| Control::Send {
        text: "more".into(),
        urgent,
    };
    assert_eq!(session.control(busy(false), t0), Err(ControlError::Busy));
    assert_eq!(
        session.control(busy(true), t0),
        Err(ControlError::NeedsInterrupt)
    );

    // An approval: only the id the session raised can be answered, once.
    let out = feed(
        session.as_mut(),
        &mut framer,
        b"{\"type\":\"approve?\",\"id\":\"req-7\",\"input\":{\"command\":\"ls\"}}\n",
        t0,
    );
    let [SessionEvent::ApprovalRequested(request)] = &events(out)[..] else {
        panic!("expected an approval request")
    };
    assert_eq!(request.input, json!({"command": "ls"}));
    let forged = Control::Answer {
        request_id: ApprovalId::new("req-8"),
        decision: Decision::AllowOnce,
    };
    assert_eq!(
        session.control(forged, t0),
        Err(ControlError::UnknownApproval)
    );
    let answer = Control::Answer {
        request_id: request.id.clone(),
        decision: Decision::Deny {
            message: Some("not now".into()),
            stop_turn: false,
        },
    };
    let out = session.control(answer.clone(), t0).unwrap();
    let [Output::Write(line)] = &out[..] else {
        panic!("expected one write, got {out:?}")
    };
    // Compared as JSON, so serde_json's map order doesn't matter.
    assert_eq!(
        serde_json::from_slice::<Value>(line.as_bytes()).unwrap(),
        json!({"type": "answer", "id": "req-7", "allow": false})
    );
    assert_eq!(
        session.control(answer, t0),
        Err(ControlError::UnknownApproval)
    );

    // Garbage and unknown messages don't end the session.
    let out = feed(
        session.as_mut(),
        &mut framer,
        b"not json\n{\"type\":\"mystery\"}\n{\"type\":\"say\",\"text\":\"hello\"}\n{\"type\":\"done\"}\n",
        t0,
    );
    let evs = events(out);
    assert!(matches!(evs[0], SessionEvent::ProtocolError { .. }));
    assert_eq!(
        evs[1],
        SessionEvent::Unknown {
            type_name: "mystery".into()
        }
    );
    assert_eq!(
        evs[2],
        SessionEvent::AssistantText {
            message_id: None,
            text: "hello".into()
        }
    );
    assert!(matches!(
        evs[3],
        SessionEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
            ..
        }
    ));

    // Close: stdin closes, then everything is refused; stdout EOF is only recorded, and the
    // exit ends it.
    assert_eq!(
        session.control(Control::Close, t0).unwrap(),
        vec![Output::CloseStdin]
    );
    assert_eq!(
        session.control(Control::Interrupt, t0),
        Err(ControlError::Closed)
    );
    assert_eq!(framer.finish(), None);
    assert_eq!(session.on_stdout_closed(t0), vec![]);
    let exit = ExitInfo {
        code: Some(0),
        signal: None,
    };
    assert_eq!(
        session.on_exit(exit, t0),
        vec![Output::Event(SessionEvent::Ended { exit })]
    );
}

#[test]
fn protocol_timeouts_are_pure() {
    let (_, mut session) = EchoRuntime.plan_session(&spec(), &ctx()).unwrap();
    let t0 = Instant::now();
    session.start(t0);
    let deadline = session.next_deadline().unwrap();
    // Ticking early is harmless.
    assert!(session.on_tick(t0).is_empty());
    assert!(session
        .on_tick(deadline - Duration::from_millis(1))
        .is_empty());
    // At the deadline the start fails, without any real time passing.
    assert_eq!(
        session.on_tick(deadline),
        vec![
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ProtocolTimeout
            }),
            Output::CloseStdin,
        ]
    );
    assert_eq!(session.next_deadline(), None);
}

#[test]
fn oversized_lines_reach_the_session_as_truncated() {
    let (_, mut session) = EchoRuntime.plan_session(&spec(), &ctx()).unwrap();
    let t0 = Instant::now();
    session.start(t0);
    let mut framer = LineFramer::new();
    let mut big = vec![b'x'; org_runtime::MAX_LINE_BYTES + 10];
    big.push(b'\n');
    assert_eq!(
        events(feed(session.as_mut(), &mut framer, &big, t0)),
        vec![SessionEvent::Truncated {
            len: org_runtime::MAX_LINE_BYTES + 10
        }]
    );
}

#[test]
fn plans_refuse_what_they_cant_honour() {
    let runtime = EchoRuntime;
    let mut no_id = ctx();
    no_id.new_session_id = None;
    assert!(matches!(
        runtime.plan_session(&spec(), &no_id),
        Err(PlanError::InvalidSpec(_))
    ));
    let mut one_off = spec();
    one_off.kind = SessionKind::OneOff {
        run_id: org_runtime::PlainId::try_from("r1").unwrap(),
    };
    let err = runtime
        .plan_take_over(&one_off, &ctx(), 80, 24)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid session spec: a one-off can't be taken over"
    );
    let plan = runtime.plan_take_over(&spec(), &ctx(), 120, 40).unwrap();
    assert_eq!(
        plan.stdio,
        Stdio::Pty {
            cols: 120,
            rows: 40
        }
    );
}

#[test]
fn probes_are_planned_and_parsed_purely() {
    let runtime = EchoRuntime;
    let probe_ctx = ProbeContext {
        runtime_path: PathBuf::from("/opt/echo/bin/echo-agent"),
        runtime_version: None,
    };
    assert!(runtime.plan_probe(Probe::Quota, &probe_ctx).is_none());
    let plan = runtime.plan_probe(Probe::Version, &probe_ctx).unwrap();
    assert_eq!(plan.args, vec!["--version".to_owned()]);
    assert!(!plan.keep_stdin_open_until_exit);
    let out = ProbeOutput {
        stdout: b"1.2.3\n".to_vec(),
        stderr: vec![],
        exit_code: Some(0),
        timed_out: false,
    };
    assert_eq!(
        runtime.parse_probe(Probe::Version, &out),
        ProbeResult::Version {
            raw: "1.2.3".into(),
            in_tested_range: None
        }
    );
}

#[test]
fn the_port_is_thread_safe_where_it_must_be() {
    fn send<T: Send + ?Sized>() {}
    fn send_sync<T: Send + Sync + ?Sized>() {}
    send::<Box<dyn Session>>();
    send_sync::<Box<dyn AgentRuntime>>();
    send::<SessionEvent>();
    send::<Output>();
}

/// Opens a fresh session and makes it ready, as the pump would.
fn ready_session(now: Instant) -> (Box<dyn Session>, LineFramer) {
    let (_, mut session) = EchoRuntime.plan_session(&spec(), &ctx()).unwrap();
    let mut framer = LineFramer::new();
    session.start(now);
    feed(
        session.as_mut(),
        &mut framer,
        b"{\"type\":\"ready\"}\n",
        now,
    );
    (session, framer)
}

fn send(text: &str, urgent: bool) -> Control {
    Control::Send {
        text: text.into(),
        urgent,
    }
}

/// The fake's fixture lines for the testkit: `ready` opens, the Send alone starts the turn,
/// `done` ends it.
fn echo_script() -> Script {
    Script {
        opening: vec![br#"{"type":"ready"}"#.to_vec()],
        turn: vec![],
        turn_end: vec![br#"{"type":"done"}"#.to_vec()],
        stderr: vec![b"echo: warning: low disk".to_vec()],
    }
}

fn make_echo() -> Box<dyn Session> {
    EchoRuntime.plan_session(&spec(), &ctx()).unwrap().1
}

// The three contract rules, through the public testkit, as every adapter will run them.
#[test]
fn rule_1_normal_send_during_a_turn_is_busy() {
    testkit::check_busy_during_turn(&mut make_echo, &echo_script(), Instant::now());
}

#[test]
fn rule_2_an_error_changes_nothing() {
    testkit::check_error_changes_nothing(&mut make_echo, &echo_script(), Instant::now());
}

#[test]
fn rule_3_ended_comes_exactly_once_and_last() {
    testkit::check_ended_once_and_last(&mut make_echo, &echo_script(), Instant::now());
}

#[test]
fn the_fake_passes_every_check() {
    testkit::check_all(&mut make_echo, &echo_script(), Instant::now());
}

/// The testkit fails every planted bug, with a broken-rule message (not a script error).
#[test]
fn the_testkit_catches_every_planted_bug() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    type Check = fn(&mut dyn FnMut() -> Box<dyn Session>, &Script, Instant);
    let rule_1: Check = testkit::check_busy_during_turn;
    let rule_2: Check = testkit::check_error_changes_nothing;
    let rule_3: Check = testkit::check_ended_once_and_last;
    let bugs = [
        (Bug::NoBusy, "rule 1", rule_1),
        (Bug::QueuesRefusedSend, "rule 2", rule_2),
        (Bug::RefusedAnswerEndsTurn, "rule 2", rule_2),
        (Bug::EndedOnStdoutClose, "rule 3", rule_3),
        (Bug::EndedOnTimeout, "rule 3", rule_3),
        (Bug::EndedOnIdleTick, "rule 3", rule_3),
        (Bug::EndedTwice, "rule 3", rule_3),
        (Bug::FixedExit, "rule 3", rule_3),
        (Bug::EndedAfterStdoutGrace, "rule 3", rule_3),
        (Bug::EndedNotLast, "rule 3", rule_3),
        (Bug::EndedOnStderr, "rule 3", rule_3),
        (Bug::TalksAfterEnded, "rule 3", rule_3),
        (Bug::ControlAfterEndedNotClosed, "rule 3", rule_3),
        (Bug::DeadlineAfterEnded, "rule 3", rule_3),
        (Bug::SilentDeadlineAfterEnded, "rule 3", rule_3),
        (Bug::EndedEarlyAndAgain, "rule 3", rule_3),
        (Bug::RefusedControlMovesDeadline, "rule 2", rule_2),
        (Bug::RefusedControlArmsLaterDeadline, "rule 2", rule_2),
    ];
    let fails = |bug: Bug, check: Check| -> Option<String> {
        let mut make = || Box::new(EchoSession::with_bug(bug)) as Box<dyn Session>;
        let result = catch_unwind(AssertUnwindSafe(|| {
            check(&mut make, &echo_script(), Instant::now())
        }));
        result.err().map(|panic| {
            panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        })
    };
    for (bug, rule, check) in bugs {
        // The rule's own check fails it, naming the rule.
        let message = fails(bug, check)
            .unwrap_or_else(|| panic!("the {rule} check passed a session with {bug:?}"));
        assert!(
            message.contains(&format!("{rule} broken")),
            "{bug:?}: expected a {rule} failure, got: {message}"
        );
        // And check_all fails it too.
        assert!(
            fails(bug, testkit::check_all).is_some(),
            "check_all passed a session with {bug:?}"
        );
    }
}

#[test]
fn an_answer_error_keeps_the_pending_approval() {
    // Rule 2 with an approval pending, which the generic script doesn't raise.
    let t0 = Instant::now();
    let (mut session, mut framer) = ready_session(t0);
    session.control(send("go", false), t0).unwrap();
    feed(
        session.as_mut(),
        &mut framer,
        b"{\"type\":\"approve?\",\"id\":\"req-1\",\"input\":{}}\n",
        t0,
    );
    let forged = Control::Answer {
        request_id: ApprovalId::new("req-2"),
        decision: Decision::AllowOnce,
    };
    assert_eq!(
        session.control(forged, t0),
        Err(ControlError::UnknownApproval)
    );
    let answer = Control::Answer {
        request_id: ApprovalId::new("req-1"),
        decision: Decision::AllowOnce,
    };
    assert_eq!(session.control(answer.clone(), t0).unwrap().len(), 1);
    assert_eq!(
        session.control(answer, t0),
        Err(ControlError::UnknownApproval)
    );
}

// Stray runtime lines never reopen a closed session or skip the opening.
#[test]
fn stray_lines_do_not_change_the_session_state() {
    let t0 = Instant::now();
    let (_, mut opening) = EchoRuntime.plan_session(&spec(), &ctx()).unwrap();
    let mut framer = LineFramer::new();
    opening.start(t0);
    feed(opening.as_mut(), &mut framer, b"{\"type\":\"done\"}\n", t0);
    assert_eq!(
        opening.control(send("early", false), t0),
        Err(ControlError::NotReady)
    );

    let (mut session, mut framer) = ready_session(t0);
    session.control(Control::Close, t0).unwrap();
    for line in [&b"{\"type\":\"done\"}\n"[..], b"{\"type\":\"ready\"}\n"] {
        feed(session.as_mut(), &mut framer, line, t0);
        assert_eq!(
            session.control(send("after close", false), t0),
            Err(ControlError::Closed)
        );
    }
}

fn with_rules() -> SessionSpec {
    let mut spec = spec();
    spec.permission_mode = PermissionMode::AcceptEdits;
    spec.policy.allow = vec![Rule {
        tool: RuleTool::try_from("Bash").unwrap(),
        scope: RuleScope::CommandPrefix(CommandPrefix::try_from("git ").unwrap()),
    }];
    spec.policy.deny = vec![Rule {
        tool: RuleTool::try_from("*").unwrap(),
        scope: RuleScope::Path(PathPattern::try_from("/office/**").unwrap()),
    }];
    spec
}

fn planned_args(spec: &SessionSpec) -> Vec<String> {
    EchoRuntime.plan_session(spec, &ctx()).unwrap().0.args
}

// Deny wins over allow, for the sandbox lists too: this fake can't enforce deny_read, so a
// non-empty one is dropped, and every allow rule with it.
#[test]
fn a_dropped_deny_list_drops_the_allow_rules() {
    let spec = with_rules();
    let args = planned_args(&spec);
    assert!(args.contains(&"--allow".to_owned()), "{args:?}");
    assert!(args.contains(&"Bash(git *)".to_owned()));
    assert!(args.contains(&"*(/office/**)".to_owned()));

    assert!(args.contains(&"accept-edits".to_owned()));

    let mut denies_reads = with_rules();
    denies_reads.sandbox.deny_read = vec![PathPattern::try_from("/office/**").unwrap()];
    let args = planned_args(&denies_reads);
    assert!(!args.contains(&"--allow".to_owned()), "{args:?}");
    // Accepting edits is an implicit allow: it narrows to asking.
    assert!(args.contains(&"ask".to_owned()), "{args:?}");
    assert!(!args.contains(&"accept-edits".to_owned()));
    // The deny rules it does enforce stay.
    assert!(args.contains(&"--deny".to_owned()));

    // Every adapter always adds its TMPDIR as a writable root.
    assert!(args.contains(&"/private/tmp/agentic-org/s1".to_owned()));
}
