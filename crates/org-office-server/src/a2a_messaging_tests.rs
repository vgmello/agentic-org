//! Real-socket, real-mTLS, real-`SessionManager`, and real-`agent-os` CLI end-to-end tests for
//! same-office and cross-office Agent-to-Agent messaging (`TASK-117`).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_agent_cli::office::Binding;
use org_agent_runtime::{
    encode_line, AgentRuntime, Capabilities, Control, ControlError, CostBasis, ExitInfo,
    LaunchContext, LaunchPlan, Output, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult,
    ProbeRun, RuntimeId, Session, SessionEvent, SessionId, SessionSpec, SignalTarget, Stdio,
    StopPlan, StopSignal, StopStep, TurnOutcome, Unavailable, UsageScope, VersionRange,
};
use org_office::{
    CreateInvitationParams, Custody, DataRoots, EngineChoice, Key, NewOffice, Office, OfficeRole,
    SatelliteProvisionSpec, TestOffice,
};
use org_office_sessions::{RuntimeRegistry, SessionManager, SessionManagerConfig};
use org_pki::{invitation::InvitationToken, SigningKey, Zeroizing};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::{Outcome, Request};
use org_store::{testkit::FakeCustody, Id, OfficeIdentity, WriterAuthority};
use serde_json::{json, Value};

use crate::federation::{enroll_satellite_with_hq, HqFederationServer, SatelliteSyncSession};
use crate::{
    build_federation_delivery_notifier, build_federation_wake_handler, build_messaging_ports,
    build_office_ports, inbox, live, org_feed, serve, start_optional_satellite_sync, SatSyncWake,
};

const ECHO_RUNTIME_SCRIPT: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  printf '{"kind":"reply","text":"ack"}\n'
done
"#;

#[derive(Debug, Clone)]
pub(super) struct RecordedTurn {
    pub(super) agent_id: String,
    pub(super) prompt: String,
}

#[derive(Default)]
pub(super) struct RecordedTurns {
    turns: Mutex<Vec<RecordedTurn>>,
}

impl RecordedTurns {
    pub(super) fn snapshot(&self) -> Vec<RecordedTurn> {
        self.turns.lock().unwrap().clone()
    }

    pub(super) fn count_for(&self, agent_id: &str) -> usize {
        self.turns
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.agent_id == agent_id)
            .count()
    }

    pub(super) fn wait_for_agent(
        &self,
        agent_id: &str,
        min_turns: usize,
        timeout: Duration,
    ) -> RecordedTurn {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let guard = self.turns.lock().unwrap();
                let matches: Vec<&RecordedTurn> =
                    guard.iter().filter(|t| t.agent_id == agent_id).collect();
                if matches.len() >= min_turns {
                    return matches[min_turns - 1].clone();
                }
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {min_turns} turn(s) for agent {agent_id}; recorded={:?}",
                self.snapshot()
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}

struct RecordingSession {
    agent_handle: String,
    recorded: Arc<RecordedTurns>,
    runtime_session_id: String,
}

impl Session for RecordingSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            return Vec::new();
        };
        match v.get("kind").and_then(Value::as_str) {
            Some("ready") => vec![
                Output::Event(SessionEvent::SessionStarted {
                    runtime_session_id: SessionId::try_from(self.runtime_session_id.as_str())
                        .unwrap(),
                }),
                Output::Event(SessionEvent::Ready {
                    runtime_version: Some("1.0.0".into()),
                    pid: None,
                    agents: vec![self.agent_handle.clone()],
                    permission_mode: Some("default".into()),
                }),
            ],
            Some("reply") => vec![
                Output::Event(SessionEvent::AssistantText {
                    message_id: Some("m1".into()),
                    text: v["text"].as_str().unwrap_or("ack").to_owned(),
                }),
                Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Completed,
                    errors: Vec::new(),
                }),
            ],
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
                self.recorded.turns.lock().unwrap().push(RecordedTurn {
                    agent_id: self.agent_handle.clone(),
                    prompt: text.clone(),
                });
                let line =
                    encode_line(&json!({ "text": text })).map_err(|_| ControlError::Closed)?;
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

struct RecordingRuntime {
    recorded: Arc<RecordedTurns>,
}

