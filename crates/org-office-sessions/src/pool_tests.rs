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
            secret_env_names: vec![EnvName::try_from("ANTHROPIC_API_KEY").unwrap()],
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

#[test]
fn endpoint_env_mapping_and_failover_classification_rules() {
    assert_eq!(
        endpoint_env_for("claude", "https://proxy.example.com/v1").unwrap(),
        ("ANTHROPIC_BASE_URL", "https://proxy.example.com/v1".into())
    );
    assert_eq!(
        endpoint_env_for("codex", "https://openai.example.com/v1").unwrap(),
        ("OPENAI_BASE_URL", "https://openai.example.com/v1".into())
    );
    assert_eq!(
        endpoint_env_for("agy", "https://gemini.example.com/v1").unwrap(),
        (
            "GOOGLE_GEMINI_BASE_URL",
            "https://gemini.example.com/v1".into()
        )
    );
    assert!(endpoint_env_for("claude", "http://169.254.169.254/latest").is_err());
    assert!(endpoint_env_for("claude", "ftp://bad.example.com").is_err());

    assert_eq!(classify_failover_error(&[], true), Some("rate_limited"));
    assert_eq!(
        classify_failover_error(&["HTTP 429 too many requests".into()], false),
        Some("rate_limited")
    );
    assert_eq!(
        classify_failover_error(&["quota_exhausted".into()], false),
        Some("quota_exhausted")
    );
    assert_eq!(
        classify_failover_error(&["401 invalid_api_key".into()], false),
        Some("auth_expired")
    );
    assert_eq!(
        classify_failover_error(&["model_unavailable".into()], false),
        Some("model_unavailable")
    );
    assert_eq!(
        classify_failover_error(&["syntax error in prompt".into()], false),
        None
    );
}

#[test]
fn l4_pool_failover_before_side_effects_switches_endpoint_and_records_cooldown() {
    // Conn A rejects with rate_limit; Conn B replies with its isolated ANTHROPIC_BASE_URL!
    let script = r#"#!/bin/sh
echo "{\"kind\":\"ready\",\"endpoint\":\"$ANTHROPIC_BASE_URL\"}"
while IFS= read -r line; do
  if [ "$ANTHROPIC_BASE_URL" = "https://conn-a.example.com/v1" ]; then
    echo '{"kind":"rate_limit_fail"}'
  else
    echo "{\"kind\":\"reply\",\"text\":\"ok-from:$ANTHROPIC_BASE_URL\"}"
  fi
done
"#;
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(script);
    let conn_a = create_connection_and_bind(
        &office,
        &owner,
        "conn-a",
        Some("https://conn-a.example.com/v1"),
        2,
        &[("claude-sonnet-4", "claude-sonnet-4-a")],
    );
    let conn_b = create_connection_and_bind(
        &office,
        &owner,
        "conn-b",
        Some("https://conn-b.example.com/v1"),
        2,
        &[("claude-sonnet-4", "claude-sonnet-4-b")],
    );
    let profile_id = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: profile_id.clone(),
                key: "pool-claude".into(),
                name: "Claude Pool".into(),
                description: "Ordered failover pool".into(),
                connection_ids: vec![conn_a.clone(), conn_b.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent = hire_agent(
        &office,
        &owner,
        "failover-ada",
        &profile_id,
        "claude-sonnet-4",
    );

    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-failover-1".into(),
            employee_id: agent.clone(),
            instruction: Some("Deliver status report".into()),
            workspace: None,
        },
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(6), || {
            mgr.inspect_session(&owner, Some(&agent))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("ok-from:https://conn-b.example.com/v1")
        }),
        "agent should automatically fail over from conn-a to conn-b; endpoints={:?}, inspect={:?}",
        recorded.observed_endpoints.lock().unwrap(),
        mgr.inspect_session(&owner, Some(&agent)).ok()
    );

    let endpoints = recorded.observed_endpoints.lock().unwrap().clone();
    assert_eq!(
        endpoints,
        vec![
            "https://conn-a.example.com/v1".to_owned(),
            "https://conn-b.example.com/v1".to_owned(),
        ]
    );
    let inspect = mgr.inspect_session(&owner, Some(&agent)).unwrap();
    assert!(
        inspect.spec_hash.contains(":route="),
        "spec_hash must bind connection route digest: {}",
        inspect.spec_hash
    );

    let quota = office
        .provider_quota_status(
            &owner,
            ProviderQuotaStatusParams {
                connection_id: Some(conn_a.clone()),
            },
        )
        .unwrap()
        .unwrap();
    assert!(
        quota
            .cooldowns
            .iter()
            .any(|c| c.connection_id == conn_a && c.status == ConnectionQuotaStatus::Cooldown),
        "conn-a must enter cooldown after rate_limited failover: {quota:?}"
    );
    assert!(
        wait_until(Duration::from_secs(2), || {
            office
                .provider_quota_status(
                    &owner,
                    ProviderQuotaStatusParams {
                        connection_id: None,
                    },
                )
                .unwrap()
                .unwrap()
                .active_permits
                .is_empty()
        }),
        "idle agent must hold 0 active permits after failover turn finishes"
    );

    mgr.stop_all("test-end");
}

