//! Real-socket, real-mTLS, real-`SessionManager`, and real-`agent-os` CLI end-to-end tests for
//! same-office and cross-office Agent-to-Agent messaging (`TASK-117`).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_agent_cli::Binding;
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
use crate::federation_runtime::{
    build_federation_delivery_notifier, build_federation_wake_handler,
    start_optional_satellite_sync,
};
use crate::federation_stream::SatSyncWake;
use crate::office_ports::{build_messaging_ports, build_office_ports};
use crate::{live, org_feed, serve};

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

    fn aliases(&self) -> &'static [&'static str] {
        &["anthropic", "claude-code"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mid_turn_delivery: org_agent_runtime::MidTurnDelivery::None,
            image_input: org_agent_runtime::ImageInputMode::StagedFileOnly,
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
    pub(super) shared: Arc<serve::Shared>,
    pub(super) recorded: Arc<RecordedTurns>,
    sat_wake: Arc<SatSyncWake>,
    stop: Arc<AtomicBool>,
    hq_fed: Option<HqFederationServer>,
    sat_thread: Option<thread::JoinHandle<()>>,
    wake_thread: Option<thread::JoinHandle<()>>,
    serve_thread: Option<thread::JoinHandle<()>>,
}

fn build_test_session_manager(
    office: &Arc<Office>,
    socket: &Path,
) -> (tempfile::TempDir, Arc<RecordedTurns>, Arc<SessionManager>) {
    let bin_dir = tempfile::Builder::new().prefix("a2ab").tempdir().unwrap();
    let script_path = bin_dir.path().join("fake-claude");
    fs::write(&script_path, ECHO_RUNTIME_SCRIPT).unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
    let recorded = Arc::new(RecordedTurns::default());
    let mut reg = RuntimeRegistry::empty();
    reg.register(
        Arc::new(RecordingRuntime {
            recorded: Arc::clone(&recorded),
        }),
        Some(script_path),
    );
    let cfg = SessionManagerConfig::with_runtimes(socket.to_path_buf(), reg);
    let sessions = Arc::new(SessionManager::new(Arc::clone(office), cfg));
    SessionManager::attach_self(&sessions);
    (bin_dir, recorded, sessions)
}

impl LiveOfficeNode {
    pub(super) fn start(office: Arc<Office>, start_hq_fed: bool, start_sat_sync: bool) -> Self {
        let addr = start_hq_fed.then(|| "127.0.0.1:0".parse().unwrap());
        Self::start_with_hq_addr(office, addr, start_sat_sync)
    }

    pub(super) fn start_with_hq_addr(
        office: Arc<Office>,
        hq_bind_addr: Option<std::net::SocketAddr>,
        start_sat_sync: bool,
    ) -> Self {
        let _ = org_platform::process::harden_self();
        let sock_dir = tempfile::Builder::new().prefix("a2a").tempdir().unwrap();
        let socket = sock_dir.path().join("office.sock");
        let (bin_dir, recorded, sessions) = build_test_session_manager(&office, &socket);

        let wake = Arc::new(org_feed::FeedWake::new());
        let live_wake = Arc::clone(&wake);
        let live = Arc::new(live::LiveEvents::for_owner(
            Box::new(move || live_wake.notify()),
            office.owner_member_id().to_owned(),
        ));
        let channel_watcher = Arc::new(crate::watcher::ChannelWatcher::production(
            Arc::clone(&office),
            Some(Arc::clone(&sessions)),
            sessions.runtimes().clone(),
        ));
        let sat_wake = Arc::new(SatSyncWake::default());
        let hq_fed_slot: crate::federation_runtime::HqFederationSlot = Arc::default();
        let sync_wake = crate::federation_runtime::build_sync_wake_fn(
            &office,
            Arc::clone(&sat_wake),
            Arc::clone(&hq_fed_slot),
        );
        let messaging = build_messaging_ports(
            &office,
            &live,
            &wake,
            &sessions,
            &channel_watcher,
            Arc::clone(&sync_wake),
        );
        let wake_handler = build_federation_wake_handler(&sessions, &messaging.chat_svc);
        let delivery_notifier = build_federation_delivery_notifier(
            office.owner_member_id().to_owned(),
            &messaging.chat_svc,
            &messaging.mailbox,
            &live,
            &channel_watcher,
        );
        let hq_fed = hq_bind_addr.map(|bind_addr| {
            let srv = HqFederationServer::start_with_notifier(
                Arc::clone(&office),
                bind_addr,
                &["127.0.0.1".into()],
                Some(Arc::clone(&wake_handler)),
                Some(Arc::clone(&delivery_notifier)),
            )
            .unwrap();
            let _ = hq_fed_slot.set(srv.handle());
            srv
        });
        let stop = Arc::new(AtomicBool::new(false));
        let sat_thread = if start_sat_sync {
            start_optional_satellite_sync(
                &office,
                &stop,
                Arc::clone(&sat_wake),
                wake_handler,
                delivery_notifier,
                Some(Arc::clone(&wake)),
                Arc::downgrade(&sessions),
            )
        } else {
            None
        };

        let hq_inbox = crate::office_runtime::hq_inbox(Arc::clone(&office)).unwrap();
        let ports = build_office_ports(
            Arc::clone(&office),
            Arc::clone(&sessions),
            channel_watcher,
            live,
            &wake,
            messaging,
            Arc::new(AtomicBool::new(hq_fed.is_some())),
            hq_fed_slot,
            sync_wake,
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
            shared,
            recorded,
            sat_wake,
            stop,
            hq_fed,
            sat_thread,
            wake_thread,
            serve_thread: Some(serve_thread),
        }
    }

    /// Wakes this satellite node's sync loop at once.
    pub(super) fn notify_sat_sync(&self) {
        self.sat_wake.notify();
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
                None,
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
            external_state: None,
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
                effort: None,
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

pub(super) fn sat_custody(org_id: &str, sat_office_id: &str, inc_suffix: u8) -> Custody {
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
                        ..Default::default()
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
                ..Default::default()
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
                attachment_ids: Vec::new(),
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
    // Ben's DM reply wakes Ada asynchronously; wait for Ada's turn to land so it does not
    // replace Ada's test ScopedBinding during the channel reply below.
    let _ = node
        .recorded
        .wait_for_agent("ada", 1, Duration::from_secs(5));
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
                ..Default::default()
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
                attachment_ids: Vec::new(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(err_off.code, org_proto::ErrorCode::UnknownRecipient);
}

fn assert_proxied_session_ops_succeed(
    hq_node: &LiveOfficeNode,
    hq: &Office,
    owner: &str,
    ben: &str,
    sat1_id: &str,
) {
    for target in [ben, "ben"] {
        let inspect_req = Request::new::<org_proto::sessions::MemberSessionInspect>(
            502,
            &org_proto::sessions::MemberSessionInspectParams {
                employee_id: Some(target.to_owned()),
            },
        )
        .unwrap();
        let Outcome::Result(val) = hq_node
            .shared
            .answer_for_test(&inspect_req, owner, true)
            .outcome
        else {
            panic!("proxied session_inspect({target}) failed");
        };
        let inspected: org_proto::sessions::SessionInspectResult =
            serde_json::from_value(val).unwrap();
        assert_eq!(inspected.session.employee_id, ben);
        assert_eq!(inspected.session.office_id, sat1_id);
    }

    let grants_req = Request::new::<org_proto::sessions::MemberSessionGrants>(
        503,
        &org_proto::sessions::MemberSessionGrantsParams {
            employee_id: Some(ben.to_owned()),
        },
    )
    .unwrap();
    let Outcome::Result(grants_val) = hq_node
        .shared
        .answer_for_test(&grants_req, owner, true)
        .outcome
    else {
        panic!("proxied session_grants failed");
    };
    let grants: org_proto::sessions::SessionGrantsResult =
        serde_json::from_value(grants_val).unwrap();
    assert_eq!(grants.employee_id, ben);

    let list_req = Request::new::<org_proto::sessions::MemberSessionList>(
        504,
        &org_proto::sessions::MemberSessionListParams {
            employee_id: Some(ben.to_owned()),
            limit: Some(10),
        },
    )
    .unwrap();
    assert!(matches!(
        hq_node
            .shared
            .answer_for_test(&list_req, owner, true)
            .outcome,
        Outcome::Result(_)
    ));

    let clear_req = Request::new::<org_proto::sessions::MemberSessionClearStall>(
        505,
        &org_proto::sessions::MemberSessionClearStallParams {
            command_id: hq.next_uuid_v7(),
            employee_id: ben.to_owned(),
        },
    )
    .unwrap();
    let clear_resp = hq_node.shared.answer_for_test(&clear_req, owner, true);
    let Outcome::Result(clear_val) = clear_resp.outcome else {
        panic!("proxied session_clear_stall failed: {clear_resp:?}");
    };
    let cleared: org_proto::sessions::SessionView = serde_json::from_value(clear_val).unwrap();
    assert_eq!(cleared.employee_id, ben);
    assert!(!cleared.recovery_required);
    assert!(!cleared.stall_locked);
}

fn assert_proxied_session_auth_and_disconnect(
    hq_node: &LiveOfficeNode,
    sat1_node: &mut LiveOfficeNode,
    hq: &Office,
    owner: &str,
    ada: &str,
    ben: &str,
) {
    let agent_inspect = Request::new::<org_proto::sessions::MemberSessionInspect>(
        506,
        &org_proto::sessions::MemberSessionInspectParams {
            employee_id: Some(ben.to_owned()),
        },
    )
    .unwrap();
    match hq_node
        .shared
        .answer_for_test(&agent_inspect, ada, false)
        .outcome
    {
        Outcome::Error(err) => assert_eq!(err.code, org_proto::ErrorCode::NotAuthorized),
        other => panic!("expected NotAuthorized for agent proxy inspect, got {other:?}"),
    }
    let agent_clear = Request::new::<org_proto::sessions::MemberSessionClearStall>(
        507,
        &org_proto::sessions::MemberSessionClearStallParams {
            command_id: hq.next_uuid_v7(),
            employee_id: ben.to_owned(),
        },
    )
    .unwrap();
    match hq_node
        .shared
        .answer_for_test(&agent_clear, ada, false)
        .outcome
    {
        Outcome::Error(err) => assert_eq!(err.code, org_proto::ErrorCode::NotAuthorized),
        other => panic!("expected NotAuthorized for agent proxy clear_stall, got {other:?}"),
    }

    sat1_node.stop.store(true, Ordering::SeqCst);
    sat1_node.sat_wake.notify();
    if let Some(h) = sat1_node.sat_thread.take() {
        let _ = h.join();
    }
    wait_until(
        Duration::from_secs(3),
        "hq drops disconnected sat1 peer",
        || {
            let probe = Request::new::<org_proto::sessions::MemberSessionInspect>(
                508,
                &org_proto::sessions::MemberSessionInspectParams {
                    employee_id: Some(ben.to_owned()),
                },
            )
            .unwrap();
            matches!(
                hq_node.shared.answer_for_test(&probe, owner, true).outcome,
                Outcome::Error(err) if err.code == org_proto::ErrorCode::Unavailable
            )
        },
    );
}

#[test]
fn task140_persistent_mtls_low_latency_and_task143_hq_session_proxy_end_to_end() {
    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let team_id = hq.org_snapshot().unwrap().teams[0].id.clone();
    let hq_id = hq.info().office_id.clone();
    let (conn_id, profile_id) = setup_provider_on_hq(&hq, &owner);
    let hq_node = LiveOfficeNode::start(Arc::clone(&hq), true, false);
    let hq_addr = hq_node.hq_fed_addr();

    let (_sat_dir, sat1) = enroll_satellite(&hq, &owner, &hq_addr, "SatProxy", 11);
    let sat1_id = sat1.info().office_id.clone();
    let ada = hire_agent_at(&hq, &owner, &team_id, &hq_id, &profile_id, "Ada", "ada");
    let ben = hire_agent_at(&hq, &owner, &team_id, &sat1_id, &profile_id, "Ben", "ben");
    bind_provider_on_office(&sat1, sat1.owner_member_id(), &conn_id);
    let mut sat1_node = LiveOfficeNode::start(Arc::clone(&sat1), false, true);

    wait_until(Duration::from_secs(10), "sat1 syncs Ben hire", || {
        sat1.org_snapshot()
            .is_ok_and(|s| s.members.iter().any(|m| m.id == ben))
    });

    // TASK-140 AC #1: proactive OrgEvent push on persistent mTLS delivers HQ team.create promptly.
    let team_req = Request::new::<org_proto::org::TeamCreate>(
        501,
        &org_proto::org::TeamCreateParams {
            command_id: hq.next_uuid_v7(),
            team_id: hq.next_uuid_v7(),
            name: "InstantPush".into(),
            owner_team_id: Some(team_id.clone()),
        },
    )
    .unwrap();
    assert!(matches!(
        hq_node
            .shared
            .answer_for_test(&team_req, &owner, true)
            .outcome,
        Outcome::Result(_)
    ));
    let push_started = Instant::now();
    wait_until(
        Duration::from_secs(2),
        "sat1 receives pushed OrgEvent",
        || {
            sat1.org_snapshot()
                .is_ok_and(|s| s.teams.iter().any(|t| t.name == "InstantPush"))
        },
    );
    assert!(
        push_started.elapsed() < Duration::from_millis(1500),
        "persistent mTLS OrgEvent push took {:?}",
        push_started.elapsed()
    );

    // TASK-143 AC #3: HQ owner proxies session RPCs over mTLS, rejects agents, and fails closed.
    assert_proxied_session_ops_succeed(&hq_node, &hq, &owner, &ben, &sat1_id);
    assert_proxied_session_auth_and_disconnect(&hq_node, &mut sat1_node, &hq, &owner, &ada, &ben);
}

#[test]
fn task192_satellite_worktree_prepare_and_push_on_hq_task_preserves_persistent_sync() {
    use crate::git_calls::GitService;
    use crate::live::LiveEvents;
    use org_proto::git::{
        GitPush, GitPushParams, RepoAttach, RepoAttachParams, RepoRegister, RepoRegisterParams,
        RepositoryKindWire, WorktreePrepare, WorktreePrepareParams, WorktreePrepareResult,
    };
    use org_proto::org::ProjectCreateParams;
    use org_proto::tasks::{TaskStatus, TasksCreateParams, TasksTransitionParams};
    use std::process::Command;

    let run_git = |dir: &std::path::Path, args: &[&str]| {
        let out = Command::new("git")
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };

    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let team_id = hq.org_snapshot().unwrap().teams[0].id.clone();
    let (_conn_id, profile_id) = setup_provider_on_hq(&hq, &owner);
    let hq_node = LiveOfficeNode::start(Arc::clone(&hq), true, false);
    let hq_addr = hq_node.hq_fed_addr();

    let (_sat_dir, sat1) = enroll_satellite(&hq, &owner, &hq_addr, "SatWorktree", 19);
    let sat1_id = sat1.info().office_id.clone();
    let ben = hire_agent_at(&hq, &owner, &team_id, &sat1_id, &profile_id, "Ben", "ben");
    let sat1_node = LiveOfficeNode::start(Arc::clone(&sat1), false, true);

    // Seed a real local bare origin repository and attach it to a project on HQ.
    let origin_tmp = tempfile::tempdir().unwrap();
    let bare = origin_tmp.path().join("origin.git");
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "--initial-branch=main"]);
    let seed = origin_tmp.path().join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    run_git(&seed, &["init", "--initial-branch=main"]);
    run_git(&seed, &["config", "user.name", "Seed"]);
    run_git(&seed, &["config", "user.email", "seed@example.test"]);
    std::fs::write(seed.join("README.md"), "# Seed\n").unwrap();
    run_git(&seed, &["add", "README.md"]);
    run_git(&seed, &["commit", "-m", "initial"]);
    run_git(&seed, &["remote", "add", "origin", bare.to_str().unwrap()]);
    run_git(&seed, &["push", "origin", "main"]);

    let project_id = hq.next_uuid_v7();
    let proj_params: ProjectCreateParams = serde_json::from_value(serde_json::json!({
        "command_id": hq.next_uuid_v7(),
        "project_id": project_id,
        "name": "Core",
        "slug": "core",
        "owner_team_id": team_id,
        "contributing_team_ids": [],
    }))
    .unwrap();
    hq.submit_org(
        &owner,
        org_office::rules::OrgCommand::ProjectCreate(proj_params),
    )
    .unwrap()
    .unwrap();
    let hq_git = GitService::new(
        Arc::clone(&hq),
        Arc::new(LiveEvents::for_owner(Box::new(|| {}), owner.clone())),
    );
    let repo_id = hq.next_uuid_v7();
    let reg_req = Request::new::<RepoRegister>(
        1,
        &RepoRegisterParams {
            command_id: hq.next_uuid_v7(),
            repository_id: repo_id.clone(),
            key: Some("sat-repo".into()),
            kind: RepositoryKindWire::Local,
            remote_url: None,
            origin_path: Some(bare.to_string_lossy().into_owned()),
        },
    )
    .unwrap();
    let reg_resp = hq_git.dispatch(Some(&owner), &reg_req).unwrap();
    assert!(
        matches!(reg_resp.outcome, Outcome::Result(_)),
        "HQ repo register failed: {reg_resp:?}"
    );
    let att_req = Request::new::<RepoAttach>(
        2,
        &RepoAttachParams {
            command_id: hq.next_uuid_v7(),
            project: project_id.clone(),
            repository: repo_id.clone(),
            integration_branch: "main".into(),
        },
    )
    .unwrap();
    let att_resp = hq_git.dispatch(Some(&owner), &att_req).unwrap();
    assert!(
        matches!(att_resp.outcome, Outcome::Result(_)),
        "HQ repo attach failed: {att_resp:?}"
    );

    let created_task = hq
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: hq.next_uuid_v7(),
                task_id: hq.next_uuid_v7(),
                team_id: Some(team_id.clone()),
                assigned_team_id: None,
                project_id: Some(project_id.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Satellite worktree sync test".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: Vec::new(),
                assignee_id: Some(ben.clone()),
                body_markdown: "test body".into(),
                reference_files: Vec::new(),
                modified_files: Vec::new(),
                depends_on: Vec::new(),
                attachment_ids: Vec::new(),
                details: None,
                dod: None,
            },
        )
        .unwrap()
        .unwrap();
    let task_id = created_task.task.id.clone();
    hq_node.hq_fed.as_ref().unwrap().handle().notify_all_peers();

    wait_until(Duration::from_secs(10), "sat1 syncs HQ task & repo", || {
        sat1.task_inspect(&task_id).is_ok_and(|r| r.is_ok())
            && sat1.resolve_repository(&repo_id).is_ok_and(|r| r.is_some())
    });

    // AC #1: Direct `record_task_repository_branch` on satellite for HQ-owned task fails closed.
    let task_id_parsed = org_store::Id::parse(&task_id).unwrap();
    let repo_id_parsed = org_store::Id::parse(&repo_id).unwrap();
    let direct_err = sat1
        .record_task_repository_branch(
            &ben,
            &sat1.next_uuid_v7(),
            &task_id_parsed,
            &repo_id_parsed,
            "task/T-A1-forbidden".into(),
            None,
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(direct_err.code, org_proto::ErrorCode::NotAuthorized);
    assert_eq!(sat1.sync_status().unwrap().pending_work_count, 0);

    // Bind the local origin path on sat1 and prepare the worktree as @ben on sat1.
    sat1.write_local_repo_origin(&repo_id_parsed, bare.to_str().unwrap())
        .unwrap();
    let sat_git = GitService::new(
        Arc::clone(&sat1),
        Arc::new(LiveEvents::for_owner(Box::new(|| {}), owner.clone())),
    );
    let prep_req = Request::new::<WorktreePrepare>(
        3,
        &WorktreePrepareParams {
            command_id: sat1.next_uuid_v7(),
            task: task_id.clone(),
            repository: "sat-repo".into(),
            base_branch: None,
        },
    )
    .unwrap();
    let prep_resp = sat_git.dispatch(Some(&ben), &prep_req).unwrap();
    let Outcome::Result(prep_val) = prep_resp.outcome else {
        panic!("satellite worktree prepare for HQ-owned task failed: {prep_resp:?}");
    };
    let prepared: WorktreePrepareResult = serde_json::from_value(prep_val).unwrap();
    assert!(
        prepared
            .worktree
            .branch
            .starts_with(&format!("task/{}-", created_task.task.key)),
        "unexpected branch: {}",
        prepared.worktree.branch
    );
    // AC #2: No local `task.md` mutation and zero work_outbox pollution on sat1.
    assert_eq!(sat1.sync_status().unwrap().pending_work_count, 0);
    assert_eq!(
        sat1.task_inspect(&task_id).unwrap().unwrap().task.revision,
        1
    );

    // AC #4: Satellite agent commits in worktree and runs git.push without mutating HQ `task.md`.
    let wt_dir = std::path::PathBuf::from(&prepared.worktree.worktree_path);
    std::fs::write(wt_dir.join("satellite.txt"), "from sat1\n").unwrap();
    run_git(&wt_dir, &["add", "satellite.txt"]);
    run_git(
        &wt_dir,
        &[
            "-c",
            "user.name=Ben",
            "-c",
            "user.email=ben@example.test",
            "commit",
            "-m",
            "satellite commit",
        ],
    );
    let push_req = Request::new::<GitPush>(
        4,
        &GitPushParams {
            task: task_id.clone(),
            repository: Some("sat-repo".into()),
        },
    )
    .unwrap();
    let push_resp = sat_git.dispatch(Some(&ben), &push_req).unwrap();
    assert!(
        matches!(push_resp.outcome, Outcome::Result(_)),
        "satellite git.push failed: {push_resp:?}"
    );

    // AC #3: HQ updates the same task to Done (emitting events/0000000002.json); sat1 replicates
    // cleanly with zero immutable-event collision and HQ->sat1 session proxy stays healthy.
    hq.task_transition(
        &owner,
        TasksTransitionParams {
            command_id: hq.next_uuid_v7(),
            task: task_id.clone(),
            expected_revision: 1,
            status: TaskStatus::Done,
            blocked_reason: None,
            resolution: Some("Merged".into()),
            note: None,
            done_anyway_reason: None,
            final_summary: None,
        },
    )
    .unwrap()
    .unwrap();
    hq_node.hq_fed.as_ref().unwrap().handle().notify_all_peers();

    wait_until(
        Duration::from_secs(5),
        "sat1 replicates HQ task done transition cleanly",
        || {
            sat1.task_inspect(&task_id)
                .is_ok_and(|r| r.is_ok_and(|t| t.task.status == TaskStatus::Done))
        },
    );
    assert_eq!(sat1.sync_status().unwrap().pending_work_count, 0);
    assert_proxied_session_ops_succeed(&hq_node, &hq, &owner, &ben, &sat1_id);

    // Blocking #4a: Satellite-owned task (`T-B1`) created on sat1 records its durable branch
    // locally via `worktree.prepare`, syncs via `sync_satellite_work_outbox`, and replicates to HQ.
    assert_sat_owned_worktree_syncs_to_hq(
        &hq,
        &sat1,
        &sat1_node,
        &sat_git,
        &owner,
        &ben,
        &team_id,
        &project_id,
        &repo_id_parsed,
    );
    // Blocking #1 & #4b: HQ-owned tasks with an already-integrated or mismatched durable link are
    // refused by satellite `worktree.prepare` with `Conflict` before creating a worktree.
    assert_sat_worktree_rejects_hq_durable_conflicts(
        &hq,
        &hq_node,
        &sat1,
        &sat_git,
        &owner,
        &ben,
        &team_id,
        &project_id,
        &repo_id_parsed,
    );
    drop(sat1_node);
}

#[expect(
    clippy::too_many_arguments,
    reason = "focused test helper shares fixture handles without adding wrapper boilerplate"
)]
fn assert_sat_owned_worktree_syncs_to_hq(
    hq: &Office,
    sat1: &Office,
    sat1_node: &LiveOfficeNode,
    sat_git: &crate::git_calls::GitService,
    owner: &str,
    ben: &str,
    team_id: &str,
    project_id: &str,
    repo_id_parsed: &org_store::Id,
) {
    use org_proto::git::{WorktreePrepare, WorktreePrepareParams, WorktreePrepareResult};
    use org_proto::tasks::TasksCreateParams;

    let sat_task = sat1
        .task_create(
            ben,
            TasksCreateParams {
                command_id: sat1.next_uuid_v7(),
                task_id: sat1.next_uuid_v7(),
                team_id: Some(team_id.to_owned()),
                assigned_team_id: None,
                project_id: Some(project_id.to_owned()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Satellite owned worktree task".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: Vec::new(),
                assignee_id: Some(ben.to_owned()),
                body_markdown: "owned by sat1".into(),
                reference_files: Vec::new(),
                modified_files: Vec::new(),
                depends_on: Vec::new(),
                attachment_ids: Vec::new(),
                details: None,
                dod: None,
            },
        )
        .unwrap()
        .unwrap();
    let sat_task_id = sat_task.task.id.clone();
    let sat_prep_req = Request::new::<WorktreePrepare>(
        5,
        &WorktreePrepareParams {
            command_id: sat1.next_uuid_v7(),
            task: sat_task_id.clone(),
            repository: "sat-repo".into(),
            base_branch: None,
        },
    )
    .unwrap();
    let Outcome::Result(sat_prep_val) = sat_git.dispatch(Some(ben), &sat_prep_req).unwrap().outcome
    else {
        panic!("sat1 worktree prepare for sat1-owned task failed");
    };
    let sat_prepared: WorktreePrepareResult = serde_json::from_value(sat_prep_val).unwrap();
    sat1_node.sat_wake.notify();
    wait_until(
        Duration::from_secs(10),
        "HQ receives sat1-owned task with recorded durable branch and sat1 processes WorkAck",
        || {
            sat1.sync_status().is_ok_and(|s| s.pending_work_count == 0)
                && hq.task_git_context(owner, &sat_task_id).is_ok_and(|r| {
                    r.is_ok_and(|ctx| {
                        ctx.task.repositories.iter().any(|link| {
                            link.repository_id == *repo_id_parsed
                                && link.branch.as_deref()
                                    == Some(sat_prepared.worktree.branch.as_str())
                        })
                    })
                })
        },
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "focused test helper shares fixture handles without adding wrapper boilerplate"
)]
fn assert_sat_worktree_rejects_hq_durable_conflicts(
    hq: &Office,
    hq_node: &LiveOfficeNode,
    sat1: &Office,
    sat_git: &crate::git_calls::GitService,
    owner: &str,
    ben: &str,
    team_id: &str,
    project_id: &str,
    repo_id_parsed: &org_store::Id,
) {
    use org_proto::git::{WorktreePrepare, WorktreePrepareParams};
    use org_proto::tasks::TasksCreateParams;

    let make_hq_task = |title: &str| {
        hq.task_create(
            owner,
            TasksCreateParams {
                command_id: hq.next_uuid_v7(),
                task_id: hq.next_uuid_v7(),
                team_id: Some(team_id.to_owned()),
                assigned_team_id: None,
                project_id: Some(project_id.to_owned()),
                task_type: Some("task".into()),
                work_type: None,
                title: title.into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: Vec::new(),
                assignee_id: Some(ben.to_owned()),
                body_markdown: title.into(),
                reference_files: Vec::new(),
                modified_files: Vec::new(),
                depends_on: Vec::new(),
                attachment_ids: Vec::new(),
                details: None,
                dod: None,
            },
        )
        .unwrap()
        .unwrap()
    };

    let integrated_task = make_hq_task("Already integrated HQ task");
    let integrated_tid = org_store::Id::parse(&integrated_task.task.id).unwrap();
    hq.record_task_repository_branch(
        owner,
        &hq.next_uuid_v7(),
        &integrated_tid,
        repo_id_parsed,
        format!(
            "task/{}-already-integrated-hq-task",
            integrated_task.task.key
        ),
        Some("0123456789abcdef0123456789abcdef01234567".into()),
    )
    .unwrap()
    .unwrap();
    let overwrite_err = hq
        .record_task_repository_branch(
            owner,
            &hq.next_uuid_v7(),
            &integrated_tid,
            repo_id_parsed,
            format!("task/{}-other", integrated_task.task.key),
            None,
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(overwrite_err.code, org_proto::ErrorCode::Conflict);

    let mismatched_task = make_hq_task("Mismatched branch HQ task");
    let mismatched_tid = org_store::Id::parse(&mismatched_task.task.id).unwrap();
    hq.record_task_repository_branch(
        owner,
        &hq.next_uuid_v7(),
        &mismatched_tid,
        repo_id_parsed,
        format!("task/{}-custom-hq-branch", mismatched_task.task.key),
        None,
    )
    .unwrap()
    .unwrap();
    hq_node.hq_fed.as_ref().unwrap().handle().notify_all_peers();

    wait_until(
        Duration::from_secs(10),
        "sat1 syncs integrated & mismatched HQ tasks",
        || {
            sat1.task_inspect(&integrated_task.task.id)
                .is_ok_and(|r| r.is_ok_and(|t| t.task.revision >= 2))
                && sat1
                    .task_inspect(&mismatched_task.task.id)
                    .is_ok_and(|r| r.is_ok_and(|t| t.task.revision >= 2))
        },
    );

    for (req_id, conflict_tid) in [
        (6u64, &integrated_task.task.id),
        (7u64, &mismatched_task.task.id),
    ] {
        let req = Request::new::<WorktreePrepare>(
            req_id,
            &WorktreePrepareParams {
                command_id: sat1.next_uuid_v7(),
                task: conflict_tid.clone(),
                repository: "sat-repo".into(),
                base_branch: None,
            },
        )
        .unwrap();
        let resp = sat_git.dispatch(Some(ben), &req).unwrap();
        assert!(
            matches!(resp.outcome, Outcome::Error(ref e) if e.code == org_proto::ErrorCode::Conflict),
            "expected Conflict for task {conflict_tid}, got {resp:?}"
        );
    }
}

#[test]
fn task141_satellite_agent_addresses_owner_by_handle_and_receives_reply() {
    use org_proto::chat::{ChatInbox, ChatInboxParams, ChatInboxResult};
    use org_proto::msg::{MsgReply, MsgReplyParams};

    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let team_id = hq.org_snapshot().unwrap().teams[0].id.clone();
    let (conn_id, profile_id) = setup_provider_on_hq(&hq, &owner);

    let hq_node = LiveOfficeNode::start(Arc::clone(&hq), true, false);
    let hq_addr = hq_node.hq_fed_addr();
    let (_sat1_dir, sat1) = enroll_satellite(&hq, &owner, &hq_addr, "Owner Handle Sat", 51);
    let sat1_id = sat1.info().office_id.clone();
    assert_ne!(
        sat1.owner_member_id(),
        owner,
        "satellite custody binds owner_member_id to the satellite office_id"
    );

    let ben = hire_agent_at(&hq, &owner, &team_id, &sat1_id, &profile_id, "Ben", "ben");
    bind_provider_on_office(&sat1, sat1.owner_member_id(), &conn_id);
    let sat1_node = LiveOfficeNode::start(Arc::clone(&sat1), false, true);

    wait_until(Duration::from_secs(10), "sat1 syncs owner and Ben", || {
        sat1.org_snapshot()
            .is_ok_and(|s| s.members.iter().any(|m| m.id == ben))
    });

    // 1. Directory snapshot & `who` on satellite identify the replicated owner by "owner" and by
    //    configured handle ("ana") identically to HQ.
    let sat_snap = sat1.org_snapshot().unwrap();
    let sat_owner = sat_snap
        .members
        .iter()
        .find(|m| m.owner)
        .expect("replicated satellite snapshot must mark the organization owner");
    assert_eq!(sat_owner.id, owner);
    assert_eq!(sat_owner.handle.as_deref(), Some("ana"));
    for lookup in ["owner", "OWNER", "ana", "ANA"] {
        let who = sat1_node.cli_ok(&ben, &["who", "--member", lookup]);
        let arr = who.as_array().expect("who returns array of cards");
        assert_eq!(arr.len(), 1, "satellite who --member {lookup}: {who}");
        assert_eq!(
            arr[0]["id"], owner,
            "satellite who --member {lookup}: {who}"
        );
        assert_eq!(arr[0]["owner"], Value::Bool(true));
    }

    // 2. Satellite agent sends DM to "owner" and to configured owner handle "ana"; both succeed
    //    into the same DM conversation and relay to HQ owner inbox.
    let dm_by_owner = sat1_node.cli_ok(
        &ben,
        &[
            "msg",
            "send",
            "--to",
            "owner",
            "--body",
            "ping via owner handle",
        ],
    );
    let dm_id = dm_by_owner["id"].as_str().unwrap().to_owned();
    let dm_conv_id = dm_by_owner["conversation_id"].as_str().unwrap().to_owned();
    let dm_by_cfg = sat1_node.cli_ok(
        &ben,
        &[
            "msg",
            "send",
            "--to",
            "ana",
            "--body",
            "ping via configured handle",
        ],
    );
    assert_eq!(
        dm_by_cfg["conversation_id"].as_str().unwrap(),
        dm_conv_id,
        "sending to 'owner' and configured handle 'ana' must target the same DM"
    );
    wait_until(
        Duration::from_secs(10),
        "HQ owner inbox receives both satellite DMs",
        || {
            let req = Request::new::<ChatInbox>(1, &ChatInboxParams::default()).unwrap();
            let Outcome::Result(val) = hq_node.shared.answer_for_test(&req, &owner, true).outcome
            else {
                return false;
            };
            let page: ChatInboxResult = serde_json::from_value(val).unwrap();
            page.items.iter().any(|i| i.id == dm_id)
                && page
                    .items
                    .iter()
                    .any(|i| i.id == dm_by_cfg["id"].as_str().unwrap())
        },
    );

    // 3. HQ owner replies -> relays back to satellite -> Ben receives the reply and wakes.
    let ben_before_reply = sat1_node.recorded.count_for("ben");
    let reply_req = Request::new::<MsgReply>(
        2,
        &MsgReplyParams {
            id: hq.next_uuid_v7(),
            to_id: dm_id.clone(),
            subject: None,
            body: "ack from HQ owner".into(),
            priority: None,
            meta: Default::default(),
            attachment_ids: Vec::new(),
        },
    )
    .unwrap();
    let reply_resp = hq_node.shared.answer_for_test(&reply_req, &owner, true);
    let Outcome::Result(reply_val) = reply_resp.outcome else {
        panic!("HQ owner reply failed: {reply_resp:?}");
    };
    let reply_id = reply_val["id"].as_str().unwrap().to_owned();
    let ben_turn =
        sat1_node
            .recorded
            .wait_for_agent("ben", ben_before_reply + 1, Duration::from_secs(10));
    assert!(
        ben_turn.prompt.contains("ack from HQ owner"),
        "prompt={}",
        ben_turn.prompt
    );
    let ben_read = sat1_node.cli_ok(&ben, &["msg", "read", "--id", &reply_id]);
    assert_eq!(ben_read["body"], "ack from HQ owner");

    // 4. Satellite agent `@owner` mention in `#general` resolves and relays to HQ owner inbox.
    let mention_post = sat1_node.cli_ok(
        &ben,
        &[
            "msg",
            "send",
            "--to",
            "#general",
            "--body",
            "FYI @owner please check release",
        ],
    );
    let mention_id = mention_post["id"].as_str().unwrap().to_owned();
    assert!(
        mention_post["mentions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["employee_id"] == owner),
        "satellite @owner mention must resolve to HQ owner: {mention_post}"
    );
    wait_until(
        Duration::from_secs(10),
        "HQ owner inbox receives satellite @owner channel mention",
        || {
            let req = Request::new::<ChatInbox>(9, &ChatInboxParams::default()).unwrap();
            let Outcome::Result(val) = hq_node.shared.answer_for_test(&req, &owner, true).outcome
            else {
                return false;
            };
            let page: ChatInboxResult = serde_json::from_value(val).unwrap();
            page.items.iter().any(|i| i.id == mention_id)
        },
    );

    // 5. Satellite agent `msg ask --to owner` reaches HQ owner and receives owner's answer.
    verify_sat_owner_ask_and_answer(&hq_node, &sat1_node, &owner, &ben);
}

fn hire_agent_reporting_to_none(
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
                reports_to_employee_id: None,
                office_id: Some(office_id.to_owned()),
                model_profile_id: Some(profile_id.to_owned()),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("safe_ask".into()),
                effort: None,
                tools_config: Vec::new(),
                system_prompt_markdown: Some("Be concise.".into()),
                body: None,
            },
            reason: "handover test hire".into(),
        },
    )
    .unwrap();
    let resp = hq.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    match resp.outcome {
        Outcome::Result(v) => v["employee_id"].as_str().unwrap().to_owned(),
        Outcome::Error(e) => panic!("hire {handle} failed: {e:?}"),
    }
}

