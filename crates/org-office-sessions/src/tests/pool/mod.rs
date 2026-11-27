//! End-to-end real-process tests for `TASK-54` (`L4` provider subscription pools, `endpoint_url`
//! isolation, §7 failover vs side-effect guard, and `ASK-4` / `LIVE-2` turn-permit serialization).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    encode_line, AgentRuntime, Capabilities, Control, ControlError, CostBasis, EnvName, ExitInfo,
    LaunchContext, LaunchPlan, LimitStatus, Output, PlanError, Probe, ProbeContext, ProbeOutput,
    ProbeResult, ProbeRun, RateLimit, RuntimeId, Session, SessionEvent, SessionId, SessionSpec,
    SignalTarget, Stdio, StopPlan, StopSignal, StopStep, TurnOutcome, Unavailable, UsageScope,
    VersionRange, Window,
};
use org_office::{Office, TestOffice};
use org_proto::directory::MemberPresence;
use org_proto::hiring::{
    AgentExecutionSet, AgentExecutionSetParams, HireProposalWire, HrHire, HrHireParams,
    ProviderConnectionCreate, ProviderConnectionCreateParams,
};
use org_proto::providers::{
    ConnectionQuotaStatus, DiscoveredModelEntitlement, DiscoveredModelInput,
    ModelProfileCreateParams, ProviderBindingSetParams, ProviderConnectionRefreshParams,
    ProviderQuotaStatusParams,
};
use org_proto::sessions::{MemberSessionStartParams, SessionState};
use org_proto::Request;

use crate::credentials::now_ms;
use crate::manager::{SessionManager, SessionManagerConfig};
use crate::router::{classify_failover_error, endpoint_env_for};
use crate::runtime_registry::RuntimeRegistry;
use crate::wake::{PendingWakeItem, SuspendedAskEntry, WakeHoldReason, WakeKind, WakeOutcome};

mod failover;
mod queued_turns;
mod start_races;

#[derive(Default)]
struct PoolRecorded {
    observed_endpoints: Mutex<Vec<String>>,
    turns: Mutex<Vec<String>>,
}

struct PoolScriptedRuntime {
    recorded: Arc<PoolRecorded>,
}

struct PoolScriptedSession {
    recorded: Arc<PoolRecorded>,
    rsid: String,
}

impl Session for PoolScriptedSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("ready") => {
                if let Some(ep) = v.get("endpoint").and_then(|e| e.as_str()) {
                    self.recorded
                        .observed_endpoints
                        .lock()
                        .unwrap()
                        .push(ep.to_owned());
                }
                vec![
                    Output::Event(SessionEvent::SessionStarted {
                        runtime_session_id: SessionId::try_from(self.rsid.as_str()).unwrap(),
                    }),
                    Output::Event(SessionEvent::Ready {
                        runtime_version: Some("1.0.0".into()),
                        pid: None,
                        agents: vec!["ada".into()],
                        permission_mode: Some("default".into()),
                    }),
                ]
            }
            Some("tool_start") => vec![Output::Event(SessionEvent::ToolStarted {
                tool_use_id: "tool-1".into(),
                name: "Bash".into(),
                capability: Some(org_agent_runtime::ToolCapability::Shell),
                input: serde_json::json!({ "command": "echo hi" }),
            })],
            Some("rate_limit_fail") => vec![
                Output::Event(SessionEvent::RateLimit(RateLimit::new(
                    Window::FiveHour,
                    LimitStatus::Rejected,
                ))),
                Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        reason: "429 rate_limited".into(),
                    },
                    errors: vec!["rate_limited".into()],
                }),
            ],
            Some("auth_expired_fail") => vec![Output::Event(SessionEvent::TurnEnded {
                outcome: TurnOutcome::SignedOut,
                errors: vec!["401 auth_expired".into()],
            })],
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