#[test]
fn l4_failover_after_tool_side_effect_enters_recovery_required_and_never_replays() {
    let script = r#"#!/bin/sh
echo "{\"kind\":\"ready\",\"endpoint\":\"$ANTHROPIC_BASE_URL\"}"
while IFS= read -r line; do
  echo '{"kind":"tool_start"}'
  echo '{"kind":"rate_limit_fail"}'
done
"#;
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(script);
    let conn_a = create_connection_and_bind(
        &office,
        &owner,
        "conn-se-a",
        Some("https://se-a.example.com/v1"),
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let conn_b = create_connection_and_bind(
        &office,
        &owner,
        "conn-se-b",
        Some("https://se-b.example.com/v1"),
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let profile_id = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: profile_id.clone(),
                key: "pool-se".into(),
                name: "Side-Effect Pool".into(),
                description: "Must not replay after side effects".into(),
                connection_ids: vec![conn_a, conn_b],
            },
        )
        .unwrap()
        .unwrap();
    let agent = hire_agent(&office, &owner, "se-ada", &profile_id, "claude-sonnet-4");

    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-se-1".into(),
            employee_id: agent.clone(),
            instruction: Some("Mutate state".into()),
            workspace: None,
        },
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(5), || {
            mgr.inspect_session(&owner, Some(&agent))
                .ok()
                .is_some_and(|r| {
                    r.session.recovery_required && r.session.state == SessionState::RecoveryRequired
                })
        }),
        "rate_limit after tool side-effect must enter RecoveryRequired: {:?}",
        mgr.inspect_session(&owner, Some(&agent)).ok()
    );
    // Give any rogue failover attempt a moment, then prove conn_b was NEVER launched!
    std::thread::sleep(Duration::from_millis(200));
    let endpoints = recorded.observed_endpoints.lock().unwrap().clone();
    assert_eq!(
        endpoints,
        vec!["https://se-a.example.com/v1".to_owned()],
        "must never replay on conn-b after external tool side effects"
    );
    let quota = office
        .provider_quota_status(
            &owner,
            ProviderQuotaStatusParams {
                connection_id: None,
            },
        )
        .unwrap()
        .unwrap();
    assert!(quota.active_permits.is_empty());

    mgr.stop_all("test-end");
}

#[test]
fn ask4_and_live2_single_slot_connection_hands_permit_from_suspended_a_to_b_and_resumes_a() {
    // Gate file lets the test control exactly when Agent A finishes its first turn!
    let tmp_gate = tempfile::tempdir().unwrap();
    let gate_file = tmp_gate.path().join("release_a");
    let script = format!(
        r#"#!/bin/sh
echo '{{"kind":"ready","endpoint":"solo"}}'
while IFS= read -r line; do
  case "$line" in
    *HOLD_UNTIL_GATE*)
      while [ ! -f "{gate}" ]; do sleep 0.05; done
      echo '{{"kind":"reply","text":"a-asked-b"}}'
      ;;
    *)
      echo '{{"kind":"reply","text":"done-turn"}}'
      ;;
  esac