impl AgentRuntime for RecordingRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }

    fn binary_name(&self) -> &'static str {
        "claude"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mid_turn_delivery: org_agent_runtime::MidTurnDelivery::None,
            interrupt_in_band: true,
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
            .unwrap_or_else(|| format!("a2a-sess-{}", spec.agent.name.as_str()));
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
            Box::new(RecordingSession {
                agent_handle: spec.agent.name.as_str().to_owned(),
                recorded: Arc::clone(&self.recorded),
                runtime_session_id: rsid,
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

pub(super) struct LiveOfficeNode {
    _sock_dir: tempfile::TempDir,
    _bin_dir: tempfile::TempDir,
    pub(super) socket: PathBuf,
    pub(super) office: Arc<Office>,
    pub(super) sessions: Arc<SessionManager>,
    pub(super) recorded: Arc<RecordedTurns>,
    sat_wake: Arc<SatSyncWake>,
    stop: Arc<AtomicBool>,
    hq_fed: Option<HqFederationServer>,
    sat_thread: Option<thread::JoinHandle<()>>,
    wake_thread: Option<thread::JoinHandle<()>>,
    serve_thread: Option<thread::JoinHandle<()>>,
}

impl LiveOfficeNode {
    pub(super) fn start(office: Arc<Office>, start_hq_fed: bool, start_sat_sync: bool) -> Self {
        let _ = org_platform::process::harden_self();
        let sock_dir = tempfile::Builder::new().prefix("a2a").tempdir().unwrap();
        let bin_dir = tempfile::Builder::new().prefix("a2ab").tempdir().unwrap();
        let script_path = bin_dir.path().join("fake-claude");
        fs::write(&script_path, ECHO_RUNTIME_SCRIPT).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

        let socket = sock_dir.path().join("office.sock");
        let recorded = Arc::new(RecordedTurns::default());
        let mut reg = RuntimeRegistry::production();
        reg.register(
            Arc::new(RecordingRuntime {
                recorded: Arc::clone(&recorded),
            }),
            Some(script_path),
        );
        let mut cfg = SessionManagerConfig::production(socket.clone());
        cfg.runtimes = reg;
        let sessions = Arc::new(SessionManager::new(Arc::clone(&office), cfg));
        SessionManager::attach_self(&sessions);

        let wake = Arc::new(org_feed::FeedWake::new());
        let live_wake = Arc::clone(&wake);
        let live = Arc::new(live::LiveEvents::for_owner(
            Box::new(move || live_wake.notify()),
            office.owner_member_id().to_owned(),
        ));
        let channel_watcher = Arc::new(crate::watcher::ChannelWatcher::production(
            Arc::clone(&office),
            Some(Arc::clone(&sessions)),
        ));
        let sat_wake = Arc::new(SatSyncWake::default());
        let messaging = build_messaging_ports(
            &office,
            &live,
            &wake,
            &sessions,
            &channel_watcher,
            Arc::clone(&sat_wake),
        );
        let wake_handler = build_federation_wake_handler(&sessions, &messaging.chat_svc);
        let delivery_notifier = build_federation_delivery_notifier(
            office.owner_member_id().to_owned(),
            &messaging.chat_svc,
            &messaging.mailbox,
            &live,
            &channel_watcher,
        );
        let hq_fed = if start_hq_fed {
            Some(
                HqFederationServer::start_with_notifier(
                    Arc::clone(&office),
                    "127.0.0.1:0".parse().unwrap(),
                    &["127.0.0.1".into()],
                    Some(Arc::clone(&wake_handler)),
                    Some(Arc::clone(&delivery_notifier)),
                )
                .unwrap(),
            )
        } else {
            None
        };
        let stop = Arc::new(AtomicBool::new(false));
        let sat_thread = if start_sat_sync {
            start_optional_satellite_sync(
                &office,
                &stop,
                Arc::clone(&sat_wake),
                wake_handler,
                delivery_notifier,
                Arc::downgrade(&sessions),
            )
        } else {
            None
        };

        let hq_inbox = inbox::Hq::for_office(Arc::clone(&office)).unwrap();
        let ports = build_office_ports(
            Arc::clone(&office),
            Arc::clone(&sessions),
            channel_watcher,
            live,
            &wake,
            messaging,
            Arc::new(AtomicBool::new(hq_fed.is_some())),
        );
        let mut shared = serve::Shared::new(
            office.info(),
            &socket,
            ports,
            crate::test_support::grant_key(),
        );
        shared.register_inbox(hq_inbox, office.is_hq());
        let shared = Arc::new(shared);

        let listener = serve::bind(&socket).unwrap();
        let serve_shared = Arc::clone(&shared);
        let serve_stop = Arc::clone(&stop);
        let serve_thread = thread::spawn(move || {
            serve::run(&listener, &serve_shared, &|| {
                serve_stop.load(Ordering::SeqCst)
            });
        });
        let wake_thread = org_office_sessions::spawn_wake_retry_loop(
            Arc::clone(&sessions),
            Arc::clone(&stop),
            Duration::from_millis(100),
        );

        Self {
            _sock_dir: sock_dir,
            _bin_dir: bin_dir,
            socket,
            office,
            sessions,
            recorded,
            sat_wake,
            stop,
            hq_fed,
            sat_thread,
            wake_thread,
            serve_thread: Some(serve_thread),
        }
    }

    pub(super) fn hq_fed_addr(&self) -> String {
        self.hq_fed
            .as_ref()
            .expect("HQ federation server running")
            .local_addr()
            .to_string()
    }
}

struct ScopedBinding {
    sessions: Arc<SessionManager>,
    employee_id: String,
    run_id: String,
    binding: Binding,
}

impl Drop for ScopedBinding {
    fn drop(&mut self) {
        self.sessions
            .bindings()
            .revoke(&self.employee_id, Some(&self.run_id));
    }
}

impl LiveOfficeNode {
    fn issue_binding(&self, employee_id: &str) -> ScopedBinding {
        wait_until(Duration::from_secs(5), "no active runtime turn", || {
            !matches!(
                self.sessions
                    .inspect_session(employee_id, None)
                    .ok()
                    .map(|s| s.session.state),
                Some(
                    org_proto::sessions::SessionState::Starting
                        | org_proto::sessions::SessionState::Running
                        | org_proto::sessions::SessionState::WaitingApproval
                )
            )
        });
        let run_id = format!("run-a2a-{}", self.office.next_uuid_v7());
        let token = self
            .sessions
            .bindings()
            .issue(
                employee_id,
                &run_id,
                "sess-a2a-e2e",
                std::process::id() as libc::pid_t,
            )
            .unwrap()
            .expose_token()
            .to_owned();
        let binding = Binding {
            socket: Some(self.socket.clone()),
            office_id: None,
            runtime_dir: None,
            session_token: Some(token),
            agent_id: Some(employee_id.to_owned()),
        };
        ScopedBinding {
            sessions: Arc::clone(&self.sessions),
            employee_id: employee_id.to_owned(),
            run_id,
            binding,
        }
    }

    fn cli(&self, employee_id: &str, args: &[&str]) -> (u8, Value) {
        let scoped = self.issue_binding(employee_id);
        let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let out = org_agent_cli::execute(&owned, scoped.binding.clone());
        drop(scoped);
        let json = serde_json::from_str(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "invalid JSON from agent-os {args:?}: {e}\nstdout: {}",
                out.stdout
            )
        });
        (out.exit_code, json)
    }

    fn cli_ok(&self, employee_id: &str, args: &[&str]) -> Value {
        let (code, v) = self.cli(employee_id, args);
        assert_eq!(code, 0, "agent-os {args:?} failed: {v}");
        assert_eq!(v["ok"], Value::Bool(true), "agent-os {args:?} not ok: {v}");
        v["data"].clone()
    }
}

impl Drop for LiveOfficeNode {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.sat_wake.notify();
        if let Some(fed) = self.hq_fed.take() {
            fed.stop();
        }
        if let Some(h) = self.sat_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.wake_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.serve_thread.take() {
            let _ = h.join();
        }
        self.sessions.shutdown("a2a test teardown");
        let _ = fs::remove_file(&self.socket);
    }
}

pub(super) fn setup_provider_on_hq(hq: &Office, owner: &str) -> (String, String) {
    let conn_id = hq.next_uuid_v7();
    let profile_id = hq.next_uuid_v7();
    let req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: hq.next_uuid_v7(),
            connection_id: conn_id.clone(),
            default_profile_id: profile_id.clone(),
            key: "a2a-claude".into(),
            name: "A2A Claude".into(),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 8,
        },
    )
    .unwrap();
    let resp = hq.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(
        matches!(resp.outcome, Outcome::Result(_)),
        "provider create: {resp:?}"
    );
    bind_provider_on_office(hq, owner, &conn_id);
    (conn_id, profile_id)
}