impl AgentRuntime for PoolScriptedRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }

    fn binary_name(&self) -> &'static str {
        "pool-scripted"
    }

    fn host_profile(&self) -> org_agent_runtime::RuntimeHostProfile {
        org_agent_runtime::RuntimeHostProfile {
            uses_host_oauth_token_injection: true,
            ..org_agent_runtime::RuntimeHostProfile::default()
        }
    }

    fn endpoint_env_var(&self) -> Option<&'static str> {
        Some("ANTHROPIC_BASE_URL")
    }

    fn auth_env_var(&self, subscription_cli: bool, secret: &str) -> Option<&'static str> {
        Some(if subscription_cli || secret.starts_with("sk-ant-oat") {
            "CLAUDE_CODE_OAUTH_TOKEN"
        } else {
            "ANTHROPIC_API_KEY"
        })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mid_turn_delivery: org_agent_runtime::MidTurnDelivery::None,
            image_input: org_agent_runtime::ImageInputMode::StagedFileOnly,
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
        let rsid = ctx
            .resume
            .as_ref()
            .or(ctx.new_session_id.as_ref())
            .map(|s| s.as_str().to_owned())
            .unwrap_or_else(|| "sess-pool-001".into());
        let plan = LaunchPlan {
            program: ctx.runtime_path.clone(),
            args: Vec::new(),
            cwd: spec.workspace.clone(),
            stdio: Stdio::Piped,
            files: Vec::new(),
            dirs: Vec::new(),
            env_add: Vec::new(),
            auth_env_names: [
                "ANTHROPIC_BASE_URL",
                "ANTHROPIC_API_KEY",
                "CLAUDE_CODE_OAUTH_TOKEN",
            ]
            .into_iter()
            .map(|n| EnvName::try_from(n).unwrap())
            .collect(),
            secret_env_names: ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]
                .into_iter()
                .map(|n| EnvName::try_from(n).unwrap())
                .collect(),
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
            Box::new(PoolScriptedSession {
                recorded: Arc::clone(&self.recorded),
                rsid,
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
    let ms = org_office::organization_family::testkit::CREATION_AT.0 + 100;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(900);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
    bytes[7] = (seq & 0xff) as u8;
    bytes[8] = 0x80;
    bytes[15] = (seq & 0xff) as u8;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        u16::from_be_bytes([bytes[8], bytes[9]]),
        u64::from_be_bytes([
            0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        ]),
    )
}

fn write_script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn setup_office_and_manager(
    script_body: &str,
) -> (
    tempfile::TempDir,
    Arc<Office>,
    Arc<SessionManager>,
    Arc<PoolRecorded>,
    String,
) {
    let _ = org_platform::process::harden_self();
    let tmp = tempfile::tempdir().unwrap();
    let script = write_script(tmp.path(), "pool-scripted", script_body);
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let recorded = Arc::new(PoolRecorded::default());
    let mut runtimes = RuntimeRegistry::production();
    runtimes.register(
        Arc::new(PoolScriptedRuntime {
            recorded: Arc::clone(&recorded),
        }),
        Some(script),
    );
    let mut cfg = SessionManagerConfig::production(tmp.path().join("office.sock"));
    cfg.runtimes = runtimes;
    cfg.helper_dir = Some(tmp.path().to_path_buf());
    cfg.suspend_grace = Duration::from_millis(120);
    let mgr = Arc::new(SessionManager::new(Arc::clone(&office), cfg));
    SessionManager::attach_self(&mgr);
    (tmp, office, mgr, recorded, owner)
}