done
"#,
        gate = gate_file.display()
    );
    let (_tmp, office, mgr, _recorded, owner) = setup_office_and_manager(&script);
    let conn_solo = create_connection_and_bind(
        &office,
        &owner,
        "conn-solo",
        Some("https://solo.example.com/v1"),
        1, // max_concurrent_turns = 1 (`LIVE-2`)
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let profile_id = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: profile_id.clone(),
                key: "solo-pool".into(),
                name: "Single-Slot Pool".into(),
                description: "One concurrent turn".into(),
                connection_ids: vec![conn_solo.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent_a = hire_agent(&office, &owner, "agent-a", &profile_id, "claude-sonnet-4");
    let agent_b = hire_agent(&office, &owner, "agent-b", &profile_id, "claude-sonnet-4");

    // 1. Agent A starts and holds the sole turn permit on `conn_solo`.
    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-a-1".into(),
            employee_id: agent_a.clone(),
            instruction: Some("HOLD_UNTIL_GATE ask B for approval".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        mgr.inspect_session(&owner, Some(&agent_a))
            .ok()
            .is_some_and(|r| r.session.state == SessionState::Running)
    }));
    let q_while_a = office
        .provider_quota_status(
            &owner,
            ProviderQuotaStatusParams {
                connection_id: Some(conn_solo.clone()),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(q_while_a.active_permits.len(), 1);
    assert_eq!(q_while_a.active_permits[0].employee_id, agent_a);

    // 2. Agent A opens a required ask directed to Agent B, and Agent B receives the wake while A holds the sole slot.
    mgr.on_required_ask_opened(
        &agent_a,
        SuspendedAskEntry {
            ask_id: "ask-live2-1".into(),
            conversation_id: "conv-1".into(),
            waiting_for_member_id: Some(agent_b.clone()),
            waiting_for_name: Some("Agent agent-b".into()),
            question_summary: "Can we deploy?".into(),
            opened_at_ms: now_ms(),
            asked_by_subagent: false,
        },
    );
    let wake_b = mgr.enqueue_wake(
        &agent_b,
        PendingWakeItem {
            item_id: "ask:ask-live2-1".into(),
            kind: WakeKind::Ask,
            sender_id: agent_a.clone(),
            sender_name: "Agent agent-a".into(),
            sender_is_person: false,
            conversation_id: Some("conv-1".into()),
            source_id: "ask-live2-1".into(),
            summary: "Can we deploy?".into(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(
        wake_b,
        WakeOutcome::Queued {
            reason: WakeHoldReason::AdmissionRefused
        },
        "B must queue while A holds the sole connection slot (`LIVE-2`)"
    );

    // 3. Release gate so A finishes its turn and suspends (`ASK-4`):
    //    - A releases its permit (0 permits held by A) and shows WaitingForAnswer naming B
    //    - B automatically wakes, acquires the freed slot, and completes its turn (`LIVE-2`)!
    fs::write(&gate_file, "go").unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || {
            let a_susp = mgr
                .member_runtime_presence(&agent_a)
                .is_some_and(|rt| {
                    rt.presence == MemberPresence::WaitingForAnswer
                        && rt.status.as_deref().unwrap_or("").contains("Agent agent-b")
                });
            let b_done = mgr
                .inspect_session(&owner, Some(&agent_b))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("done-turn");
            a_susp && b_done
        }),
        "A must suspend (ASK-4) and B must automatically run on the freed slot (LIVE-2): A={:?} B={:?}",
        mgr.member_runtime_presence(&agent_a),
        mgr.inspect_session(&owner, Some(&agent_b)).ok()
    );
    assert!(
        wait_until(Duration::from_secs(3), || {
            office
                .provider_quota_status(
                    &owner,
                    ProviderQuotaStatusParams {
                        connection_id: Some(conn_solo.clone()),
                    },
                )
                .unwrap()
                .unwrap()
                .active_permits
                .is_empty()
        }),
        "after A suspends and B finishes, 0 permits must remain active (`ASK-4`)"
    );

    // 4. B answers A's required ask -> A automatically resumes on `conn_solo` (`LIVE-2`)!
    let resumed = mgr.on_ask_answered(
        &agent_a,
        "ask-live2-1",
        "Can we deploy?",
        "Yes, ship it!",
        &agent_b,
        "Agent agent-b",
        false,
        false,
    );
    assert_eq!(resumed, WakeOutcome::Started);
    assert!(
        wait_until(Duration::from_secs(5), || {
            mgr.inspect_session(&owner, Some(&agent_a))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("done-turn")
        }),
        "A must resume and complete after B answers (`LIVE-2`): {:?}",
        mgr.inspect_session(&owner, Some(&agent_a)).ok()
    );

    mgr.stop_all("test-end");
}
