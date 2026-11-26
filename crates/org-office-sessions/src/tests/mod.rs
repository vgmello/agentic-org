//! Comprehensive C3 unit & real-process integration tests covering:
//! - `AC #1`: Real-process agent session in private `0700` home + HR lead proposal drafting
//! - `SUB-1..5`, `SUB-7`: office-owned sub-agents, tool subsetting, descendant binding, usage, reaping
//! - `LIVE-9..13`: `pending-start`, `active-turn`, restart continuation, 3-resume cap, 3-stall lock
//! - `INSTR-4, 5, 7, 8, 12, 15, 21`: <= 10 KiB built-in rules, live catalogue verification, authority
//! - `ENROLL-1..7`: CLI enrollment, single active lock, expiry & `0700` wipe, wrapped vault, redaction, presets

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    encode_line, AgentRuntime, ApprovalId, ApprovalKind, ApprovalRequest, Capabilities, Control,
    ControlError, CostBasis, Escalation, ExitInfo, LaunchContext, LaunchPlan, Output,
    PermissionMode, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun, RuntimeId,
    Session, SessionEvent, SessionId, SessionSpec, SpecPath, Stdio, StopPlan, StopSignal, StopStep,
    ToolName, TurnOutcome, Unavailable, Usage, UsageScope, VersionRange,
};
use org_office::organization_family::records::PermissionPreset;
use org_office::{Office, TestOffice};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::sessions::{
    EnrollmentState, MemberSessionStartParams, ProviderEnrollmentStart,
    ProviderEnrollmentStartParams, SessionState,
};
use org_proto::Request;

use crate::binding::{BindingRejection, RunBindingRegistry};
use crate::credentials::{
    redact_secrets, validate_pasted_code, CredentialVault, EnrollmentDriver, EnrollmentManager,
    EnrollmentOutcome, EnrollmentPrompt, EnrollmentVaultError,
};
use crate::instructions::{
    assemble_standing_instructions, compose_turn_objective, verify_command_references,
    InstructionInput, TurnTrigger, BUILTIN_BEHAVIORAL_RULES, BUILTIN_INSTRUCTIONS_MAX_BYTES,
    HR_LEAD_GUIDANCE, SAME_USER_ISOLATION_NOTICE,
};
use crate::manager::{SessionManager, SessionManagerConfig};
use crate::preset::{
    build_grants_summary, build_session_policy, decide_escalation, decide_escalation_with_tools,
    parse_preset, EscalationDisposition,
};
use crate::recovery::{
    ActiveTurnRecord, PendingStartRecord, ProcessExitOracle, RecoveryAction, SessionStore,
    StallWatchdog,
};
use crate::runtime_registry::RuntimeRegistry;
use crate::subagents::{
    build_general_subagent_definition, subset_tools, write_subagent_definition, BackgroundKind,
    BackgroundRegistry,
};

mod approval;
mod approval_bridge;
mod bwrap;
mod enrollment;
mod instructions;
mod launch;
mod pool;
mod presets;
mod real_process;
mod recovery;
mod recovery_clear;
mod sandbox;
mod subagents_binding;
mod subscription_usage;
mod wake;
mod warm;

// ---------------------------------------------------------------------------
// ENROLL-1..6: CLI enrollment, wrapped credential vault, redaction
// ---------------------------------------------------------------------------

struct FakeEnrollmentDriver {
    secret: String,
}

impl EnrollmentDriver for FakeEnrollmentDriver {
    fn start(
        &self,
        _provider: &str,
        temp_dir: &Path,
    ) -> Result<EnrollmentPrompt, EnrollmentVaultError> {
        fs::write(temp_dir.join("scratch.txt"), b"ephemeral").unwrap();
        Ok(EnrollmentPrompt {
            state: EnrollmentState::AwaitingCode,
            auth_url: Some("https://example.test/oauth".into()),
            device_code: Some("ABCD-1234".into()),
            message: Some("Paste code".into()),
        })
    }

    fn submit_code(
        &self,
        _provider: &str,
        _temp_dir: &Path,
        code: &str,
    ) -> Result<EnrollmentOutcome, EnrollmentVaultError> {
        validate_pasted_code(code)?;
        Ok(EnrollmentOutcome::Completed {
            secret: zeroize::Zeroizing::new(self.secret.as_bytes().to_vec()),
            expires_at: Some("2027-01-01T00:00:00.000Z".into()),
            account_hint: Some("user@example.test".into()),
        })
    }