pub(super) fn bind_provider_on_office(office: &Office, actor: &str, conn_id: &str) {
    office
        .provider_binding_set(
            actor,
            org_proto::providers::ProviderBindingSetParams {
                operation_id: office.next_uuid_v7(),
                connection_id: conn_id.to_owned(),
                expected_revision: None,
                auth_context_ref: "vault://a2a-claude".into(),
                runtime_binary_path: None,
                account_identity_ref: None,
            },
        )
        .unwrap()
        .unwrap();
    office
        .provider_connection_refresh(
            actor,
            org_proto::providers::ProviderConnectionRefreshParams {
                connection_id: conn_id.to_owned(),
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
}

pub(super) fn hire_agent_at(
    hq: &Office,
    owner: &str,
    team_id: &str,
    office_id: &str,
    profile_id: &str,
    name: &str,
    handle: &str,
) -> String {
    let req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: hq.next_uuid_v7(),
            request_id: hq.next_uuid_v7(),
            employee_id: hq.next_uuid_v7(),
            home_team_id: team_id.to_owned(),
            role_id: None,
            proposal: HireProposalWire {
                name: name.into(),
                handle: handle.into(),
                title: Some("Engineer".into()),
                focus: Some("Messaging".into()),
                role_text: None,
                title_in_team: None,
                reports_to_employee_id: Some(owner.to_owned()),
                office_id: Some(office_id.to_owned()),
                model_profile_id: Some(profile_id.to_owned()),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("safe_ask".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: Some("Be concise.".into()),
                body: None,
            },
            reason: "a2a test hire".into(),
        },
    )
    .unwrap();
    let resp = hq.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    match resp.outcome {
        Outcome::Result(v) => v["employee_id"].as_str().unwrap().to_owned(),
        Outcome::Error(e) => panic!("hire {handle} failed: {e:?}"),
    }
}

fn sat_custody(org_id: &str, sat_office_id: &str, inc_suffix: u8) -> Custody {
    Custody {
        identity: OfficeIdentity {
            instance_id: Id::parse(&format!(
                "0195b000-0000-7000-8000-0000000001{inc_suffix:02x}"
            ))
            .unwrap(),
            organization_id: Id::parse(org_id).unwrap(),
            office_id: Id::parse(sat_office_id).unwrap(),
            journal_incarnation: Id::parse(&format!(
                "0195b000-0000-7000-8000-0000000002{inc_suffix:02x}"
            ))
            .unwrap(),
            writer_epoch: 1,
            machine_id: Id::parse(&format!(
                "0195b000-0000-7000-8000-0000000003{inc_suffix:02x}"
            ))
            .unwrap(),
        },
        authority: Arc::new(FakeCustody::new()) as Arc<dyn WriterAuthority>,
    }
}

pub(super) fn enroll_satellite(
    hq: &Arc<Office>,
    owner: &str,
    hq_addr: &str,
    name: &str,
    inc_suffix: u8,
) -> (tempfile::TempDir, Arc<Office>) {
    let org_id = hq.info().org_id.clone();
    let created = hq
        .create_satellite_invitation(
            owner,
            CreateInvitationParams {
                office_name: name.into(),
                hq_addresses: vec![hq_addr.to_owned()],
                ttl_secs: Some(600),
                existing_office_id: None,
            },
        )
        .unwrap()
        .unwrap();
    let token = InvitationToken::decode(&created.token).unwrap();
    let sat_office_id = token.office_id.clone();
    let sat_tls_key = SigningKey::generate().unwrap();
    let sat_tls_der = sat_tls_key.local_der().to_vec();

    let (tx_code, rx_code) = std::sync::mpsc::channel::<String>();
    let token_clone = token.clone();
    let enroll_thread = thread::spawn(move || {
        enroll_satellite_with_hq(&token_clone, &sat_tls_key, move |code| {
            let _ = tx_code.send(code.to_owned());
        })
    });
    let shown_code = rx_code.recv().unwrap();
    hq.approve_satellite_invitation(owner, &token.invitation_id, &shown_code)
        .unwrap()
        .unwrap();
    let enrolled = enroll_thread.join().unwrap().unwrap();

    let sat_dir = tempfile::tempdir().unwrap();
    let sat_root = sat_dir.path().canonicalize().unwrap();
    let sat_folder = sat_root.join("home/offices").join(&sat_office_id);
    let sat_roots = DataRoots {
        durable: sat_root.join("data").join(&org_id).join("durable"),
        runtime: sat_root.join("data").join(&org_id).join("runtime"),
    };
    fs::create_dir_all(&sat_roots.durable).unwrap();
    fs::create_dir_all(&sat_roots.runtime).unwrap();
    let sat = Arc::new(
        Office::provision_satellite(
            sat_folder,
            SatelliteProvisionSpec {
                new: NewOffice {
                    office_id: sat_office_id.clone(),
                    office_name: name.into(),
                    role: OfficeRole::Satellite,
                    org_id: org_id.clone(),
                    org_name: hq.info().org_name.clone(),
                },
                operation_id: hq.next_uuid_v7(),
                hq_office_id: enrolled.hq_office_id,
                hq_addresses: vec![hq_addr.to_owned()],
                epoch: enrolled.epoch,
                ca_cert_der: enrolled.ca_cert_der,
                office_cert_der: enrolled.cert_der,
                office_key_der: Zeroizing::new(sat_tls_der),
            },
            Key::from_bytes([0x33 + inc_suffix; 32]),
            sat_roots,
            sat_custody(&org_id, &sat_office_id, inc_suffix),
        )
        .unwrap(),
    );
    let mut sess = SatelliteSyncSession::connect(Arc::clone(&sat)).unwrap();
    sess.sync_initial_snapshot(&hq.next_uuid_v7()).unwrap();
    (sat_dir, sat)
}

pub(super) fn wait_until<F>(timeout: Duration, desc: &str, mut predicate: F)
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(40));
    }
    panic!("timed out waiting for: {desc}");
}

