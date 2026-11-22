//! Comprehensive real-process & durable-store tests for `TASK-47` (`C6/C8/C10`):
//! - `ASK-3..9`, `ASK-11..12`: required-ask immediate & grace suspend, presence, same-session
//!   resume, restart survival, background reaping notice, budget-hold preservation, sub-agent asks.
//! - `WAKE-1..7`, `LIVE-1`, `LIVE-3..8`: peer wakes, busy coalescing, hourly limit, person exempt,
//!   mutual-suspension deadlock breaker, post-restart pending wakes, idle-with-no-new-work silence.
//! - `AVAIL-1..6`: always-available presence, paused non-start, human/task auto-start.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    encode_line, AgentRuntime, Capabilities, Control, ControlError, CostBasis, ExitInfo,
    LaunchContext, LaunchPlan, Output, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult,
    ProbeRun, RuntimeId, Session, SessionEvent, SessionId, SessionSpec, SignalTarget, Stdio,
    StopPlan, StopSignal, StopStep, TurnOutcome, Unavailable, UsageScope, VersionRange,
};
use org_office::{Office, TestOffice};
use org_proto::directory::MemberPresence;
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::sessions::{MemberSessionStartParams, SessionState};
use org_proto::Request;

use crate::credentials::now_ms;
use crate::manager::{SessionManager, SessionManagerConfig};
use crate::runtime_registry::RuntimeRegistry;
use crate::wake::{PendingWakeItem, SuspendedAskEntry, WakeHoldReason, WakeKind, WakeOutcome};

#[derive(Default)]
struct RecordedLaunches {
    resumes: Mutex<Vec<Option<String>>>,
    turns: Mutex<Vec<String>>,
}

struct WakeScriptedRuntime {
    recorded: Arc<RecordedLaunches>,
}

struct WakeScriptedSession {
    recorded: Arc<RecordedLaunches>,
    runtime_session_id: String,
    ready: bool,
}

