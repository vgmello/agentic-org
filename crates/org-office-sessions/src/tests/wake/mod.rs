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

mod lock_scope;
mod mid_turn;
mod queue;
mod suspend;

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
                    capability: Some(org_agent_runtime::ToolCapability::Subagent),
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
            mid_turn_delivery: org_agent_runtime::MidTurnDelivery::None,
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

        let conn_id = next_id();
        let prof_id = next_id();
        let conn_req = Request::new::<ProviderConnectionCreate>(
            1,
            &ProviderConnectionCreateParams {
                command_id: next_id(),
                connection_id: conn_id.clone(),
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
        office
            .provider_binding_set(
                &owner_id,
                org_proto::providers::ProviderBindingSetParams {
                    operation_id: next_id(),
                    connection_id: conn_id.clone(),
                    expected_revision: None,
                    auth_context_ref: "vault://claude-wake".into(),
                    runtime_binary_path: None,
                    account_identity_ref: None,
                },
            )
            .unwrap()
            .unwrap();
        office
            .provider_connection_refresh(
                &owner_id,
                org_proto::providers::ProviderConnectionRefreshParams {
                    connection_id: conn_id,
                    driver_version: Some("1.0.0".into()),
                    auth_signed_in: Some(true),
                    observed_subscription_identity: None,
                    operator_attested_subscription: false,
                    models: vec![org_proto::providers::DiscoveredModelInput {
                        model_id: "claude-sonnet-4-6".into(),
                        driver_model_id: "claude-sonnet-4-6".into(),
                        entitlement: org_proto::providers::DiscoveredModelEntitlement::Confirmed,
                        capabilities_json: "{}".into(),
                    }],
                    issue_code: None,
                    ttl_seconds: Some(300),
                },
            )
            .unwrap()
            .unwrap();

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
                    effort: None,
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

const GRACE_HOLD_SCRIPT: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  sleep 1.2
  printf '{"kind":"reply","text":"done"}\n'
done
"#;