#[test]
fn same_office_a2a_dm_reply_channel_mentions_ask_and_handoff() {
    let office = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = office.owner_member_id().to_owned();
    let team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let hq_id = office.info().office_id.clone();
    let (_conn_id, profile_id) = setup_provider_on_hq(&office, &owner);
    let ada = hire_agent_at(&office, &owner, &team_id, &hq_id, &profile_id, "Ada", "ada");
    let ben = hire_agent_at(&office, &owner, &team_id, &hq_id, &profile_id, "Ben", "ben");

    let node = LiveOfficeNode::start(Arc::clone(&office), false, false);

    // 1. DM send -> Ben wakes -> Ben reads & replies via `agent-os msg reply --body` -> Ada wakes.
    let sent = node.cli_ok(&ada, &["msg", "send", "--to", "ben", "--body", "hello ben"]);
    let dm_id = sent["id"].as_str().unwrap().to_owned();
    let ben_turn_1 = node
        .recorded
        .wait_for_agent("ben", 1, Duration::from_secs(10));
    assert!(
        ben_turn_1.prompt.contains("hello ben"),
        "prompt={}",
        ben_turn_1.prompt
    );

    let ben_inbox = node.cli_ok(&ben, &["msg", "inbox"]);
    assert!(
        ben_inbox
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == dm_id),
        "Ben inbox missing DM: {ben_inbox}"
    );
    let read_msg = node.cli_ok(&ben, &["msg", "read", "--id", &dm_id]);
    assert_eq!(read_msg["body"], "hello ben");

    let reply = node.cli_ok(
        &ben,
        &["msg", "reply", "--id", &dm_id, "--body", "ack from ben"],
    );
    let reply_id = reply["id"].as_str().unwrap().to_owned();
    assert_eq!(reply["thread_id"], dm_id);
    let ada_turn_1 = node
        .recorded
        .wait_for_agent("ada", 1, Duration::from_secs(10));
    assert!(
        ada_turn_1.prompt.contains("ack from ben"),
        "prompt={}",
        ada_turn_1.prompt
    );
    let ada_read = node.cli_ok(&ada, &["msg", "read", "--id", &reply_id]);
    assert_eq!(ada_read["body"], "ack from ben");

    // 2. Channel: ordinary + announcement -> 0 wakes; @ben mention -> wakes; `msg reply` stays in channel!
    let ben_before_chan = node.recorded.count_for("ben");
    let _ = node.cli_ok(
        &ada,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--body",
            "routine update",
        ],
    );
    let _ = node.cli_ok(
        &ada,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--kind",
            "announcement",
            "--reason",
            "FYI",
            "--body",
            "planned maintenance",
        ],
    );
    thread::sleep(Duration::from_millis(250));
    assert_eq!(
        node.recorded.count_for("ben"),
        ben_before_chan,
        "ordinary channel post and announcement must not wake Ben"
    );

    let mention = node.cli_ok(
        &ada,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--body",
            "Hey @ben please review",
        ],
    );
    let mention_id = mention["id"].as_str().unwrap().to_owned();
    let chan_conv_id = mention["conversation_id"].as_str().unwrap().to_owned();
    let ben_turn_2 =
        node.recorded
            .wait_for_agent("ben", ben_before_chan + 1, Duration::from_secs(10));
    assert!(
        ben_turn_2.prompt.contains("please review"),
        "prompt={}",
        ben_turn_2.prompt
    );

    let chan_reply = node.cli_ok(
        &ben,
        &[
            "msg",
            "reply",
            "--id",
            &mention_id,
            "--body",
            "reviewing in channel",
        ],
    );
    let chan_reply_id = chan_reply["id"].as_str().unwrap().to_owned();
    assert_eq!(chan_reply["thread_id"], mention_id);
    let chan_msgs = node.cli_ok(&ada, &["msg", "list", "--conversation", &chan_conv_id]);
    assert!(
        chan_msgs
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == chan_reply_id && m["body"] == "reviewing in channel"),
        "channel reply must remain in channel conversation (not divert to DM): {chan_msgs}"
    );

    // 3. Structured `msg ask` (short-hold) -> Ben wakes -> `msg reply --answer` unblocks Ada immediately.
    let ben_before_ask = node.recorded.count_for("ben");
    let ask_binding = node.issue_binding(&ada);
    let ask_started = Instant::now();
    let ask_thread = thread::spawn(move || {
        let ask_json = json!({
            "to": "ben",
            "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Ship v1?" }],
            "hold_ms": 20_000
        })
        .to_string();
        let args = vec![
            "msg".to_owned(),
            "ask".to_owned(),
            "--json".to_owned(),
            ask_json,
        ];
        let out = org_agent_cli::execute(&args, ask_binding.binding.clone());
        drop(ask_binding);
        out
    });
    let _ben_ask_turn =
        node.recorded
            .wait_for_agent("ben", ben_before_ask + 1, Duration::from_secs(10));
    let mut ask_id = String::new();
    wait_until(Duration::from_secs(5), "ask in Ben inbox", || {
        let inbox = node.cli_ok(&ben, &["msg", "inbox"]);
        if let Some(item) = inbox
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["kind"] == "request")
        {
            ask_id = item["id"].as_str().unwrap().to_owned();
            return true;
        }
        false
    });
    let _ = node.cli_ok(
        &ben,
        &[
            "msg",
            "reply",
            "--id",
            &ask_id,
            "--answer",
            r#"[{"question_id":"q1","option_ids":["yes"]}]"#,
        ],
    );
    let ask_out = ask_thread.join().unwrap();
    assert_eq!(ask_out.exit_code, 0, "ask failed: {}", ask_out.stdout);
    let ask_json: Value = serde_json::from_str(&ask_out.stdout).unwrap();
    assert_eq!(ask_json["data"]["status"], "answered");
    assert!(
        ask_started.elapsed() < Duration::from_secs(6),
        "held ask should unblock promptly, took {:?}",
        ask_started.elapsed()
    );

    // 4. Handoff + `msg accept`.
    let ben_before_handoff = node.recorded.count_for("ben");
    let handoff = node.cli_ok(
        &ada,
        &[
            "msg",
            "send",
            "--to",
            "ben",
            "--kind",
            "handoff",
            "--body",
            "Own PR 42",
        ],
    );
    let handoff_id = handoff["id"].as_str().unwrap().to_owned();
    let _ = node
        .recorded
        .wait_for_agent("ben", ben_before_handoff + 1, Duration::from_secs(10));
    let accepted = node.cli_ok(
        &ben,
        &["msg", "accept", "--id", &handoff_id, "--reason", "on it"],
    );
    assert_eq!(accepted["accepted_by"], ben);
}