impl Session for WakeScriptedSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("ready") => {
                self.ready = true;
                vec![
                    Output::Event(SessionEvent::SessionStarted {
                        runtime_session_id: SessionId::try_from(self.runtime_session_id.as_str())
                            .unwrap(),
                    }),
                    Output::Event(SessionEvent::Ready {
                        runtime_version: Some("1.0.0".into()),
                        pid: None,
                        agents: vec!["ada".into()],
                        permission_mode: Some("default".into()),
                    }),
                ]
            }
            Some("tool_start") => {
                let id = v["id"].as_str().unwrap_or("t1").to_owned();
                vec![Output::Event(SessionEvent::ToolStarted {
                    tool_use_id: id,
                    name: "Agent".into(),
                    input: serde_json::json!({ "task": "helper" }),
                })]
            }
            Some("reply") => {
                let text = v["text"].as_str().unwrap_or("").to_owned();
                vec![
                    Output::Event(SessionEvent::AssistantText {
                        message_id: Some("m1".into()),
                        text,
                    }),
                    Output::Event(SessionEvent::TurnEnded {
                        outcome: TurnOutcome::Completed,
                        errors: Vec::new(),
                    }),
                ]
            }
            _ => Vec::new(),
        }
    }

    fn on_truncated(&mut self, _len: usize, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_stderr_line(&mut self, _line: &[u8], _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_stdout_closed(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_exit(&mut self, _exit: ExitInfo, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_tick(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn control(&mut self, cmd: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        match cmd {
            Control::Send { text, .. } => {
                self.recorded.turns.lock().unwrap().push(text.clone());
                let line = encode_line(&serde_json::json!({ "text": text }))
                    .map_err(|_| ControlError::Closed)?;
                Ok(vec![
                    Output::Event(SessionEvent::TurnStarted {
                        turn_id: Some("t1".into()),
                        solicited: true,
                    }),
                    Output::Write(line),
                ])
            }
            Control::Close => Ok(vec![Output::CloseStdin]),
            _ => Ok(Vec::new()),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}

impl AgentRuntime for WakeScriptedRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }

    fn binary_name(&self) -> &'static str {
        "wake-scripted"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            steer: false,
            interrupt_in_band: false,
            idle_interrupt_ends_turn: false,
            approval_bridge: false,
            deny_message_reaches_model: false,
            runtime_cancels_approvals: false,
            unsolicited_turns: false,
            session_id_known_up_front: false,
            resume_ignores_cwd: false,
            resume_applies_instructions: false,
            reads_workspace_instruction_files: false,
            tools_detach_from_group: false,
            take_over: false,
            enforces_writable_roots: false,
            enforces_deny_write: false,
            enforces_deny_read: false,
            enforces_unix_socket_allowlist: false,
            enforces_tool_allowlist: false,
            enforces_deny_rules: false,
            usage_scope: UsageScope::TurnDelta,
            cost: CostBasis::ListPriceEstimate,
            tested_versions: VersionRange {
                min: "1.0.0".into(),
                max: "1.0.0".into(),
            },
        }
    }

    fn plan_session(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
    ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
        let resume_str = ctx.resume.as_ref().map(|s| s.as_str().to_owned());
        self.recorded
            .resumes
            .lock()
            .unwrap()
            .push(resume_str.clone());
        let rsid = resume_str
            .or_else(|| ctx.new_session_id.as_ref().map(|s| s.as_str().to_owned()))
            .unwrap_or_else(|| "sess-fixed-001".to_owned());
        let plan = LaunchPlan {
            program: ctx.runtime_path.clone(),
            args: Vec::new(),
            cwd: spec.workspace.clone(),
            stdio: Stdio::Piped,
            files: Vec::new(),
            dirs: Vec::new(),
            env_add: Vec::new(),
            auth_env_names: Vec::new(),
            secret_env_names: Vec::new(),
            stop: StopPlan {
                steps: vec![
                    StopStep::CloseStdin { grace_ms: 200 },
                    StopStep::Signal {
                        signal: StopSignal::Terminate,
                        target: SignalTarget::Group,
                        grace_ms: 200,
                    },
                ],
            },
            cleanup_after_exit: Vec::new(),
            runtime_session_id: SessionId::try_from(rsid.as_str()).ok(),
        };
        Ok((
            plan,
            Box::new(WakeScriptedSession {
                recorded: Arc::clone(&self.recorded),
                runtime_session_id: rsid,
                ready: false,
            }),
        ))
    }

    fn plan_take_over(
        &self,
        _spec: &SessionSpec,
        _ctx: &LaunchContext,
        _cols: u16,
        _rows: u16,
    ) -> Result<LaunchPlan, PlanError> {
        Err(PlanError::Unsupported("no takeover"))
    }

    fn plan_probe(&self, _probe: Probe, _ctx: &ProbeContext) -> Option<ProbeRun> {
        None
    }

    fn parse_probe(&self, _probe: Probe, _out: &ProbeOutput) -> ProbeResult {
        ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled,
        }
    }
}

fn next_id() -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(500);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
    bytes[7] = (seq & 0xff) as u8;
    bytes[8] = 0x80;
    bytes[15] = (seq & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

struct WakeFixture {
    office: Arc<Office>,
    owner_id: String,
    agent_a: String,
    agent_b: String,
    _bin_dir: tempfile::TempDir,
    _sock_dir: tempfile::TempDir,
    script_path: PathBuf,
    recorded: Arc<RecordedLaunches>,
}

impl WakeFixture {
    fn new(script: &str) -> Self {
        let _ = org_platform::process::harden_self();
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let owner_id = office.owner_member_id().to_owned();
        let snap = office.org_snapshot().unwrap();
        let root_team_id = snap.teams[0].id.clone();

        let prof_id = next_id();
        let conn_req = Request::new::<ProviderConnectionCreate>(
            1,
            &ProviderConnectionCreateParams {
                command_id: next_id(),
                connection_id: next_id(),
                default_profile_id: prof_id.clone(),
                key: "claude-wake".into(),
                name: "Claude Wake".into(),
                provider_type: "anthropic".into(),
                auth_mode: "api_key".into(),
                subscription_identity: None,
                runtime_driver: "claude-code".into(),
                endpoint_url: None,
                max_concurrent_turns: 4,
            },
        )
        .unwrap();
        let _ = office.dispatch_hiring_rpc(Some(&owner_id), &conn_req);

        let agent_a = Self::hire(&office, &owner_id, &root_team_id, &prof_id, "Ada", "ada");
        let agent_b = Self::hire(
            &office,
            &owner_id,
            &root_team_id,
            &prof_id,
            "Grace",
            "grace",
        );

        let bin_dir = tempfile::tempdir().unwrap();
        let script_path = bin_dir.path().join("wake-scripted");
        fs::write(&script_path, script).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            office,
            owner_id,
            agent_a,
            agent_b,
            _bin_dir: bin_dir,
            _sock_dir: tempfile::tempdir().unwrap(),
            script_path,
            recorded: Arc::new(RecordedLaunches::default()),
        }
    }

    fn hire(
        office: &Office,
        owner_id: &str,
        team_id: &str,
        prof_id: &str,
        name: &str,
        handle: &str,
    ) -> String {
        let req = Request::new::<HrHire>(
            2,
            &HrHireParams {
                command_id: next_id(),
                request_id: next_id(),
                employee_id: next_id(),
                home_team_id: team_id.to_owned(),
                role_id: None,
                proposal: HireProposalWire {
                    name: name.into(),
                    handle: handle.into(),
                    title: Some("Engineer".into()),
                    focus: None,
                    role_text: None,
                    title_in_team: Some("Engineer".into()),
                    reports_to_employee_id: Some(owner_id.to_owned()),
                    office_id: None,
                    model_profile_id: Some(prof_id.to_owned()),
                    model_id: Some("claude-sonnet-4-6".into()),
                    permission_preset: Some("workspace_edits".into()),
                    tools_config: Vec::new(),
                    system_prompt_markdown: None,
                    body: Some("Work concisely.".into()),
                },
                reason: "Test hire".into(),
            },
        )
        .unwrap();
        let resp = office.dispatch_hiring_rpc(Some(owner_id), &req).unwrap();
        let org_proto::Outcome::Result(val) = resp.outcome else {
            panic!("hire failed: {resp:?}");
        };
        let hired: org_proto::hiring::HireOpResult = serde_json::from_value(val).unwrap();
        hired.employee_id.unwrap()
    }

    fn make_manager(
        &self,
        hourly_limit: usize,
        wake_window: Duration,
        suspend_grace: Duration,
    ) -> SessionManager {
        let mut reg = RuntimeRegistry::production();
        reg.register(
            Arc::new(WakeScriptedRuntime {
                recorded: Arc::clone(&self.recorded),
            }),
            Some(self.script_path.clone()),
        );
        let mut cfg = SessionManagerConfig::production(self._sock_dir.path().join("office.sock"));
        cfg.runtimes = reg;
        cfg.hourly_wake_limit = hourly_limit;
        cfg.wake_window = wake_window;
        cfg.suspend_grace = suspend_grace;
        SessionManager::new(Arc::clone(&self.office), cfg)
    }
}

fn wait_until(pred: impl Fn() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    pred()
}

const LOOP_ECHO_SCRIPT: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  printf '{"kind":"reply","text":"ack"}\n'
done
"#;

const SLOW_TURN_SCRIPT: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  echo '{"kind":"tool_start","id":"bg-helper-1"}'
  sleep 0.25
  printf '{"kind":"reply","text":"done"}\n'
done
"#;

#[test]
fn c6_required_ask_suspends_on_turn_end_survives_restart_and_resumes_same_session() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr1 = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    // Register open required ask (from a sub-agent, ASK-12) before turn ends (ASK-11).
    mgr1.on_required_ask_opened(
        &fx.agent_a,
        SuspendedAskEntry {
            ask_id: "ask-req-1".into(),
            conversation_id: "conv-1".into(),
            question_summary: "Which target region should we deploy to?".into(),
            waiting_for_member_id: Some(fx.owner_id.clone()),
            waiting_for_name: Some("Owner".into()),
            asked_by_subagent: true,
            opened_at_ms: now_ms(),
        },
    );
    mgr1.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Deploy service".into()),
            workspace: None,
        },
    )
    .unwrap();

    // Turn ends -> immediately suspends without waiting for timer (ASK-11) & releases binding (ASK-4).
    assert!(
        wait_until(
            || mgr1
                .store()
                .read_suspended_state(&fx.agent_a)
                .ok()
                .flatten()
                .is_some(),
            Duration::from_secs(3)
        ),
        "agent did not suspend on turn end with open required ask"
    );
    assert!(!mgr1.bindings().has_active(&fx.agent_a));
    let rt = mgr1.member_runtime_presence(&fx.agent_a).unwrap();
    assert_eq!(rt.presence, MemberPresence::WaitingForAnswer);
    assert!(rt
        .status
        .unwrap()
        .contains("Waiting for an answer from Owner"));

    // Simulate full office restart (ASK-6): fresh SessionManager on same office durable root!
    mgr1.stop_all("restart");
    let mgr2 = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let inspected = mgr2
        .inspect_session(&fx.owner_id, Some(&fx.agent_a))
        .unwrap();
    assert_eq!(inspected.session.state, SessionState::Suspended);
    assert_eq!(inspected.session.suspended_on_asks, vec!["ask-req-1"]);
    let orig_rsid = inspected.session.runtime_session_id.clone();
    assert!(orig_rsid.is_some());

    // Budget hold blocks resume without losing the answer (ASK-8).
    mgr2.set_budget_hold(Some("monthly cap".into()));
    let held = mgr2.on_ask_answered(
        &fx.agent_a,
        "ask-req-1",
        "Which target region should we deploy to?",
        "eu-west-1",
        &fx.owner_id,
        "Owner",
        true,
        true,
    );
    assert_eq!(
        held,
        WakeOutcome::Queued {
            reason: WakeHoldReason::BudgetHold
        }
    );

    // Clearing budget hold resumes the EXACT same runtime_session_id with question + answer (ASK-5/8/12).
    mgr2.set_budget_hold(None);
    assert!(wait_until(
        || fx.recorded.turns.lock().unwrap().len() >= 2,
        Duration::from_secs(3)
    ));
    let resumes = fx.recorded.resumes.lock().unwrap().clone();
    assert_eq!(resumes.len(), 2);
    assert_eq!(resumes[0], None);
    assert_eq!(resumes[1], orig_rsid);
    let resumed_input = fx.recorded.turns.lock().unwrap()[1].clone();
    assert!(resumed_input.contains("Which target region should we deploy to?"));
    assert!(resumed_input.contains("eu-west-1"));
    assert!(resumed_input.contains("sub-agent"));
    mgr2.stop_all("done");
}