fn create_connection_and_bind(
    office: &Office,
    owner: &str,
    key: &str,
    endpoint_url: Option<&str>,
    max_concurrent: u64,
    models: &[(&str, &str)],
) -> String {
    let conn_id = next_id();
    let req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: next_id(),
            connection_id: conn_id.clone(),
            default_profile_id: next_id(),
            key: key.into(),
            name: key.into(),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude".into(),
            endpoint_url: endpoint_url.map(str::to_owned),
            max_concurrent_turns: max_concurrent,
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(
        matches!(resp.outcome, org_proto::Outcome::Result(_)),
        "create connection failed: {resp:?}"
    );
    office
        .provider_binding_set(
            owner,
            ProviderBindingSetParams {
                operation_id: next_id(),
                connection_id: conn_id.clone(),
                expected_revision: None,
                auth_context_ref: format!("vault://{key}"),
                runtime_binary_path: None,
                account_identity_ref: None,
            },
        )
        .unwrap()
        .unwrap();
    office
        .provider_connection_refresh(
            owner,
            ProviderConnectionRefreshParams {
                connection_id: conn_id.clone(),
                driver_version: Some("1.0.0".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: None,
                operator_attested_subscription: false,
                models: models
                    .iter()
                    .map(|(m, dm)| DiscoveredModelInput {
                        model_id: (*m).into(),
                        driver_model_id: (*dm).into(),
                        entitlement: DiscoveredModelEntitlement::Confirmed,
                        capabilities_json: "{}".into(),
                    })
                    .collect(),
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();
    conn_id
}

fn create_subscription_connection_and_bind(
    office: &Office,
    owner: &str,
    key: &str,
    endpoint_url: Option<&str>,
    max_concurrent: u64,
    models: &[(&str, &str)],
) -> String {
    let conn_id = next_id();
    let req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: next_id(),
            connection_id: conn_id.clone(),
            default_profile_id: next_id(),
            key: key.into(),
            name: key.into(),
            provider_type: "anthropic".into(),
            auth_mode: "subscription_cli".into(),
            subscription_identity: Some(format!("{key}@example.com")),
            runtime_driver: "claude".into(),
            endpoint_url: endpoint_url.map(str::to_owned),
            max_concurrent_turns: max_concurrent,
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(
        matches!(resp.outcome, org_proto::Outcome::Result(_)),
        "create subscription connection failed: {resp:?}"
    );
    office
        .provider_binding_set(
            owner,
            ProviderBindingSetParams {
                operation_id: next_id(),
                connection_id: conn_id.clone(),
                expected_revision: None,
                auth_context_ref: format!("vault://{key}"),
                runtime_binary_path: None,
                account_identity_ref: Some(format!("{key}@example.com")),
            },
        )
        .unwrap()
        .unwrap();
    refresh_subscription_connection_ready(office, owner, &conn_id, key, models);
    conn_id
}

fn refresh_subscription_connection_ready(
    office: &Office,
    owner: &str,
    conn_id: &str,
    key: &str,
    models: &[(&str, &str)],
) {
    office
        .provider_connection_refresh(
            owner,
            ProviderConnectionRefreshParams {
                connection_id: conn_id.to_owned(),
                driver_version: Some("1.0.0".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: Some(format!("{key}@example.com")),
                operator_attested_subscription: true,
                models: models
                    .iter()
                    .map(|(m, dm)| DiscoveredModelInput {
                        model_id: (*m).into(),
                        driver_model_id: (*dm).into(),
                        entitlement: DiscoveredModelEntitlement::Confirmed,
                        capabilities_json: "{}".into(),
                    })
                    .collect(),
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();
}

fn hire_agent(
    office: &Office,
    owner: &str,
    handle: &str,
    profile_id: &str,
    model_id: &str,
) -> String {
    let snap = office.org_snapshot().unwrap();
    let team_id = snap.teams[0].id.clone();
    let emp_id = next_id();
    let req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: next_id(),
            request_id: next_id(),
            employee_id: emp_id.clone(),
            home_team_id: team_id,
            role_id: None,
            proposal: HireProposalWire {
                name: format!("Agent {handle}"),
                handle: handle.into(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner.into()),
                office_id: None,
                model_profile_id: Some(profile_id.into()),
                model_id: Some(model_id.into()),
                permission_preset: Some("safe_ask".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: Some("Be helpful.".into()),
                effort: None,
            },
            reason: "pool test".into(),
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(
        matches!(resp.outcome, org_proto::Outcome::Result(_)),
        "hire failed: {resp:?}"
    );
    let set_req = Request::new::<AgentExecutionSet>(
        3,
        &AgentExecutionSetParams {
            command_id: next_id(),
            employee_id: emp_id.clone(),
            expected_version: None,
            model_profile_id: profile_id.into(),
            model_id: model_id.into(),
            tools_config: Vec::new(),
            permission_preset: Some("safe_ask".into()),
            enabled: true,
            system_prompt_markdown: "Be helpful.".into(),
            effort: None,
        },
    )
    .unwrap();
    let set_resp = office.dispatch_hiring_rpc(Some(owner), &set_req).unwrap();
    assert!(
        matches!(set_resp.outcome, org_proto::Outcome::Result(_)),
        "exec set failed: {set_resp:?}"
    );
    emp_id
}

fn wait_until(timeout: Duration, mut pred: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    pred()
}