#[test]
fn cross_office_a2a_hq_satellite_and_sat_to_sat_end_to_end() {
    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let team_id = hq.org_snapshot().unwrap().teams[0].id.clone();
    let hq_id = hq.info().office_id.clone();
    let (conn_id, profile_id) = setup_provider_on_hq(&hq, &owner);

    let hq_node = LiveOfficeNode::start(Arc::clone(&hq), true, false);
    let hq_addr = hq_node.hq_fed_addr();

    // Enroll Satellite 1 and Satellite 2 over live Org-CA mTLS.
    let (_sat1_dir, sat1) = enroll_satellite(&hq, &owner, &hq_addr, "Porto Sat 1", 1);
    let (_sat2_dir, sat2) = enroll_satellite(&hq, &owner, &hq_addr, "Lisbon Sat 2", 2);
    let sat1_id = sat1.info().office_id.clone();
    let sat2_id = sat2.info().office_id.clone();

    // Hire Ada on HQ, Ben on Satellite 1, Cy on Satellite 2.
    let ada = hire_agent_at(&hq, &owner, &team_id, &hq_id, &profile_id, "Ada", "ada");
    let ben = hire_agent_at(&hq, &owner, &team_id, &sat1_id, &profile_id, "Ben", "ben");
    let cy = hire_agent_at(&hq, &owner, &team_id, &sat2_id, &profile_id, "Cy", "cy");

    // Verify P0-1: `org_snapshot()` reports each agent's true placed `office_id`.
    let hq_snap = hq.org_snapshot().unwrap();
    let office_of = |id: &str| {
        hq_snap
            .members
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.office_id.clone())
    };
    assert_eq!(office_of(&ada).as_deref(), Some(hq_id.as_str()));
    assert_eq!(office_of(&ben).as_deref(), Some(sat1_id.as_str()));
    assert_eq!(office_of(&cy).as_deref(), Some(sat2_id.as_str()));

    // Start live satellite nodes (background mTLS sync threads pull the hires immediately).
    // Verify satellite operator local binding succeeds and non-operator/HQ-only ops fail closed.
    let err_agent_bind = sat1
        .provider_binding_set(
            &ben,
            org_proto::providers::ProviderBindingSetParams {
                operation_id: sat1.next_uuid_v7(),
                connection_id: conn_id.clone(),
                expected_revision: None,
                auth_context_ref: "vault://nope".into(),
                runtime_binary_path: None,
                account_identity_ref: None,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(err_agent_bind.code, org_proto::ErrorCode::NotAuthorized);
    let err_sat_org_mut = sat1
        .provider_connection_update(
            sat1.owner_member_id(),
            org_proto::providers::ProviderConnectionUpdateParams {
                command_id: sat1.next_uuid_v7(),
                connection_id: conn_id.clone(),
                expected_version: 1,
                name: Some("Forbidden".into()),
                endpoint_url: None,
                clear_endpoint_url: false,
                enabled: None,
                archived: None,
                max_concurrent_turns: None,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(err_sat_org_mut.code, org_proto::ErrorCode::NotAuthorized);
    bind_provider_on_office(&sat1, sat1.owner_member_id(), &conn_id);
    bind_provider_on_office(&sat2, sat2.owner_member_id(), &conn_id);
    let sat1_node = LiveOfficeNode::start(Arc::clone(&sat1), false, true);
    let sat2_node = LiveOfficeNode::start(Arc::clone(&sat2), false, true);

    wait_until(Duration::from_secs(10), "sat1 & sat2 sync hires", || {
        sat1.org_snapshot()
            .is_ok_and(|s| s.members.iter().any(|m| m.id == cy))
            && sat2
                .org_snapshot()
                .is_ok_and(|s| s.members.iter().any(|m| m.id == cy))
    });

    // Verify P0-1 on live Unix sockets: agents bind ONLY on their placed office!
    let _ = hq_node.cli_ok(&ada, &["msg", "inbox"]);
    let _ = sat1_node.cli_ok(&ben, &["msg", "inbox"]);
    let _ = sat2_node.cli_ok(&cy, &["msg", "inbox"]);
    let (wrong_code, wrong_json) = hq_node.cli(&ben, &["msg", "inbox"]);
    assert_ne!(
        wrong_code, 0,
        "satellite agent Ben must not bind on HQ: {wrong_json}"
    );
    let (wrong_code2, wrong_json2) = sat1_node.cli(&ada, &["msg", "inbox"]);
    assert_ne!(
        wrong_code2, 0,
        "HQ agent Ada must not bind on Sat1: {wrong_json2}"
    );

    verify_cross_office_dm_and_sat_to_sat(&hq_node, &sat1_node, &sat2_node, &ada, &ben, &cy);
    verify_cross_office_channel_wakes(&hq_node, &sat1_node, &ada, &ben);
    verify_cross_office_ask_and_answer(&hq_node, &sat1_node, &ada, &ben);
    verify_cross_office_withdraw_and_handoff(&hq_node, &sat1_node, &ada, &ben);
}

fn verify_cross_office_dm_and_sat_to_sat(
    hq_node: &LiveOfficeNode,
    sat1_node: &LiveOfficeNode,
    sat2_node: &LiveOfficeNode,
    ada: &str,
    ben: &str,
    cy: &str,
) {
    // 1. HQ Ada -> Sat1 Ben DM -> Ben wakes -> Ada holds `msg wait` -> Ben replies via `msg reply --body`
    //    -> `InboundDeliveryNotifier` wakes HQ `Mailbox` (`msg wait`) AND Ada's `SessionManager`!
    let sent = hq_node.cli_ok(ada, &["msg", "send", "--to", "ben", "--body", "hq to sat1"]);
    let dm_id = sent["id"].as_str().unwrap().to_owned();
    let ben_turn = sat1_node
        .recorded
        .wait_for_agent("ben", 1, Duration::from_secs(10));
    assert!(
        ben_turn.prompt.contains("hq to sat1"),
        "prompt={}",
        ben_turn.prompt
    );
    let ben_read = sat1_node.cli_ok(ben, &["msg", "read", "--id", &dm_id]);
    assert_eq!(ben_read["body"], "hq to sat1");

    // Ada holds `msg wait` on HQ before Ben replies from Sat 1.
    let wait_binding = hq_node.issue_binding(ada);
    let wait_started = Instant::now();
    let wait_thread = thread::spawn(move || {
        let args: Vec<String> = ["msg", "wait", "--timeout-ms", "15000"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let out = org_agent_cli::execute(&args, wait_binding.binding.clone());
        drop(wait_binding);
        out
    });
    thread::sleep(Duration::from_millis(150));

    let reply = sat1_node.cli_ok(
        ben,
        &["msg", "reply", "--id", &dm_id, "--body", "sat1 reply to hq"],
    );
    let reply_id = reply["id"].as_str().unwrap().to_owned();
    let wait_out = wait_thread.join().unwrap();
    assert_eq!(
        wait_out.exit_code, 0,
        "msg wait failed: {}",
        wait_out.stdout
    );
    let wait_json: Value = serde_json::from_str(&wait_out.stdout).unwrap();
    assert_eq!(
        wait_json["data"]["id"],
        reply_id,
        "wait_json={wait_json}\nhq_ada_inbox={}\nsat1_sync={:?}\nsat1_outbox={:?}",
        hq_node.cli_ok(ada, &["msg", "inbox"]),
        sat1_node.office.sync_status(),
        sat1_node.office.replication().message_queue_snapshot()
    );
    assert!(
        wait_started.elapsed() < Duration::from_secs(5),
        "HQ msg wait should wake immediately via InboundDeliveryNotifier, took {:?}",
        wait_started.elapsed()
    );
    let ada_turn = hq_node
        .recorded
        .wait_for_agent("ada", 1, Duration::from_secs(10));
    assert!(
        ada_turn.prompt.contains("sat1 reply to hq"),
        "prompt={}",
        ada_turn.prompt
    );

    // 2. Sat 1 Ben -> Sat 2 Cy DM (store-and-forward through HQ).
    let s2s = sat1_node.cli_ok(
        ben,
        &["msg", "send", "--to", "cy", "--body", "sat1 to sat2"],
    );
    let s2s_id = s2s["id"].as_str().unwrap().to_owned();
    let cy_turn = sat2_node
        .recorded
        .wait_for_agent("cy", 1, Duration::from_secs(10));
    assert!(
        cy_turn.prompt.contains("sat1 to sat2"),
        "prompt={}",
        cy_turn.prompt
    );
    let cy_read = sat2_node.cli_ok(cy, &["msg", "read", "--id", &s2s_id]);
    assert_eq!(cy_read["body"], "sat1 to sat2");
}

fn verify_cross_office_channel_wakes(
    hq_node: &LiveOfficeNode,
    sat1_node: &LiveOfficeNode,
    ada: &str,
    ben: &str,
) {
    // 3. Cross-office channel wake fidelity: announcement + ordinary do NOT wake Ben; @ben DOES wake Ben,
    //    and a thread reply on Ben's own ordinary post only wakes Ben when `@ben` is explicitly mentioned (`LIVE-7`).
    let ben_before_chan = sat1_node.recorded.count_for("ben");
    let ann = hq_node.cli_ok(
        ada,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--kind",
            "announcement",
            "--reason",
            "ops",
            "--body",
            "global fyi",
        ],
    );
    let ann_id = ann["id"].as_str().unwrap().to_owned();
    let ordinary = hq_node.cli_ok(
        ada,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--body",
            "ordinary chatter",
        ],
    );
    let ord_id = ordinary["id"].as_str().unwrap().to_owned();
    wait_until(
        Duration::from_secs(10),
        "channel posts replicated to sat1",
        || {
            let list = sat1_node.cli_ok(ben, &["msg", "list", "--channel", "#general"]);
            let arr = list.as_array().unwrap();
            arr.iter().any(|m| m["id"] == ann_id) && arr.iter().any(|m| m["id"] == ord_id)
        },
    );
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        sat1_node.recorded.count_for("ben"),
        ben_before_chan,
        "cross-office announcement and ordinary channel post must not wake Ben"
    );

    // Ben starts an ordinary channel post; Ada replies in-thread without @ben (must NOT wake Ben),
    // then replies in-thread WITH `@ben` (MUST wake Ben despite `ThreadReply < Mention` precedence).
    let ben_post = sat1_node.cli_ok(
        ben,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--body",
            "Ben status note",
        ],
    );
    let ben_post_id = ben_post["id"].as_str().unwrap().to_owned();
    wait_until(
        Duration::from_secs(10),
        "ben channel post replicated to HQ",
        || {
            let list = hq_node.cli_ok(ada, &["msg", "list", "--channel", "#general"]);
            list.as_array()
                .unwrap()
                .iter()
                .any(|m| m["id"] == ben_post_id)
        },
    );
    let quiet_reply = hq_node.cli_ok(
        ada,
        &[
            "msg",
            "reply",
            "--id",
            &ben_post_id,
            "--body",
            "quiet thread note",
        ],
    );
    let quiet_reply_id = quiet_reply["id"].as_str().unwrap().to_owned();
    wait_until(
        Duration::from_secs(10),
        "quiet thread reply replicated to sat1",
        || {
            let list = sat1_node.cli_ok(ben, &["msg", "list", "--channel", "#general"]);
            list.as_array()
                .unwrap()
                .iter()
                .any(|m| m["id"] == quiet_reply_id)
        },
    );
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        sat1_node.recorded.count_for("ben"),
        ben_before_chan,
        "ordinary channel thread reply without @mention must not wake Ben"
    );
    let _mention_reply = hq_node.cli_ok(
        ada,
        &[
            "msg",
            "reply",
            "--id",
            &ben_post_id,
            "--body",
            "Ping @ben inside thread",
        ],
    );
    let mention_turn =
        sat1_node
            .recorded
            .wait_for_agent("ben", ben_before_chan + 1, Duration::from_secs(10));
    assert!(
        mention_turn.prompt.contains("inside thread"),
        "prompt={}",
        mention_turn.prompt
    );
}