#[test]
fn c6_grace_period_suspends_and_names_reaped_background_work_on_resume() {
    let fx = WakeFixture::new(SLOW_TURN_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_millis(80));

    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Start long background task".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .ok()
            .is_some_and(|r| !r.session.background_tasks.is_empty()),
        Duration::from_secs(3)
    ));

    // Open required ask + wait expired arms grace timer (ASK-3), reaping background work (ASK-7).
    mgr.on_required_ask_opened(
        &fx.agent_a,
        SuspendedAskEntry {
            ask_id: "ask-bg-1".into(),
            conversation_id: "conv-bg".into(),
            question_summary: "Approve schema migration?".into(),
            waiting_for_member_id: Some(fx.agent_b.clone()),
            waiting_for_name: Some("Grace".into()),
            asked_by_subagent: false,
            opened_at_ms: now_ms(),
        },
    );
    mgr.on_required_ask_wait_expired(&fx.agent_a, "ask-bg-1");
    assert!(wait_until(
        || mgr
            .store()
            .read_suspended_state(&fx.agent_a)
            .ok()
            .flatten()
            .is_some(),
        Duration::from_secs(3)
    ));
    let susp = mgr
        .store()
        .read_suspended_state(&fx.agent_a)
        .unwrap()
        .unwrap();
    assert!(
        !susp.reaped_background_tasks.is_empty(),
        "expected reaped background tasks recorded: {susp:?}"
    );

    // Peer agent answers (ASK-9); resumed turn names reaped background work (ASK-7).
    let outcome = mgr.on_ask_answered(
        &fx.agent_a,
        "ask-bg-1",
        "Approve schema migration?",
        "Approved",
        &fx.agent_b,
        "Grace",
        false,
        false,
    );
    assert_eq!(outcome, WakeOutcome::Started);
    assert!(wait_until(
        || fx.recorded.turns.lock().unwrap().len() >= 2,
        Duration::from_secs(3)
    ));
    let resumed_input = fx.recorded.turns.lock().unwrap()[1].clone();
    assert!(resumed_input.contains("Background work stopped during suspension"));
    assert!(resumed_input.contains("bg-helper-1"));
    mgr.stop_all("done");
}