fn add_person_on_hq(
    hq_node: &LiveOfficeNode,
    hq: &Office,
    owner: &str,
    team_id: &str,
    name: &str,
    handle: &str,
) -> String {
    let id = hq.next_uuid_v7();
    let req = Request::new::<org_proto::org::MemberAddPerson>(
        20,
        &org_proto::org::MemberAddPersonParams {
            command_id: hq.next_uuid_v7(),
            member_id: id.clone(),
            name: name.into(),
            contact: None,
            handle: Some(handle.into()),
            home_team_id: Some(team_id.to_owned()),
        },
    )
    .unwrap();
    let resp = hq_node.shared.answer_for_test(&req, owner, true);
    assert!(
        matches!(resp.outcome, Outcome::Result(_)),
        "add person {handle} failed: {resp:?}"
    );
    id
}

fn hq_inbox_has(hq_node: &LiveOfficeNode, member_id: &str, msg_id: &str) -> bool {
    use org_proto::chat::{ChatInbox, ChatInboxParams, ChatInboxResult};
    let req = Request::new::<ChatInbox>(21, &ChatInboxParams::default()).unwrap();
    let Outcome::Result(val) = hq_node
        .shared
        .answer_for_test(&req, member_id, true)
        .outcome
    else {
        return false;
    };
    let page: ChatInboxResult = serde_json::from_value(val).unwrap();
    page.items.iter().any(|i| i.id == msg_id)
}