fn verify_cross_office_ask_and_answer(
    hq_node: &LiveOfficeNode,
    sat1_node: &LiveOfficeNode,
    ada: &str,
    ben: &str,
) {
    // 4. Cross-office `msg ask` (short-hold 20s) + `msg reply --answer` wakes HQ `ask_waiters` < 5s!
    let ben_before_ask = sat1_node.recorded.count_for("ben");
    let ask_binding = hq_node.issue_binding(ada);
    let ask_started = Instant::now();
    let ask_thread = thread::spawn(move || {
        let ask_json = json!({
            "to": "ben",
            "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Approve cross-office deploy?" }],
            "hold_ms": 20_000
        })
        .to_string();
        let args = vec![
            "msg".to_owned(),
            "ask".to_owned(),
            "--json".to_owned(),
            ask_json,
        ];
        let out = org_agent_cli::execute(&args, ask_binding.binding.clone());
        drop(ask_binding);
        out
    });
    let _ben_ask_turn =
        sat1_node
            .recorded
            .wait_for_agent("ben", ben_before_ask + 1, Duration::from_secs(10));
    let mut ask_id = String::new();
    wait_until(
        Duration::from_secs(10),
        "cross-office ask in Ben inbox",
        || {
            let inbox = sat1_node.cli_ok(ben, &["msg", "inbox"]);
            if let Some(item) = inbox
                .as_array()
                .unwrap()
                .iter()
                .find(|i| i["kind"] == "request")
            {
                ask_id = item["id"].as_str().unwrap().to_owned();
                return true;
            }
            false
        },
    );
    let _ = sat1_node.cli_ok(
        ben,
        &[
            "msg",
            "reply",
            "--id",
            &ask_id,
            "--answer",
            r#"[{"question_id":"q1","option_ids":["yes"]}]"#,
        ],
    );
    let ask_out = ask_thread.join().unwrap();
    assert_eq!(
        ask_out.exit_code, 0,
        "cross-office ask failed: {}",
        ask_out.stdout
    );
    let ask_json: Value = serde_json::from_str(&ask_out.stdout).unwrap();
    assert_eq!(ask_json["data"]["status"], "answered");
    assert!(
        ask_started.elapsed() < Duration::from_secs(6),
        "cross-office ask should unblock via InboundDeliveryNotifier, took {:?}",
        ask_started.elapsed()
    );
}