#[test]
fn c8_busy_agent_coalesces_three_arriving_wakes_into_one_follow_up_turn() {
    let fx = WakeFixture::new(SLOW_TURN_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Initial work".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .ok()
            .is_some_and(|r| r.session.state == SessionState::Running),
        Duration::from_secs(2)
    ));
    for i in 1..=3 {
        let res = mgr.enqueue_wake(
            &fx.agent_a,
            PendingWakeItem {
                item_id: format!("m-{i}"),
                kind: WakeKind::Handoff,
                sender_id: fx.agent_b.clone(),
                sender_name: "Grace".into(),
                sender_is_person: false,
                conversation_id: Some("c1".into()),
                source_id: format!("msg-{i}"),
                summary: format!("Queued item {i}"),
                enqueued_at_ms: now_ms(),
            },
        );
        assert_eq!(
            res,
            WakeOutcome::Queued {
                reason: WakeHoldReason::Busy
            }
        );
    }
    assert!(wait_until(
        || fx.recorded.turns.lock().unwrap().len() >= 2,
        Duration::from_secs(3)
    ));
    let coalesced = fx.recorded.turns.lock().unwrap()[1].clone();
    assert!(coalesced.contains("Queued item 1"));
    assert!(coalesced.contains("Queued item 2"));
    assert!(coalesced.contains("Queued item 3"));
    mgr.stop_all("done");
}