fn assert_sat_owner_is(sat: &Office, sat_node: &LiveOfficeNode, agent_id: &str, expected: &str) {
    let snap = sat.org_snapshot().unwrap();
    let owners: Vec<&str> = snap
        .members
        .iter()
        .filter(|m| m.owner)
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(owners, vec![expected], "satellite snapshot owners mismatch");
    let who = sat_node.cli_ok(agent_id, &["who", "--member", "owner"]);
    let arr = who.as_array().unwrap();
    assert_eq!(arr.len(), 1, "who --member owner: {who}");
    assert_eq!(arr[0]["id"], expected);
}

#[test]
fn task141_satellite_tracks_owner_handover_across_reconnect_and_offline_restart() {
    use org_proto::org::{TeamSetLead, TeamSetLeadParams};

    let hq_dir = tempfile::tempdir().unwrap();
    let hq_root = hq_dir.path().canonicalize().unwrap();
    let (hq_initial, hq_roots, hq_custody, hq_key) = Office::for_test_with_real_ca_in(
        &hq_root,
        TestOffice {
            engine: EngineChoice::Sqlite,
        },
    );
    let hq = Arc::new(hq_initial);
    let alice = hq.owner_member_id().to_owned();
    let hq_folder = hq.folder().to_path_buf();
    let hq_instance = hq_custody.identity.instance_id.as_str().to_owned();
    let org_id = hq.info().org_id.clone();
    let team_id = hq.org_snapshot().unwrap().teams[0].id.clone();
    let (conn_id, profile_id) = setup_provider_on_hq(&hq, &alice);

    let hq_node = LiveOfficeNode::start(Arc::clone(&hq), true, false);
    let hq_addr = hq_node.hq_fed_addr();
    let hq_sock_addr: std::net::SocketAddr = hq_addr.parse().unwrap();
    let (sat1_dir, sat1) = enroll_satellite(&hq, &alice, &hq_addr, "Pre-Handover Sat1", 52);
    let sat1_id = sat1.info().office_id.clone();
    let sat1_folder = sat1.folder().to_path_buf();
    let sat1_root = sat1_dir.path().canonicalize().unwrap();
    let sat1_roots = DataRoots {
        durable: sat1_root.join("data").join(&org_id).join("durable"),
        runtime: sat1_root.join("data").join(&org_id).join("runtime"),
    };

    let bob = add_person_on_hq(&hq_node, &hq, &alice, &team_id, "Bob", "bob");
    let charlie = add_person_on_hq(&hq_node, &hq, &alice, &team_id, "Charlie", "charlie");
    // Appoint non-owner Charlie as Team #1 lead — must NEVER elevate Charlie to owner.
    let team_rev = hq.org_snapshot().unwrap().teams[0].version;
    let upd_team = Request::new::<TeamSetLead>(
        22,
        &TeamSetLeadParams {
            command_id: hq.next_uuid_v7(),
            team_id: team_id.clone(),
            expected_version: team_rev,
            lead_member_id: Some(charlie.clone()),
        },
    )
    .unwrap();
    assert!(matches!(
        hq_node
            .shared
            .answer_for_test(&upd_team, &alice, true)
            .outcome,
        Outcome::Result(_)
    ));
    let ben =
        hire_agent_reporting_to_none(&hq, &alice, &team_id, &sat1_id, &profile_id, "Ben", "ben");
    bind_provider_on_office(&sat1, sat1.owner_member_id(), &conn_id);
    let sat1_node = LiveOfficeNode::start(Arc::clone(&sat1), false, true);
    wait_until(Duration::from_secs(10), "sat1 syncs initial org", || {
        sat1.org_snapshot()
            .is_ok_and(|s| s.members.iter().any(|m| m.id == ben))
    });
    assert_sat_owner_is(&sat1, &sat1_node, &ben, &alice);

    // Simulate `browser.org_owner_handover` (Alice -> Bob) + live HQ reload: keep `sat1_node`
    // connected while HQ closes and reopens via `Office::open_as_owner(..., &bob)` on the same
    // federation port so `sat1_node` automatically reconnects and adopts Bob via `Hello`.
    drop(hq_node);
    Arc::try_unwrap(hq).unwrap().close().unwrap();

    let hq_bob = Arc::new(
        Office::open_as_owner(
            hq_folder,
            Some(hq_key),
            &hq_instance,
            hq_roots,
            hq_custody,
            &bob,
        )
        .unwrap(),
    );
    verify_post_handover_convergence_and_offline_restart(
        hq_bob,
        hq_sock_addr,
        &hq_addr,
        sat1,
        sat1_node,
        sat1_folder,
        sat1_roots,
        &org_id,
        &sat1_id,
        &team_id,
        &conn_id,
        &profile_id,
        &alice,
        &bob,
        &charlie,
        &ben,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "focused handover test helper passes fixture handles directly without wrapper boilerplate"
)]
fn verify_post_handover_convergence_and_offline_restart(
    hq_bob: Arc<Office>,
    hq_sock_addr: std::net::SocketAddr,
    hq_addr: &str,
    sat1: Arc<Office>,
    sat1_node: LiveOfficeNode,
    sat1_folder: PathBuf,
    sat1_roots: DataRoots,
    org_id: &str,
    sat1_id: &str,
    team_id: &str,
    conn_id: &str,
    profile_id: &str,
    alice: &str,
    bob: &str,
    charlie: &str,
    ben: &str,
) {
    use org_proto::org::{MemberRemove, MemberRemoveParams};

    let hq_bob_node =
        LiveOfficeNode::start_with_hq_addr(Arc::clone(&hq_bob), Some(hq_sock_addr), false);
    sat1_node.sat_wake.notify();
    let (_sat2_dir, sat2) = enroll_satellite(&hq_bob, bob, hq_addr, "Post-Handover Sat2", 53);
    let sat2_id = sat2.info().office_id.clone();
    let cy = hire_agent_at(&hq_bob, bob, team_id, &sat2_id, profile_id, "Cy", "cy");
    bind_provider_on_office(&sat2, sat2.owner_member_id(), conn_id);
    let sat2_node = LiveOfficeNode::start(Arc::clone(&sat2), false, true);

    // Pre-handover Sat1 AND post-handover Sat2 BOTH resolve "owner" to Bob (zero split-brain,
    // even while ex-owner Alice is still active and Charlie leads Team #1!).
    wait_until(
        Duration::from_secs(10),
        "sat1 and sat2 both converge on new HQ owner Bob",
        || {
            sat1.org_snapshot()
                .is_ok_and(|s| s.members.iter().any(|m| m.id == bob && m.owner))
                && sat2.org_snapshot().is_ok_and(|s| {
                    s.members.iter().any(|m| m.id == cy)
                        && s.members.iter().any(|m| m.id == bob && m.owner)
                })
        },
    );
    assert_sat_owner_is(&sat1, &sat1_node, ben, bob);
    assert_sat_owner_is(&sat2, &sat2_node, &cy, bob);

    // Deactivate ex-owner Alice while Charlie still leads Team #1 -> Sat1 STILL resolves Bob.
    let alice_ver = hq_bob
        .org_snapshot()
        .unwrap()
        .members
        .iter()
        .find(|m| m.id == alice)
        .unwrap()
        .version;
    let deact = Request::new::<MemberRemove>(
        23,
        &MemberRemoveParams {
            command_id: hq_bob.next_uuid_v7(),
            member_id: alice.to_owned(),
            expected_version: alice_ver,
        },
    )
    .unwrap();
    let deact_resp = hq_bob_node.shared.answer_for_test(&deact, bob, true);
    assert!(
        matches!(deact_resp.outcome, Outcome::Result(_)),
        "deactivate Alice failed: {deact_resp:?}"
    );
    hq_bob_node
        .hq_fed
        .as_ref()
        .unwrap()
        .handle()
        .notify_all_peers();
    wait_until(
        Duration::from_secs(10),
        "sat1 syncs Alice deactivation",
        || {
            sat1.org_snapshot()
                .is_ok_and(|s| !s.members.iter().any(|m| m.id == alice))
        },
    );
    assert_sat_owner_is(&sat1, &sat1_node, ben, bob);

    // Satellite DM to "owner" reaches Bob's HQ inbox (never Charlie).
    let dm = sat1_node.cli_ok(
        ben,
        &[
            "msg",
            "send",
            "--to",
            "owner",
            "--body",
            "hello new owner Bob",
        ],
    );
    let dm_id = dm["id"].as_str().unwrap().to_owned();
    wait_until(
        Duration::from_secs(10),
        "new HQ owner Bob receives satellite DM addressed to owner",
        || hq_inbox_has(&hq_bob_node, bob, &dm_id),
    );
    assert!(!hq_inbox_has(&hq_bob_node, charlie, &dm_id));

    // Offline restart of Sat1 (without connecting to HQ) preserves Bob from durable hq-owner.v1.json.
    drop(sat1_node);
    drop(sat2_node);
    drop(hq_bob_node);
    assert_sat_offline_restart_keeps_owner(sat1, sat1_folder, sat1_roots, org_id, sat1_id, bob);
}