    fn poll(
        &self,
        _provider: &str,
        _temp_dir: &Path,
    ) -> Result<EnrollmentOutcome, EnrollmentVaultError> {
        Ok(EnrollmentOutcome::Pending {
            state: EnrollmentState::AwaitingCode,
            message: None,
        })
    }
}

// ---------------------------------------------------------------------------
// LIVE-9..13: Restart recovery, 3-attempt cap, unprovable exit, 3-stall lock
// ---------------------------------------------------------------------------

struct FixedOracle(Result<bool, io::ErrorKind>);

impl ProcessExitOracle for FixedOracle {
    fn prove_exited(
        &self,
        _mark: &str,
        _pid: Option<u32>,
        _started_identity: Option<&str>,
        _boot: Option<&str>,
    ) -> io::Result<bool> {
        match self.0 {
            Ok(v) => Ok(v),
            Err(kind) => Err(io::Error::new(kind, "oracle failure")),
        }
    }
}

// ---------------------------------------------------------------------------
// AC #1: Real supervised process execution in private 0700 home via SessionManager
// ---------------------------------------------------------------------------

struct ScriptedRuntime;
struct ScriptedSession {
    ready: bool,
    closed: bool,
}

impl Session for ScriptedSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(val) = serde_json::from_slice::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match val.get("kind").and_then(|v| v.as_str()) {
            Some("ready") => {
                self.ready = true;
                vec![
                    Output::Event(SessionEvent::SessionStarted {
                        runtime_session_id: SessionId::try_from("rs-scripted-1").unwrap(),
                    }),
                    Output::Event(SessionEvent::Ready {
                        runtime_version: Some("1.0.0".into()),
                        pid: None,
                        agents: vec!["ada".into()],
                        permission_mode: Some("default".into()),
                    }),
                ]
            }
            Some("reply") => {
                let text = val["text"].as_str().unwrap_or("").to_owned();
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

    fn on_truncated(&mut self, len: usize, _now: Instant) -> Vec<Output> {
        vec![Output::Event(SessionEvent::Truncated { len })]
    }

    fn on_stderr_line(&mut self, _line: &[u8], _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_stdout_closed(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_exit(&mut self, exit: ExitInfo, _now: Instant) -> Vec<Output> {
        if self.closed {
            return Vec::new();
        }
        self.closed = true;
        vec![Output::Event(SessionEvent::Ended { exit })]
    }

    fn on_tick(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn control(&mut self, control: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        if self.closed {
            return Err(ControlError::Closed);
        }
        match control {
            Control::Send { text, .. } => {
                let line = encode_line(&serde_json::json!({ "turn": text }))
                    .map_err(|_| ControlError::Unsupported)?;
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

impl AgentRuntime for ScriptedRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }

    fn binary_name(&self) -> &'static str {
        "scripted-agent"
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
        Ok((
            LaunchPlan {
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
                            target: org_agent_runtime::SignalTarget::Group,
                            grace_ms: 200,
                        },
                    ],
                },
                cleanup_after_exit: Vec::new(),
                runtime_session_id: None,
            },
            Box::new(ScriptedSession {
                ready: false,
                closed: false,
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

fn new_uuid_v7() -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
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

fn hire_agent_with_runtime(
    office: &Office,
    owner_id: &str,
    root_team_id: &str,
    provider_type: &str,
    runtime_driver: &str,
    handle: &str,
    model_id: &str,
) -> (String, String) {
    let conn_id = new_uuid_v7();
    let prof_id = new_uuid_v7();
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id.clone(),
            default_profile_id: prof_id.clone(),
            key: format!("{handle}-conn"),
            name: format!("{handle} conn"),
            provider_type: provider_type.into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: runtime_driver.into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    let resp = office
        .dispatch_hiring_rpc(Some(owner_id), &conn_req)
        .unwrap();
    assert!(matches!(resp.outcome, org_proto::Outcome::Result(_)));
    bind_and_refresh_test_connection(office, owner_id, &conn_id, model_id);

    let hire_req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: new_uuid_v7(),
            home_team_id: root_team_id.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: format!("Agent {handle}"),
                handle: handle.into(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner_id.into()),
                office_id: None,
                model_profile_id: Some(prof_id),
                model_id: Some(model_id.into()),
                permission_preset: Some("workspace_edits".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: Some("Answer directly.".into()),
            },
            reason: "Runtime E2E".into(),
        },
    )
    .unwrap();
    let hired_resp = office
        .dispatch_hiring_rpc(Some(owner_id), &hire_req)
        .unwrap();
    match hired_resp.outcome {
        org_proto::Outcome::Result(v) => {
            let hired: org_proto::hiring::HireOpResult = serde_json::from_value(v).unwrap();
            (hired.employee_id.expect("employee_id"), conn_id)
        }
        org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
    }
}

fn wait_for_reply(manager: &SessionManager, owner_id: &str, agent_id: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let inspected = manager.inspect_session(owner_id, Some(agent_id)).unwrap();
        if let Some(reply) = inspected.session.last_reply {
            return reply;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for reply from {agent_id}");
}

fn write_fake_cli(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn seed_project_task_wake_fixture(
    office: &Arc<Office>,
    owner_id: &str,
    root_team_id: &str,
) -> (String, String, String, String) {
    use org_office::rules::OrgCommand;
    use org_proto::org::{ProjectCreateParams, TeamCreateParams};
    use org_proto::processes::{
        DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
    };
    use org_proto::tasks::TasksCreateParams;

    let owning_team = new_uuid_v7();
    office
        .submit_org(
            owner_id,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: new_uuid_v7(),
                team_id: owning_team.clone(),
                name: "Payments".into(),
                owner_team_id: Some(root_team_id.to_owned()),
            }),
        )
        .unwrap()
        .unwrap();

    let contrib_team = new_uuid_v7();
    office
        .submit_org(
            owner_id,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: new_uuid_v7(),
                team_id: contrib_team.clone(),
                name: "QA Guild".into(),
                owner_team_id: Some(root_team_id.to_owned()),
            }),
        )
        .unwrap()
        .unwrap();

    let project_id = new_uuid_v7();
    office
        .submit_org(
            owner_id,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: new_uuid_v7(),
                project_id: project_id.clone(),
                name: "Ledger".into(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: Some("ledger".into()),
                owner_team_id: Some(owning_team.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: vec![contrib_team.clone()],
            }),
        )
        .unwrap()
        .unwrap();

    let (contrib_agent_id, conn_id) = hire_agent_with_runtime(
        office,
        owner_id,
        &contrib_team,
        "google",
        "agy",
        "qa-bot",
        "gemini-2.5-pro",
    );

    for (scope, key, title, body) in [
        (
            ProcessScopeWire::Team {
                team_id: owning_team,
            },
            "payments-audit",
            "Payments Audit Rule",
            "OWNING_TEAM_MANDATORY_SENTINEL",
        ),
        (
            ProcessScopeWire::Team {
                team_id: contrib_team,
            },
            "qa-only",
            "QA Guild Rule",
            "CONTRIB_TEAM_MUST_NOT_APPEAR",
        ),
        (
            ProcessScopeWire::Project {
                project_id: project_id.clone(),
            },
            "ledger-invariants",
            "Ledger Invariants",
            "PROJECT_MANDATORY_SENTINEL",
        ),
    ] {
        office
            .process_write(
                owner_id,
                ProcessWriteParams {
                    command_id: new_uuid_v7(),
                    document_id: None,
                    kind: DocumentKindWire::Standard,
                    scope,
                    key: Some(key.into()),
                    title: title.into(),
                    status: DocumentStatusWire::Current,
                    owner_employee_id: Some(owner_id.to_owned()),
                    overridable: false,
                    body_markdown: body.into(),
                    expected_revision: None,
                },
            )
            .unwrap()
            .unwrap();
    }

    let task = office
        .task_create(
            owner_id,
            TasksCreateParams {
                command_id: new_uuid_v7(),
                task_id: new_uuid_v7(),
                team_id: None,
                project_id: Some(project_id),
                assigned_team_id: None,
                assignee_id: Some(contrib_agent_id.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Audit ledger entries".into(),
                body_markdown: "Check double-entry invariant.".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
            },
        )
        .unwrap()
        .unwrap()
        .task;
    (contrib_agent_id, conn_id, task.id, task.title)
}

fn bind_and_refresh_test_connection(
    office: &Office,
    owner_id: &str,
    conn_id: &str,
    model_id: &str,
) {
    office
        .provider_binding_set(
            owner_id,
            org_proto::providers::ProviderBindingSetParams {
                operation_id: new_uuid_v7(),
                connection_id: conn_id.to_owned(),
                expected_revision: None,
                auth_context_ref: format!("vault://{conn_id}"),
                runtime_binary_path: None,
                account_identity_ref: None,
            },
        )
        .unwrap()
        .unwrap();
    office
        .provider_connection_refresh(
            owner_id,
            org_proto::providers::ProviderConnectionRefreshParams {
                connection_id: conn_id.to_owned(),
                driver_version: Some("1.0.0".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: None,
                operator_attested_subscription: false,
                models: vec![org_proto::providers::DiscoveredModelInput {
                    model_id: model_id.to_owned(),
                    driver_model_id: model_id.to_owned(),
                    entitlement: org_proto::providers::DiscoveredModelEntitlement::Confirmed,
                    capabilities_json: "{}".into(),
                }],
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();
}

fn spawn_stub_bwrap_handle(
    office_id: &str,
    emp_id: &str,
    state_kind: SessionState,
    masked: std::collections::BTreeSet<String>,
) -> (
    crate::manager::LiveSessionHandle,
    Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::{AtomicBool, Ordering};

    let (tx, rx) = std::sync::mpsc::channel::<crate::pump::PumpCommand>();
    let stopped = Arc::new(AtomicBool::new(false));
    let stopped_c = Arc::clone(&stopped);
    let worker = std::thread::spawn(move || {
        while let Ok(cmd) = rx.recv() {
            if matches!(cmd, crate::pump::PumpCommand::Stop { .. }) {
                stopped_c.store(true, Ordering::SeqCst);
                break;
            }
        }
    });
    let state = Arc::new(Mutex::new(crate::pump::SessionRuntimeState {
        employee_id: emp_id.into(),
        office_id: office_id.into(),
        state: state_kind,
        run_id: Some("run-a".into()),
        session_id: Some("sess-a".into()),
        runtime_id: Some("claude".into()),
        runtime_session_id: None,
        private_home: "/tmp/home-a".into(),
        permission_preset: "autonomous_sandboxed".into(),
        sandbox_enabled: true,
        spec_hash: "h".into(),
        watchdog: crate::recovery::StallWatchdog::new(Duration::from_secs(60), Instant::now()),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: crate::subagents::BackgroundRegistry::default(),
        pending_approvals: Vec::new(),
        policy: org_agent_runtime::PermissionPolicy {
            version: 1,
            allow: Vec::new(),
            deny: Vec::new(),
        },
        preset: PermissionPreset::AutonomousSandboxed,
        allowed_tools: None,
        writable_roots: Vec::new(),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
    }));
    (
        crate::manager::LiveSessionHandle {
            state,
            tx,
            thread: Some(worker),
            connection_id: None,
            route_digest: String::new(),
            oauth_expires_at_ms: None,
            workspace: None,
            active_permit: Arc::new(Mutex::new(None)),
            supports_mid_turn: false,
            bwrap_masked_siblings: Some(masked),
        },
        stopped,
    )
}

fn spawn_panicking_bwrap_handle(
    office_id: &str,
    emp_id: &str,
    masked: std::collections::BTreeSet<String>,
) -> crate::manager::LiveSessionHandle {
    let (mut handle, _) = spawn_stub_bwrap_handle(office_id, emp_id, SessionState::Idle, masked);
    let (ptx, prx) = std::sync::mpsc::channel::<crate::pump::PumpCommand>();
    let panicking = std::thread::spawn(move || {
        let _ = prx.recv();
        panic!("simulated unverified teardown");
    });
    let _ = handle.tx.send(crate::pump::PumpCommand::Stop {
        reason: "test_cleanup".into(),
    });
    if let Some(t) = handle.thread.take() {
        let _ = t.join();
    }
    handle.tx = ptx;
    handle.thread = Some(panicking);
    handle
}