#[test]
fn c8_hourly_limit_person_exemption_and_post_restart_pending_wakes() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(2, Duration::from_millis(2000), Duration::from_secs(2));

    let make_agent_wake = |id: &str| PendingWakeItem {
        item_id: id.into(),
        kind: WakeKind::Ask,
        sender_id: fx.agent_b.clone(),
        sender_name: "Grace".into(),
        sender_is_person: false,
        conversation_id: None,
        source_id: id.into(),
        summary: format!("Peer ask {id}"),
        enqueued_at_ms: now_ms(),
    };

    assert_eq!(
        mgr.enqueue_wake(&fx.agent_a, make_agent_wake("ask-1")),
        WakeOutcome::Started
    );
    mgr.stop_all("step1");
    assert_eq!(
        mgr.enqueue_wake(&fx.agent_a, make_agent_wake("ask-2")),
        WakeOutcome::Started
    );
    mgr.stop_all("step2");
    assert_eq!(
        mgr.enqueue_wake(&fx.agent_a, make_agent_wake("ask-3")),
        WakeOutcome::Queued {
            reason: WakeHoldReason::HourlyLimit
        }
    );

    // Person task assignment is exempt from hourly limit (AVAIL-2, AVAIL-6)!
    let p_wake = mgr.on_task_assigned(
        &fx.agent_a,
        &fx.owner_id,
        "Owner",
        true,
        "T-A99",
        "Urgent prod fix",
    );
    assert_eq!(p_wake, WakeOutcome::Started);
    mgr.stop_all("step3");

    // Simulate window expiry (LIVE-3) + full office restart (LIVE-4) with durable pending wake.
    std::thread::sleep(Duration::from_millis(2100));
    let mgr_after_restart = fx.make_manager(2, Duration::from_millis(2000), Duration::from_secs(2));
    mgr_after_restart
        .store()
        .write_pending_wakes(&crate::wake::PendingWakeQueueRecord {
            version: 1,
            employee_id: fx.agent_a.clone(),
            items: vec![PendingWakeItem {
                item_id: "after-restart-1".into(),
                kind: WakeKind::Mention,
                sender_id: fx.agent_b.clone(),
                sender_name: "Grace".into(),
                sender_is_person: false,
                conversation_id: Some("c-gen".into()),
                source_id: "msg-restart".into(),
                summary: "Survives restart".into(),
                enqueued_at_ms: now_ms(),
            }],
            hold_reason: Some(WakeHoldReason::HourlyLimit),
            held_since_ms: Some(now_ms().saturating_sub(2200)),
            retry_after_ms: Some(now_ms().saturating_sub(10)),
            updated_at: crate::credentials::rfc3339_now(),
        })
        .unwrap();
    assert_eq!(
        mgr_after_restart.process_due_wakes_for(&fx.agent_a),
        WakeOutcome::Started
    );
    mgr_after_restart.stop_all("done");
}