fn verify_cross_office_withdraw_and_handoff(
    hq_node: &LiveOfficeNode,
    sat1_node: &LiveOfficeNode,
    ada: &str,
    ben: &str,
) {
    // 5. Cross-office `msg withdraw`: Ada withdraws an open ask -> replicated to Sat 1.
    let w_ask_json = json!({
        "to": "ben",
        "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Cancel me?" }],
        "hold_ms": 0
    })
    .to_string();
    let w_ask = hq_node.cli_ok(ada, &["msg", "ask", "--json", &w_ask_json]);
    let w_ask_id = w_ask["ask_id"].as_str().unwrap().to_owned();
    wait_until(
        Duration::from_secs(10),
        "withdraw ask replicated to sat1",
        || {
            sat1_node
                .office
                .inspect_ask_status(ben, &w_ask_id)
                .ok()
                .and_then(Result::ok)
                .is_some()
        },
    );
    let _ = hq_node.cli_ok(
        ada,
        &[
            "msg",
            "withdraw",
            "--id",
            &w_ask_id,
            "--reason",
            "never mind",
        ],
    );
    wait_until(
        Duration::from_secs(10),
        "withdrawal replicated to sat1",
        || {
            sat1_node
                .office
                .inspect_ask_status(ben, &w_ask_id)
                .ok()
                .and_then(Result::ok)
                .is_some_and(|s| s.status == org_proto::ask::AskStatus::Withdrawn)
        },
    );

    // 6. Cross-office `msg accept` handoff (including adversarial `--reason origin-close` regression):
    //    HQ sends handoff -> Sat 1 Ben accepts with reason `"origin-close"` -> HQ sees acceptance!
    let ben_before_handoff = sat1_node.recorded.count_for("ben");
    let handoff = hq_node.cli_ok(
        ada,
        &[
            "msg",
            "send",
            "--to",
            "ben",
            "--kind",
            "handoff",
            "--body",
            "Own cross-office release",
        ],
    );
    let handoff_id = handoff["id"].as_str().unwrap().to_owned();
    let conv_id = handoff["conversation_id"].as_str().unwrap().to_owned();
    let _ =
        sat1_node
            .recorded
            .wait_for_agent("ben", ben_before_handoff + 1, Duration::from_secs(10));
    let _ = sat1_node.cli_ok(
        ben,
        &[
            "msg",
            "accept",
            "--id",
            &handoff_id,
            "--reason",
            "origin-close",
        ],
    );
    wait_until(
        Duration::from_secs(10),
        "handoff accept replicated to HQ",
        || {
            let msgs = hq_node
                .office
                .chat_messages(
                    ada,
                    org_proto::chat::ChatMessagesParams {
                        conversation: Some(conv_id.clone()),
                        thread_id: None,
                        limit: Some(50),
                        cursor: None,
                    },
                )
                .ok()
                .and_then(Result::ok);
            msgs.is_some_and(|res| {
                res.messages.iter().any(|m| {
                    m.id == handoff_id
                        && m.acceptance.as_ref().is_some_and(|a| {
                            a.accepted_by == ben && a.reason.as_deref() == Some("origin-close")
                        })
                })
            })
        },
    );
}

fn handoff_accept_delta_files(
    org_id: &str,
    conv_id: &str,
    msg_id: &str,
    emp_id: &str,
    accepted_at: Option<&str>,
    reason: Option<&str>,
) -> Vec<(String, Option<Vec<u8>>)> {
    let path =
        format!("messaging/conversations/{conv_id}/participants/{emp_id}/answers/{msg_id}.json");
    let body = json!({
        "schema_version": 1,
        "organization_id": org_id,
        "conversation_id": conv_id,
        "message_id": msg_id,
        "employee_id": emp_id,
        "revision": 1,
        "accepted_by": emp_id,
        "accepted_at": accepted_at,
        "reason": reason,
        "answer": null
    });
    vec![(path, Some(serde_json::to_vec(&body).unwrap()))]
}