fn assert_sat_offline_restart_keeps_owner(
    sat1: Arc<Office>,
    sat1_folder: PathBuf,
    sat1_roots: DataRoots,
    org_id: &str,
    sat1_id: &str,
    expected_owner: &str,
) {
    Arc::try_unwrap(sat1).unwrap().close().unwrap();
    let sat1_custody = sat_custody(org_id, sat1_id, 52);
    let sat1_instance = sat1_custody.identity.instance_id.as_str().to_owned();
    let sat1_reopened = Office::open(
        sat1_folder,
        Some(Key::from_bytes([0x33 + 52; 32])),
        &sat1_instance,
        sat1_roots,
        sat1_custody,
    )
    .unwrap();
    let offline_owners: Vec<String> = sat1_reopened
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .filter(|m| m.owner)
        .map(|m| m.id)
        .collect();
    assert_eq!(offline_owners, vec![expected_owner.to_owned()]);
    sat1_reopened.close().unwrap();
}

fn verify_sat_owner_ask_and_answer(
    hq_node: &LiveOfficeNode,
    sat1_node: &LiveOfficeNode,
    owner: &str,
    ben: &str,
) {
    use org_proto::ask::{ChatAnswer, ChatAnswerParams, QuestionAnswer};
    use org_proto::chat::{ChatInbox, ChatInboxParams, ChatInboxResult};

    let ask_binding = sat1_node.issue_binding(ben);
    let ask_thread = thread::spawn(move || {
        let ask_json = json!({
            "to": "owner",
            "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Proceed with deploy?" }],
            "hold_ms": 15_000
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
    let mut ask_id = String::new();
    wait_until(
        Duration::from_secs(10),
        "HQ owner inbox receives satellite ask",
        || {
            let req = Request::new::<ChatInbox>(3, &ChatInboxParams::default()).unwrap();
            let Outcome::Result(val) = hq_node.shared.answer_for_test(&req, owner, true).outcome
            else {
                return false;
            };
            let page: ChatInboxResult = serde_json::from_value(val).unwrap();
            if let Some(item) = page.items.iter().find(|i| i.kind == "request") {
                ask_id = item.id.clone();
                return true;
            }
            false
        },
    );
    let ans_req = Request::new::<ChatAnswer>(
        4,
        &ChatAnswerParams {
            ask_id: ask_id.clone(),
            answers: vec![QuestionAnswer {
                question_id: "q1".into(),
                option_ids: vec!["yes".into()],
                text: None,
            }],
            option_id: None,
            text: Some("approved".into()),
            attachment_ids: Vec::new(),
        },
    )
    .unwrap();
    let ans_resp = hq_node.shared.answer_for_test(&ans_req, owner, true);
    assert!(
        matches!(ans_resp.outcome, Outcome::Result(_)),
        "HQ owner answer failed: {ans_resp:?}"
    );
    let ask_out = ask_thread.join().unwrap();
    assert_eq!(ask_out.exit_code, 0, "sat ask failed: {}", ask_out.stdout);
    let ask_json: Value = serde_json::from_str(&ask_out.stdout).unwrap();
    assert_eq!(ask_json["data"]["status"], "answered");
}