#[test]
fn c8_mutual_suspension_deadlock_breaker_resumes_peer_without_human_intervention() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    // Agent B is suspended waiting for an answer from Agent A (LIVE-8 / ADR-0030 d7).
    mgr.store()
        .write_suspended_state(&crate::wake::SuspendedStateRecord {
            version: 1,
            employee_id: fx.agent_b.clone(),
            session_id: "sess-b-1".into(),
            runtime_session_id: Some("rsid-b-1".into()),
            suspended_since: crate::credentials::rfc3339_now(),
            asks: vec![SuspendedAskEntry {
                ask_id: "ask-b-to-a".into(),
                conversation_id: "dm-ab".into(),
                question_summary: "What API version do you emit?".into(),
                waiting_for_member_id: Some(fx.agent_a.clone()),
                waiting_for_name: Some("Ada".into()),
                asked_by_subagent: false,
                opened_at_ms: now_ms(),
            }],
            reaped_background_tasks: Vec::new(),
            pending_resume_answers: Vec::new(),
            hold_reason: None,
            held_since_ms: None,
            updated_at: crate::credentials::rfc3339_now(),
        })
        .unwrap();

    // An ordinary message from someone else stays queued as Suspended (WAKE-3).
    let blocked = mgr.enqueue_wake(
        &fx.agent_b,
        PendingWakeItem {
            item_id: "other-dm".into(),
            kind: WakeKind::DirectMessage,
            sender_id: fx.owner_id.clone(),
            sender_name: "Owner".into(),
            sender_is_person: true,
            conversation_id: Some("dm-ob".into()),
            source_id: "msg-ob".into(),
            summary: "Just checking in".into(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(
        blocked,
        WakeOutcome::Queued {
            reason: WakeHoldReason::Suspended
        }
    );

    // An ask from Agent A (whom B is waiting on) breaks the mutual deadlock and wakes B!
    let broken = mgr.enqueue_wake(
        &fx.agent_b,
        PendingWakeItem {
            item_id: "ask-a-to-b".into(),
            kind: WakeKind::Ask,
            sender_id: fx.agent_a.clone(),
            sender_name: "Ada".into(),
            sender_is_person: false,
            conversation_id: Some("dm-ab".into()),
            source_id: "ask-a-to-b".into(),
            summary: "Which auth header format?".into(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(broken, WakeOutcome::Started);
    assert!(wait_until(
        || !fx.recorded.turns.lock().unwrap().is_empty(),
        Duration::from_secs(3)
    ));
    let turn_text = fx.recorded.turns.lock().unwrap()[0].clone();
    assert!(turn_text.contains("Which auth header format?"));
    assert!(turn_text.contains("What API version do you emit?"));
    assert_eq!(
        fx.recorded.resumes.lock().unwrap()[0].as_deref(),
        Some("rsid-b-1")
    );
    mgr.stop_all("done");
}

#[test]
fn pass1_idle_live_session_accepts_subsequent_wake_and_person_run_clears_stall_lock() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    // 1. High #1 regression: start a session, wait until Idle (binding stays active!), then
    // enqueue a peer wake -> MUST dispatch immediately instead of sticking in Busy forever.
    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("First turn".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .ok()
            .is_some_and(|r| r.session.state == SessionState::Idle),
        Duration::from_secs(3)
    ));
    let wake = mgr.enqueue_wake(
        &fx.agent_a,
        PendingWakeItem {
            item_id: "idle-wake-1".into(),
            kind: WakeKind::DirectMessage,
            sender_id: fx.owner_id.clone(),
            sender_name: "Owner".into(),
            sender_is_person: true,
            conversation_id: Some("dm-oa".into()),
            source_id: "msg-idle-1".into(),
            summary: "Follow up while live idle".into(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(wake, WakeOutcome::Started);
    assert!(wait_until(
        || fx.recorded.turns.lock().unwrap().len() >= 2,
        Duration::from_secs(3)
    ));
    mgr.stop_all("step1");

    // 2. Medium #9 regression: person Run clears stall_locked (LIVE-13), while autonomous wake stays held.
    let mut rec = mgr.store().read_active_turn(&fx.agent_a).unwrap().unwrap();
    rec.stall_locked = true;
    rec.consecutive_stalls = 3;
    mgr.store().write_active_turn(&rec).unwrap();
    let held = mgr.enqueue_wake(
        &fx.agent_a,
        PendingWakeItem {
            item_id: "stall-held-1".into(),
            kind: WakeKind::Ask,
            sender_id: fx.agent_b.clone(),
            sender_name: "Grace".into(),
            sender_is_person: false,
            conversation_id: None,
            source_id: "ask-s1".into(),
            summary: "Should stay held".into(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(
        held,
        WakeOutcome::Queued {
            reason: WakeHoldReason::StallLocked
        }
    );
    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Owner manually resumes stalled agent".into()),
            workspace: None,
        },
    )
    .expect("person Run must clear stall_locked and start");
    mgr.stop_all("done");
}

#[test]
fn pass1_concurrent_live8_deadlock_process_due_wakes_and_redaction() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_millis(80));

    // 1. High #2 regression: agent B is suspended on A AND already has A's ask queued in
    // pending-wakes.v1.json (real concurrent LIVE-8); process_due_wakes breaks deadlock!
    mgr.store()
        .write_suspended_state(&crate::wake::SuspendedStateRecord {
            version: 1,
            employee_id: fx.agent_b.clone(),
            session_id: "sess-b-live8".into(),
            runtime_session_id: Some("rsid-b-live8".into()),
            suspended_since: crate::credentials::rfc3339_now(),
            asks: vec![SuspendedAskEntry {
                ask_id: "ask-b-live8".into(),
                conversation_id: "dm-ab".into(),
                question_summary: "Need secret sk-ant-oat01-SECRETTOKEN1234567890".into(),
                waiting_for_member_id: Some(fx.agent_a.clone()),
                waiting_for_name: Some("Ada".into()),
                asked_by_subagent: false,
                opened_at_ms: now_ms(),
            }],
            reaped_background_tasks: Vec::new(),
            pending_resume_answers: Vec::new(),
            hold_reason: None,
            held_since_ms: None,
            updated_at: crate::credentials::rfc3339_now(),
        })
        .unwrap();
    // Medium #12: secret prefix is redacted on disk!
    let raw_susp =
        fs::read_to_string(mgr.store().suspended_state_path(&fx.agent_b).unwrap()).unwrap();
    assert!(!raw_susp.contains("SECRETTOKEN1234567890"), "{raw_susp}");

    mgr.store()
        .write_pending_wakes(&crate::wake::PendingWakeQueueRecord {
            version: 1,
            employee_id: fx.agent_b.clone(),
            items: vec![PendingWakeItem {
                item_id: "live8-queued-ask".into(),
                kind: WakeKind::Ask,
                sender_id: fx.agent_a.clone(),
                sender_name: "Ada".into(),
                sender_is_person: false,
                conversation_id: Some("dm-ab".into()),
                source_id: "ask-a-live8".into(),
                summary: "Concurrent peer ask".into(),
                enqueued_at_ms: now_ms(),
            }],
            hold_reason: Some(WakeHoldReason::Busy),
            held_since_ms: Some(now_ms().saturating_sub(50)),
            retry_after_ms: None,
            updated_at: crate::credentials::rfc3339_now(),
        })
        .unwrap();
    assert_eq!(mgr.process_due_wakes_for(&fx.agent_b), WakeOutcome::Started);
    assert!(wait_until(
        || !fx.recorded.turns.lock().unwrap().is_empty(),
        Duration::from_secs(3)
    ));
    mgr.stop_all("done");
}

const GRACE_HOLD_SCRIPT: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  sleep 1.2
  printf '{"kind":"reply","text":"done"}\n'
done
"#;

#[test]
fn pass2_grace_period_disarm_prevents_stale_deadline_on_subsequent_ask() {
    let fx = WakeFixture::new(GRACE_HOLD_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_millis(250));

    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Long running turn".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .unwrap()
            .session
            .state
            == SessionState::Running,
        Duration::from_secs(3)
    ));

    // Arm suspend grace deadline for ask-1.
    mgr.on_required_ask_opened(
        &fx.agent_a,
        SuspendedAskEntry {
            ask_id: "ask-grace-1".into(),
            conversation_id: "conv-1".into(),
            question_summary: "Need quick approval".into(),
            waiting_for_member_id: Some(fx.owner_id.clone()),
            waiting_for_name: Some("Owner".into()),
            asked_by_subagent: false,
            opened_at_ms: now_ms(),
        },
    );
    mgr.on_required_ask_wait_expired(&fx.agent_a, "ask-grace-1");

    // Answer ask-1 well before the 250ms grace deadline, then allow pump ticks to disarm it.
    let _ = mgr.on_ask_answered(
        &fx.agent_a,
        "ask-grace-1",
        "Need quick approval",
        "Approved",
        &fx.owner_id,
        "Owner",
        true,
        false,
    );
    std::thread::sleep(Duration::from_millis(90));

    // Open ask-2 BEFORE ask-1's original 250ms deadline would have expired.
    // Because ask-1's deadline was disarmed immediately when open asks hit zero, ask-2 must NOT
    // trigger an immediate suspend when ask-1's original 250ms elapses.
    mgr.on_required_ask_opened(
        &fx.agent_a,
        SuspendedAskEntry {
            ask_id: "ask-grace-2".into(),
            conversation_id: "conv-1".into(),
            question_summary: "Second check".into(),
            waiting_for_member_id: Some(fx.owner_id.clone()),
            waiting_for_name: Some("Owner".into()),
            asked_by_subagent: false,
            opened_at_ms: now_ms(),
        },
    );
    std::thread::sleep(Duration::from_millis(240));
    assert_eq!(
        mgr.inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .unwrap()
            .session
            .state,
        SessionState::Running,
        "stale ask-1 grace deadline must not suspend session for ask-2"
    );
    assert!(mgr
        .store()
        .read_suspended_state(&fx.agent_a)
        .unwrap()
        .is_none());

    // Withdraw ask-2 before turn finishes so the session returns to Idle, not Suspended.
    mgr.on_ask_withdrawn(&fx.agent_a, "ask-grace-2");
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .unwrap()
            .session
            .state
            == SessionState::Idle,
        Duration::from_secs(3)
    ));
    mgr.stop_all("done");
}

#[test]
fn l3_office_agent_budget_hold_blocks_wake_until_extended_or_cleared() {
    use org_proto::budgets::{
        BudgetExtendOnceParams, BudgetPeriodKind, BudgetScopeKind, BudgetSetParams,
        BudgetTimeZoneSource, BudgetUnit,
    };
    use org_proto::costs::{
        AttributionBasis, CostUsageRecordParams, UsageActorKind, UsageScopeKind,
    };

    let fx = WakeFixture::new(SLOW_TURN_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    // Set a 1,000-token daily budget for agent_a and record 1,000 tokens -> triggers L3 hold!
    let budget = fx
        .office
        .budget_set(
            &fx.owner_id,
            BudgetSetParams {
                operation_id: next_id(),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(fx.agent_a.clone()),
                release_id: None,
                period_kind: BudgetPeriodKind::Day,
                unit: BudgetUnit::Tokens,
                limit_amount: 1_000,
                time_zone: Some("UTC".into()),
                time_zone_source: Some(BudgetTimeZoneSource::OwnerPerson),
                reason: None,
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    let occurred_at =
        org_office::format_rfc3339_ms(org_office::organization_family::testkit::CREATION_AT.0);
    fx.office
        .record_office_usage(CostUsageRecordParams {
            id: None,
            producer_id: "test-session".into(),
            source_event_id: "ev-budget-hit".into(),
            run_id: None,
            actor_kind: UsageActorKind::Agent,
            employee_id: Some(fx.agent_a.clone()),
            runtime: "fake".into(),
            provider: None,
            model: None,
            occurred_at,
            source_metadata_json: None,
            input_tokens: Some(600),
            output_tokens: Some(400),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: Some(0),
            token_issue: None,
            runtime_cost_micro_usd: None,
            is_subscription: false,
            scope_kind: UsageScopeKind::Organization,
            task_id: None,
            project_id: None,
            team_id: None,
            feature_id: None,
            milestone_id: None,
            release_id: None,
            task_type_snapshot: None,
            work_type: None,
            attribution_basis: AttributionBasis::Unattributed,
        })
        .unwrap()
        .unwrap();

    // Wake for agent_a is held on BudgetHold, while agent_b is unaffected!
    let held = mgr.on_task_assigned(
        &fx.agent_a,
        &fx.owner_id,
        "Owner",
        true,
        "task-1",
        "Check status",
    );
    assert_eq!(
        held,
        WakeOutcome::Queued {
            reason: WakeHoldReason::BudgetHold
        }
    );

    // One-time +20% extension (1_000 -> 1_200 > 1_000) releases hold and retry_budget_held wakes agent_a!
    fx.office
        .budget_extend_once(
            &fx.owner_id,
            BudgetExtendOnceParams {
                operation_id: next_id(),
                budget_id: budget.id,
                reason: Some("finish turn".into()),
            },
        )
        .unwrap()
        .unwrap();
    mgr.retry_budget_held();
    assert!(wait_until(
        || !fx.recorded.turns.lock().unwrap().is_empty(),
        Duration::from_secs(3)
    ));
    mgr.stop_all("done");
}