#[test]
fn relayed_handoff_accept_enforces_placement_addressing_validation_and_tiebreak() {
    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let team_id = hq.org_snapshot().unwrap().teams[0].id.clone();
    let hq_id = hq.info().office_id.clone();
    let org_id = hq.info().org_id.clone();
    let (_conn_id, profile_id) = setup_provider_on_hq(&hq, &owner);
    let hq_node = LiveOfficeNode::start(Arc::clone(&hq), true, false);
    let hq_addr = hq_node.hq_fed_addr();
    let (_sat_dir, sat1) = enroll_satellite(&hq, &owner, &hq_addr, "Berlin Sat", 1);
    let sat1_id = sat1.info().office_id.clone();

    let ada = hire_agent_at(&hq, &owner, &team_id, &hq_id, &profile_id, "Ada", "ada");
    let ben = hire_agent_at(&hq, &owner, &team_id, &sat1_id, &profile_id, "Ben", "ben");
    let cy = hire_agent_at(&hq, &owner, &team_id, &sat1_id, &profile_id, "Cy", "cy");

    let handoff = hq_node.cli_ok(
        &ada,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--kind",
            "handoff",
            "--body",
            "Please take this @ben",
        ],
    );
    let msg_id = handoff["id"].as_str().unwrap().to_owned();
    let conv_id = handoff["conversation_id"].as_str().unwrap().to_owned();
    let big_reason = "x".repeat(org_proto::msg::MAX_NOTE_BYTES + 16);
    struct RejectedAcceptCase<'a> {
        origin: &'a str,
        actor: &'a str,
        ts: Option<&'a str>,
        reason: Option<&'a str>,
        expected_err: &'a str,
    }
    let rejected_cases = [
        RejectedAcceptCase {
            origin: &sat1_id,
            actor: &owner,
            ts: Some("2026-11-25T07:00:00Z"),
            reason: Some("forged"),
            expected_err: "not placed at authenticated origin office",
        },
        RejectedAcceptCase {
            origin: &hq_id,
            actor: &ada,
            ts: Some("2026-11-25T07:00:00Z"),
            reason: Some("self"),
            expected_err: "handoff sender cannot accept own handoff",
        },
        RejectedAcceptCase {
            origin: &sat1_id,
            actor: &cy,
            ts: Some("2026-11-25T07:00:00Z"),
            reason: Some("hijack"),
            expected_err: "not an addressed recipient",
        },
        RejectedAcceptCase {
            origin: &sat1_id,
            actor: &ben,
            ts: None,
            reason: Some("ok"),
            expected_err: "missing accepted_at",
        },
        RejectedAcceptCase {
            origin: &sat1_id,
            actor: &ben,
            ts: Some("2026-11-25T07:00:00Z"),
            reason: Some(&big_reason),
            expected_err: "exceeds size limit",
        },
    ];
    for case in rejected_cases {
        let files = handoff_accept_delta_files(
            &org_id,
            &conv_id,
            &msg_id,
            case.actor,
            case.ts,
            case.reason,
        );
        let relay_id = format!("accept-{msg_id}-{}", case.actor);
        let err = hq
            .apply_relayed_messaging_files(case.origin, &relay_id, &hq.next_uuid_v7(), files)
            .expect_err(&format!(
                "expected refusal for origin={} actor={} ts={:?}",
                case.origin, case.actor, case.ts
            ));
        let err_debug = format!("{err:?}");
        assert!(
            err_debug.contains(case.expected_err),
            "expected '{}', got {err_debug} for origin={} actor={}",
            case.expected_err,
            case.origin,
            case.actor
        );
    }

    // Apply later `"origin-close"` accept by `@cy` FIRST and earlier `"first"` accept by `@ben`
    // SECOND so both rows exist in SQLite `messaging_answers` and `answer_for_message` ordering is
    // genuinely exercised (ensuring `reason = 'origin-close'` only prioritizes ask answers).
    let multi = hq_node.cli_ok(
        &ada,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--kind",
            "handoff",
            "--body",
            "Either @ben or @cy take this",
        ],
    );
    let multi_id = multi["id"].as_str().unwrap().to_owned();
    let cy_later = handoff_accept_delta_files(
        &org_id,
        &conv_id,
        &multi_id,
        &cy,
        Some("2026-11-25T07:05:00Z"),
        Some("origin-close"),
    );
    hq.apply_relayed_messaging_files(
        &sat1_id,
        &format!("accept-{multi_id}-{cy}"),
        &hq.next_uuid_v7(),
        cy_later,
    )
    .unwrap();
    let ben_first = handoff_accept_delta_files(
        &org_id,
        &conv_id,
        &multi_id,
        &ben,
        Some("2026-11-25T07:00:01Z"),
        Some("first"),
    );
    hq.apply_relayed_messaging_files(
        &sat1_id,
        &format!("accept-{multi_id}-{ben}"),
        &hq.next_uuid_v7(),
        ben_first,
    )
    .unwrap();
    let msgs = hq
        .chat_messages(
            &ada,
            org_proto::chat::ChatMessagesParams {
                conversation: Some(conv_id),
                thread_id: None,
                limit: Some(50),
                cursor: None,
            },
        )
        .unwrap()
        .unwrap();
    let accepted = msgs
        .messages
        .iter()
        .find(|m| m.id == multi_id)
        .unwrap()
        .acceptance
        .as_ref()
        .unwrap();
    assert_eq!(accepted.accepted_by, ben);
    assert_eq!(accepted.reason.as_deref(), Some("first"));
    // Replicated `replication.apply_handoff_accept` entries must never be re-exported as local relays.
    let pending = hq.collect_pending_message_journal_entries().unwrap();
    assert!(
        !pending
            .entries
            .iter()
            .any(|e| e.message_id.starts_with(&format!("accept-{multi_id}-"))),
        "replicated handoff accept must not echo into outbound relay: {:?}",
        pending
            .entries
            .iter()
            .map(|e| &e.message_id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn chat_reply_enforces_oversize_snooze_clear_offboarded_dm_and_thread_mention_wake() {
    let hq = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let team_id = hq.org_snapshot().unwrap().teams[0].id.clone();
    let hq_id = hq.info().office_id.clone();
    let (_conn_id, profile_id) = setup_provider_on_hq(&hq, &owner);
    let node = LiveOfficeNode::start(Arc::clone(&hq), false, false);

    let ada = hire_agent_at(&hq, &owner, &team_id, &hq_id, &profile_id, "Ada", "ada");
    let ben = hire_agent_at(&hq, &owner, &team_id, &hq_id, &profile_id, "Ben", "ben");

    let huge_id = "m".repeat(org_proto::msg::MAX_ID_BYTES + 8);
    let err = hq
        .chat_reply(
            &ada,
            org_proto::msg::MsgReplyParams {
                id: hq.next_uuid_v7(),
                to_id: huge_id,
                priority: None,
                subject: None,
                body: "reply".into(),
                meta: std::collections::BTreeMap::new(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(err.code, org_proto::ErrorCode::TooLarge);

    let dm = node.cli_ok(&ada, &["msg", "send", "--to", "ben", "--body", "check me"]);
    let dm_id = dm["id"].as_str().unwrap().to_owned();
    let _ = node
        .recorded
        .wait_for_agent("ben", 1, Duration::from_secs(5));
    let _ = node.cli_ok(
        &ben,
        &[
            "msg",
            "defer",
            "--id",
            &dm_id,
            "--until",
            "2099-01-01T00:00:00Z",
        ],
    );
    let _ = node.cli_ok(
        &ben,
        &["msg", "reply", "--id", &dm_id, "--body", "done now"],
    );
    let inbox_all = hq
        .chat_inbox(
            &ben,
            org_proto::chat::ChatInboxParams {
                include_all: true,
                state: None,
                because: None,
                from: None,
                thread_id: None,
                limit: Some(50),
                cursor: None,
            },
        )
        .unwrap()
        .unwrap();
    let parent_item = inbox_all.items.iter().find(|i| i.id == dm_id).unwrap();
    assert_eq!(
        parent_item.state,
        org_proto::msg::MessageLifecycleState::Read
    );
    assert!(parent_item.snoozed_until.is_none());

    let ben_post = node.cli_ok(
        &ben,
        &["msg", "send", "--to", "#general", "--body", "Ben note"],
    );
    let ben_post_id = ben_post["id"].as_str().unwrap().to_owned();
    let before = node.recorded.count_for("ben");
    let _ = node.cli_ok(
        &ada,
        &["msg", "reply", "--id", &ben_post_id, "--body", "no ping"],
    );
    thread::sleep(Duration::from_millis(200));
    assert_eq!(node.recorded.count_for("ben"), before);
    let _ = node.cli_ok(
        &ada,
        &["msg", "reply", "--id", &ben_post_id, "--body", "hey @ben"],
    );
    let woke = node
        .recorded
        .wait_for_agent("ben", before + 1, Duration::from_secs(5));
    assert!(woke.prompt.contains("hey @ben"));

    let offboard_req = Request::new::<org_proto::hiring::AgentOffboard>(
        3,
        &org_proto::hiring::AgentOffboardParams {
            command_id: hq.next_uuid_v7(),
            member_id: ben.clone(),
            expected_version: 1,
        },
    )
    .unwrap();
    let off_resp = hq.dispatch_hiring_rpc(Some(&owner), &offboard_req).unwrap();
    assert!(
        matches!(off_resp.outcome, Outcome::Result(_)),
        "{off_resp:?}"
    );
    let err_off = hq
        .chat_reply(
            &ada,
            org_proto::msg::MsgReplyParams {
                id: hq.next_uuid_v7(),
                to_id: dm_id,
                priority: None,
                subject: None,
                body: "self follow-up".into(),
                meta: std::collections::BTreeMap::new(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(err_off.code, org_proto::ErrorCode::UnknownRecipient);
}
