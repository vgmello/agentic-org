//! Tests for idle warm-session hibernation, per-office RSS budgets with LRU idle eviction,
//! and supervisor-enforced bounded build concurrency (`TASK-133` ACs #1, #2, #3).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    encode_line, AgentRuntime, Capabilities, Control, ControlError, CostBasis, ExitInfo,
    LaunchContext, LaunchPlan, Output, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult,
    ProbeRun, RuntimeId, Session, SessionEvent, SessionId, SessionSpec, Stdio, StopPlan,
    StopSignal, StopStep, TurnOutcome, Unavailable, UsageScope, VersionRange,
};

use super::*;
use crate::manager::{ProcessMemorySampler, ResourceLimitsConfig};

struct FakeMemorySampler {
    rss_by_pid: HashMap<u32, u64>,
}

impl FakeMemorySampler {
    fn from_pairs(pairs: impl IntoIterator<Item = (u32, u64)>) -> Arc<Self> {
        Arc::new(Self {
            rss_by_pid: pairs.into_iter().collect(),
        })
    }
}

impl ProcessMemorySampler for FakeMemorySampler {
    fn process_tree_rss_bytes(&self, root_pid: u32) -> Option<u64> {
        self.rss_by_pid.get(&root_pid).copied()
    }
}

type ObservedResumes = Arc<Mutex<Vec<Option<String>>>>;

struct ResumeTrackingRuntime {
    observed_resumes: ObservedResumes,
}

impl ResumeTrackingRuntime {
    fn new() -> (Arc<Self>, ObservedResumes) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                observed_resumes: Arc::clone(&log),
            }),
            log,
        )
    }
}

struct TrackingScriptedSession {
    ready: bool,
    closed: bool,
}

impl Session for TrackingScriptedSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match v.get("event").and_then(|x| x.as_str()) {
            Some("ready") => {
                self.ready = true;
                vec![Output::Event(SessionEvent::Ready {
                    runtime_version: Some("1.0.0".into()),
                    pid: None,
                    agents: vec!["hr".into()],
                    permission_mode: Some("default".into()),
                })]
            }
            Some("session_started") => {
                let rsid = v
                    .get("rsid")
                    .and_then(|x| x.as_str())
                    .unwrap_or("11111111-1111-4111-8111-111111111111");
                vec![Output::Event(SessionEvent::SessionStarted {
                    runtime_session_id: SessionId::try_from(rsid).unwrap(),
                })]
            }
            Some("reply") => {
                let text = v
                    .get("text")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_owned();
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

    fn on_stderr_line(&mut self, _line: &[u8], _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_truncated(&mut self, _bytes: usize, _now: Instant) -> Vec<Output> {
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

    fn next_deadline(&self) -> Option<Instant> {
        None
    }

    fn control(&mut self, cmd: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        if self.closed {
            return Err(ControlError::Closed);
        }
        match cmd {
            Control::Send { text, .. } => {
                if !self.ready {
                    return Err(ControlError::NotReady);
                }
                let line = encode_line(&serde_json::json!({"cmd": "turn", "text": text}))
                    .map_err(|_| ControlError::Closed)?;
                Ok(vec![
                    Output::Event(SessionEvent::TurnStarted {
                        turn_id: Some("t1".into()),
                        solicited: true,
                    }),
                    Output::Write(line),
                ])
            }
            Control::Close => {
                self.closed = true;
                Ok(vec![Output::CloseStdin])
            }
            _ => Ok(Vec::new()),
        }
    }
}

impl AgentRuntime for ResumeTrackingRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }

    fn binary_name(&self) -> &'static str {
        "tracking-scripted-agent"
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
        self.observed_resumes
            .lock()
            .unwrap()
            .push(ctx.resume.as_ref().map(|s| s.as_str().to_owned()));
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
                runtime_session_id: ctx.resume.clone(),
            },
            Box::new(TrackingScriptedSession {
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

struct StubSessionSpec<'a> {
    emp_id: &'a str,
    run_id: &'a str,
    rsid: Option<&'a str>,
    state: SessionState,
    pid: u32,
    last_used_at: Instant,
    idle_since: Option<Instant>,
}

struct StubSessionRefs {
    state: Arc<Mutex<crate::pump::SessionRuntimeState>>,
    permit: Arc<Mutex<Option<(String, u64)>>>,
    stopped: Arc<AtomicBool>,
}

fn insert_stub_handle(mgr: &SessionManager, spec: StubSessionSpec<'_>) -> StubSessionRefs {
    let StubSessionSpec {
        emp_id,
        run_id,
        rsid,
        state,
        pid,
        last_used_at,
        idle_since,
    } = spec;
    let (tx, rx) = std::sync::mpsc::channel::<crate::pump::PumpCommand>();
    let stopped = Arc::new(AtomicBool::new(false));
    let stopped_flag = Arc::clone(&stopped);
    let worker = std::thread::spawn(move || {
        while let Ok(cmd) = rx.recv() {
            if matches!(cmd, crate::pump::PumpCommand::Stop { .. }) {
                stopped_flag.store(true, Ordering::SeqCst);
                break;
            }
        }
    });
    let runtime_state = Arc::new(Mutex::new(crate::pump::SessionRuntimeState {
        employee_id: emp_id.to_owned(),
        office_id: mgr.office.info().office_id.clone(),
        state,
        run_id: Some(run_id.to_owned()),
        session_id: Some(format!("sess-{emp_id}")),
        runtime_id: Some("claude".into()),
        runtime_session_id: rsid.map(str::to_owned),
        private_home: "/tmp/home".into(),
        permission_preset: "autonomous_sandboxed".into(),
        sandbox_enabled: true,
        spec_hash: "h".into(),
        watchdog: StallWatchdog::new(Duration::from_secs(60), Instant::now()),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: BackgroundRegistry::default(),
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
        queued_turns: 0,
        activity: Default::default(),
    }));
    let permit = Arc::new(Mutex::new(None));
    mgr.registry.lock().unwrap().live.insert(
        emp_id.to_owned(),
        crate::manager::LiveSessionHandle {
            state: Arc::clone(&runtime_state),
            tx,
            thread: Some(worker),
            connection_id: None,
            route_digest: String::new(),
            oauth_expires_at_ms: None,
            workspace: None,
            active_permit: Arc::clone(&permit),
            supports_mid_turn: false,
            sandbox_plan: crate::manager::SandboxPlan::host_native(),
            partition: None,
        },
    );
    mgr.hibernation
        .set_test_timing(emp_id, run_id, pid, (last_used_at, idle_since));
    StubSessionRefs {
        state: runtime_state,
        permit,
        stopped,
    }
}

fn wait_for_idle(mgr: &SessionManager, owner_id: &str, emp_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(inspected) = mgr.inspect_session(owner_id, Some(emp_id)) {
            if inspected.session.state == SessionState::Idle
                && inspected.session.last_reply.is_some()
            {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("session {emp_id} did not settle to Idle before deadline");
}

#[test]
fn task133_ac1_idle_warm_session_hibernates_after_ttl_and_resumes_transparently() {
    let _ = org_platform::process::harden_self();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let hr_id = office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.kind == org_proto::org::MemberKind::Agent)
        .unwrap()
        .id;

    let bin_dir = tempfile::tempdir().unwrap();
    let script_path = bin_dir.path().join("fake-claude-resume");
    let preserved_rsid = "11111111-2222-4333-8444-555555555555";
    fs::write(
        &script_path,
        format!(
            "#!/bin/sh\n\
             echo '{{\"event\":\"ready\"}}'\n\
             echo '{{\"event\":\"session_started\",\"rsid\":\"{preserved_rsid}\"}}'\n\
             while IFS= read -r line; do\n\
               echo '{{\"event\":\"reply\",\"text\":\"ok\"}}'\n\
             done\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

    let (runtime, observed_resumes) = ResumeTrackingRuntime::new();
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes.register(runtime, Some(script_path.clone()));
    cfg.resource_limits.idle_hibernation_ttl = Some(Duration::from_secs(60));
    let mgr = SessionManager::new(Arc::clone(&office), cfg);

    // Turn 1: cold start with no prior resume id.
    mgr.start_session(
        &owner_id,
        &MemberSessionStartParams {
            command_id: "cmd-hib-1".into(),
            employee_id: hr_id.clone(),
            instruction: Some("first turn".into()),
            workspace: None,
        },
    )
    .unwrap();
    wait_for_idle(&mgr, &owner_id, &hr_id);
    assert_eq!(*observed_resumes.lock().unwrap(), vec![None]);
    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_id));

    // Before TTL expires: session stays warm.
    assert!(mgr.hibernate_idle_sessions(Instant::now()).is_empty());
    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_id));
    assert_eq!(mgr.hibernated_count(), 0);

    // Past TTL: idle session is hibernated cleanly while preserving runtime_session_id.
    let hibernated = mgr.hibernate_idle_sessions(Instant::now() + Duration::from_secs(120));
    assert_eq!(hibernated, vec![hr_id.clone()]);
    assert!(!mgr.registry.lock().unwrap().live.contains_key(&hr_id));
    assert_eq!(mgr.hibernated_count(), 1);
    let persisted = mgr.store().read_active_turn(&hr_id).unwrap().unwrap();
    assert_eq!(
        persisted.runtime_session_id.as_deref(),
        Some(preserved_rsid)
    );

    // Turn 2: next wake transparently resumes with `--resume=<preserved_rsid>`.
    mgr.start_session(
        &owner_id,
        &MemberSessionStartParams {
            command_id: "cmd-hib-2".into(),
            employee_id: hr_id.clone(),
            instruction: Some("second turn after hibernation".into()),
            workspace: None,
        },
    )
    .unwrap();
    wait_for_idle(&mgr, &owner_id, &hr_id);
    assert_eq!(
        *observed_resumes.lock().unwrap(),
        vec![None, Some(preserved_rsid.to_owned())]
    );

    // Explicit stop_session after hibernation clears preserved resume id so next run starts fresh.
    let hibernated_again = mgr.hibernate_idle_sessions(Instant::now() + Duration::from_secs(120));
    assert_eq!(hibernated_again, vec![hr_id.clone()]);
    mgr.stop_session(&hr_id, "explicit_owner_stop").unwrap();
    mgr.start_session(
        &owner_id,
        &MemberSessionStartParams {
            command_id: "cmd-hib-3".into(),
            employee_id: hr_id.clone(),
            instruction: Some("fresh turn after explicit stop".into()),
            workspace: None,
        },
    )
    .unwrap();
    wait_for_idle(&mgr, &owner_id, &hr_id);
    assert_eq!(
        *observed_resumes.lock().unwrap(),
        vec![None, Some(preserved_rsid.to_owned()), None]
    );
    mgr.stop_all("test_done");
}

#[test]
fn task133_active_queued_permit_or_background_sessions_are_never_hibernated_or_evicted() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.resource_limits.idle_hibernation_ttl = Some(Duration::from_secs(1));
    cfg.resource_limits.max_office_rss_bytes = Some(1);
    cfg.resource_limits.memory_sampler = FakeMemorySampler::from_pairs([
        (2001, 500 * 1024 * 1024),
        (2002, 500 * 1024 * 1024),
        (2003, 500 * 1024 * 1024),
        (2004, 500 * 1024 * 1024),
        (2005, 500 * 1024 * 1024),
    ]);
    let mgr = SessionManager::new(Arc::clone(&office), cfg);
    let now = Instant::now();
    let old = now - Duration::from_secs(3600);

    let stop_running = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-running",
            run_id: "r1",
            rsid: Some("rs-1"),
            state: SessionState::Running,
            pid: 2001,
            last_used_at: old,
            idle_since: Some(old),
        },
    )
    .stopped;
    let stop_approval = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-approval",
            run_id: "r2",
            rsid: Some("rs-2"),
            state: SessionState::WaitingApproval,
            pid: 2002,
            last_used_at: old,
            idle_since: Some(old),
        },
    )
    .stopped;
    let queued = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-queued",
            run_id: "r3",
            rsid: Some("rs-3"),
            state: SessionState::Idle,
            pid: 2003,
            last_used_at: old,
            idle_since: Some(old),
        },
    );
    queued.state.lock().unwrap().queued_turns = 1;
    let stop_queued = queued.stopped;
    let with_permit = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-permit",
            run_id: "r4",
            rsid: Some("rs-4"),
            state: SessionState::Idle,
            pid: 2004,
            last_used_at: old,
            idle_since: Some(old),
        },
    );
    *with_permit.permit.lock().unwrap() = Some(("perm-1".into(), 1));
    let stop_permit = with_permit.stopped;
    let with_bg = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-bg",
            run_id: "r5",
            rsid: Some("rs-5"),
            state: SessionState::Idle,
            pid: 2005,
            last_used_at: old,
            idle_since: Some(old),
        },
    );
    with_bg.state.lock().unwrap().backgrounds.register(
        "bg-1",
        BackgroundKind::Subagent,
        "helper",
        Some(9999),
        "2026-11-27T00:00:00Z",
    );
    let stop_bg = with_bg.stopped;

    assert!(mgr
        .hibernate_idle_sessions(now + Duration::from_secs(600))
        .is_empty());
    assert!(mgr
        .enforce_rss_budget_at(now + Duration::from_secs(600))
        .is_empty());
    assert_eq!(mgr.hibernated_count(), 0);
    assert_eq!(mgr.rss_evicted_count(), 0);
    for stopped in [
        &stop_running,
        &stop_approval,
        &stop_queued,
        &stop_permit,
        &stop_bg,
    ] {
        assert!(!stopped.load(Ordering::SeqCst));
    }
    assert_eq!(mgr.registry.lock().unwrap().live.len(), 5);
    mgr.stop_all("test_done");
}

#[test]
fn task133_ac2_rss_budget_evicts_idle_sessions_in_strict_lru_order_and_exposes_snapshot() {
    let mib = 1024 * 1024_u64;
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.resource_limits.idle_hibernation_ttl = None;
    cfg.resource_limits.max_office_rss_bytes = Some(250 * mib);
    cfg.resource_limits.memory_sampler = FakeMemorySampler::from_pairs([
        (3001, 100 * mib),
        (3002, 100 * mib),
        (3003, 100 * mib),
        (3004, 100 * mib),
    ]);
    let mgr = SessionManager::new(Arc::clone(&office), cfg);
    let base = Instant::now() - Duration::from_secs(120);

    let stop_active = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-active",
            run_id: "ra",
            rsid: Some("rs-a"),
            state: SessionState::Running,
            pid: 3001,
            last_used_at: base,
            idle_since: None,
        },
    )
    .stopped;
    let stop_oldest = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-idle-oldest",
            run_id: "r1",
            rsid: Some("rs-1"),
            state: SessionState::Idle,
            pid: 3002,
            last_used_at: base + Duration::from_secs(10),
            idle_since: Some(base + Duration::from_secs(10)),
        },
    )
    .stopped;
    let stop_mid = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-idle-mid",
            run_id: "r2",
            rsid: Some("rs-2"),
            state: SessionState::Idle,
            pid: 3003,
            last_used_at: base + Duration::from_secs(20),
            idle_since: Some(base + Duration::from_secs(20)),
        },
    )
    .stopped;
    let stop_newest = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-idle-newest",
            run_id: "r3",
            rsid: Some("rs-3"),
            state: SessionState::Idle,
            pid: 3004,
            last_used_at: base + Duration::from_secs(30),
            idle_since: Some(base + Duration::from_secs(30)),
        },
    )
    .stopped;

    let before = mgr.office_session_rss_snapshot();
    assert_eq!(before.total_rss_bytes, 400 * mib);
    assert_eq!(before.budget_bytes, Some(250 * mib));
    assert_eq!(before.per_session_rss.len(), 4);
    assert_eq!(before.hibernated_total, 0);
    assert_eq!(before.rss_evicted_total, 0);

    let evicted = mgr.enforce_rss_budget_at(Instant::now());
    assert_eq!(
        evicted,
        vec!["emp-idle-oldest".to_owned(), "emp-idle-mid".to_owned()]
    );
    assert!(stop_oldest.load(Ordering::SeqCst));
    assert!(stop_mid.load(Ordering::SeqCst));
    assert!(
        !stop_newest.load(Ordering::SeqCst),
        "newest idle session must stay warm once under budget"
    );
    assert!(
        !stop_active.load(Ordering::SeqCst),
        "active session must never be RSS-evicted"
    );

    // Both evicted sessions retain their runtime_session_id for transparent resume.
    assert_eq!(
        mgr.hibernation
            .matching_resume_id("emp-idle-oldest", "claude", "h", 0)
            .as_deref(),
        Some("rs-1")
    );
    assert_eq!(
        mgr.hibernation
            .matching_resume_id("emp-idle-mid", "claude", "h", 0)
            .as_deref(),
        Some("rs-2")
    );

    let after = mgr.office_session_rss_snapshot();
    assert_eq!(after.total_rss_bytes, 200 * mib);
    assert_eq!(after.per_session_rss.len(), 2);
    assert_eq!(after.rss_evicted_total, 2);
    assert_eq!(after.hibernated_total, 0);
    mgr.stop_all("test_done");
}

#[test]
fn task133_ac3_resource_limits_env_parsing() {
    let mib = 1024 * 1024_u64;
    let parsed = ResourceLimitsConfig::from_env_lookup(|k| match k {
        "ORG_SESSION_IDLE_TTL_SECS" => Some("300".into()),
        "ORG_OFFICE_MAX_RSS_MIB" => Some("1536".into()),
        "ORG_AGENT_BUILD_JOBS" => Some("64".into()),
        _ => None,
    });
    assert_eq!(parsed.idle_hibernation_ttl, Some(Duration::from_secs(300)));
    assert_eq!(parsed.max_office_rss_bytes, Some(1536 * mib));
    assert_eq!(parsed.build_concurrency_jobs, 16);

    let disabled_and_low = ResourceLimitsConfig::from_env_lookup(|k| match k {
        "ORG_SESSION_IDLE_TTL_SECS" => Some("off".into()),
        "ORG_OFFICE_MAX_RSS_MIB" => Some("0".into()),
        "ORG_AGENT_BUILD_JOBS" => Some("0".into()),
        _ => None,
    });
    assert_eq!(disabled_and_low.idle_hibernation_ttl, None);
    assert_eq!(disabled_and_low.max_office_rss_bytes, None);
    assert_eq!(disabled_and_low.build_concurrency_jobs, 1);
}

#[test]
fn task133_ac3_child_build_concurrency_enforcement_and_clamping() {
    let _ = org_platform::process::harden_self();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let hr_id = office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.kind == org_proto::org::MemberKind::Agent)
        .unwrap()
        .id;

    let bin_dir = tempfile::tempdir().unwrap();
    let script_path = bin_dir.path().join("fake-claude-env");
    fs::write(
        &script_path,
        "#!/bin/sh\n\
         echo '{\"event\":\"ready\"}'\n\
         while IFS= read -r line; do\n\
           printf '{\"event\":\"reply\",\"text\":\"CBJ=%s|RNT=%s|MF=%s|NCJ=%s\"}\\n' \
             \"$CARGO_BUILD_JOBS\" \"$RAYON_NUM_THREADS\" \"$MAKEFLAGS\" \"$NPM_CONFIG_JOBS\"\n\
         done\n",
    )
    .unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

    let sock_dir = tempfile::tempdir().unwrap();
    for (idx, jobs, expected) in [
        (1, 3_u32, "CBJ=3|RNT=3|MF=-j3|NCJ=3"),
        (2, 99_u32, "CBJ=16|RNT=16|MF=-j16|NCJ=16"),
    ] {
        let (runtime, _) = ResumeTrackingRuntime::new();
        let mut cfg =
            SessionManagerConfig::production(sock_dir.path().join(format!("o{idx}.sock")));
        cfg.runtimes.register(runtime, Some(script_path.clone()));
        cfg.resource_limits.build_concurrency_jobs = jobs;
        let mgr = SessionManager::new(Arc::clone(&office), cfg);
        mgr.start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: format!("cmd-env-{idx}"),
                employee_id: hr_id.clone(),
                instruction: Some("print build env".into()),
                workspace: None,
            },
        )
        .unwrap();
        wait_for_idle(&mgr, &owner_id, &hr_id);
        let reply = mgr
            .inspect_session(&owner_id, Some(&hr_id))
            .unwrap()
            .session
            .last_reply
            .unwrap();
        assert_eq!(reply, expected);
        mgr.stop_all("test_done");
    }
}

#[test]
fn task133_pre_launch_sweep_evicts_over_budget_idle_session() {
    let _ = org_platform::process::harden_self();
    let mib = 1024 * 1024_u64;
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let hr_id = office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.kind == org_proto::org::MemberKind::Agent)
        .unwrap()
        .id;

    let bin_dir = tempfile::tempdir().unwrap();
    let script_path = bin_dir.path().join("fake-claude-sweep");
    fs::write(
        &script_path,
        "#!/bin/sh\n\
         echo '{\"event\":\"ready\"}'\n\
         while IFS= read -r line; do\n\
           echo '{\"event\":\"reply\",\"text\":\"ok\"}'\n\
         done\n",
    )
    .unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

    let (runtime, _) = ResumeTrackingRuntime::new();
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes.register(runtime, Some(script_path));
    cfg.resource_limits.idle_hibernation_ttl = None;
    cfg.resource_limits.max_office_rss_bytes = Some(100 * mib);
    cfg.resource_limits.memory_sampler = FakeMemorySampler::from_pairs([(4001, 200 * mib)]);
    let mgr = SessionManager::new(Arc::clone(&office), cfg);

    let old = Instant::now() - Duration::from_secs(60);
    let idle_stopped = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-old-idle",
            run_id: "r-old",
            rsid: Some("rs-old"),
            state: SessionState::Idle,
            pid: 4001,
            last_used_at: old,
            idle_since: Some(old),
        },
    )
    .stopped;
    mgr.start_session(
        &owner_id,
        &MemberSessionStartParams {
            command_id: "cmd-prelaunch-sweep".into(),
            employee_id: hr_id.clone(),
            instruction: Some("launch triggers pre-launch RSS sweep".into()),
            workspace: None,
        },
    )
    .unwrap();
    wait_for_idle(&mgr, &owner_id, &hr_id);
    assert!(
        idle_stopped.load(Ordering::SeqCst),
        "pre-launch sweep must evict over-budget idle session"
    );
    assert_eq!(mgr.rss_evicted_count(), 1);
    mgr.stop_all("test_done");
}

#[test]
fn task133_periodic_reaper_hibernates_and_shutdown_joins_sweeper() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = tempfile::tempdir().unwrap();
    let mut periodic_cfg = SessionManagerConfig::production(sock_dir.path().join("office3.sock"));
    periodic_cfg.resource_limits.idle_hibernation_ttl = Some(Duration::from_millis(20));
    periodic_cfg.resource_limits.sweep_interval = Duration::from_millis(30);
    let periodic_mgr = Arc::new(SessionManager::new(Arc::clone(&office), periodic_cfg));
    SessionManager::attach_self(&periodic_mgr);
    assert!(periodic_mgr.hibernation.has_sweeper_thread());
    let past = Instant::now() - Duration::from_secs(5);
    let periodic_stopped = insert_stub_handle(
        &periodic_mgr,
        StubSessionSpec {
            emp_id: "emp-periodic",
            run_id: "r-per",
            rsid: Some("rs-per"),
            state: SessionState::Idle,
            pid: 4002,
            last_used_at: past,
            idle_since: Some(past),
        },
    )
    .stopped;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !periodic_stopped.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(periodic_stopped.load(Ordering::SeqCst));
    assert_eq!(periodic_mgr.hibernated_count(), 1);
    periodic_mgr.shutdown("test_done");
    assert!(
        !periodic_mgr.hibernation.has_sweeper_thread(),
        "shutdown() must wake and join the periodic sweeper thread"
    );
    assert_eq!(periodic_mgr.registry.lock().unwrap().bg_spawning, 0);
}

#[test]
fn task133_hibernation_fails_closed_when_teardown_panics() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.resource_limits.idle_hibernation_ttl = Some(Duration::from_secs(1));
    let mgr = SessionManager::new(Arc::clone(&office), cfg);

    let (tx, rx) = std::sync::mpsc::channel::<crate::pump::PumpCommand>();
    let panicking_worker = std::thread::spawn(move || {
        let _ = rx.recv();
        panic!("simulated teardown panic");
    });
    let state = Arc::new(Mutex::new(crate::pump::SessionRuntimeState {
        employee_id: "emp-panic".into(),
        office_id: office.info().office_id.clone(),
        state: SessionState::Idle,
        run_id: Some("r-panic".into()),
        session_id: Some("s-panic".into()),
        runtime_id: Some("claude".into()),
        runtime_session_id: Some("rs-panic".into()),
        private_home: "/tmp/home".into(),
        permission_preset: "autonomous_sandboxed".into(),
        sandbox_enabled: true,
        spec_hash: "h".into(),
        watchdog: StallWatchdog::new(Duration::from_secs(60), Instant::now()),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: BackgroundRegistry::default(),
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
        queued_turns: 0,
        activity: Default::default(),
    }));
    mgr.registry.lock().unwrap().live.insert(
        "emp-panic".into(),
        crate::manager::LiveSessionHandle {
            state: Arc::clone(&state),
            tx,
            thread: Some(panicking_worker),
            connection_id: None,
            route_digest: String::new(),
            oauth_expires_at_ms: None,
            workspace: None,
            active_permit: Arc::new(Mutex::new(None)),
            supports_mid_turn: false,
            sandbox_plan: crate::manager::SandboxPlan::host_native(),
            partition: None,
        },
    );
    let old = Instant::now() - Duration::from_secs(60);
    mgr.hibernation
        .set_test_timing("emp-panic", "r-panic", 5001, (old, Some(old)));

    let hibernated = mgr.hibernate_idle_sessions(Instant::now());
    assert!(hibernated.is_empty(), "panicking teardown must fail closed");
    assert_eq!(mgr.hibernated_count(), 0);
    assert!(
        mgr.registry.lock().unwrap().live.contains_key("emp-panic"),
        "unverified handle must be restored fail-closed into reg.live"
    );
    assert!(state.lock().unwrap().recovery_required);
    assert_eq!(state.lock().unwrap().state, SessionState::RecoveryRequired);
    mgr.stop_all("test_done");
}

#[test]
fn task133_undrained_background_wakes_and_open_required_asks_block_hibernation() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.resource_limits.idle_hibernation_ttl = Some(Duration::from_secs(10));
    cfg.resource_limits.max_office_rss_bytes = Some(1);
    cfg.resource_limits.memory_sampler =
        FakeMemorySampler::from_pairs([(6001, 50_000), (6002, 50_000)]);
    let mgr = SessionManager::new(Arc::clone(&office), cfg);
    let old = Instant::now() - Duration::from_secs(120);

    // 1. Background task finished (active_count == 0) but pending_wakes is non-empty!
    let bg_refs = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-bg-wake",
            run_id: "r-bg",
            rsid: Some("rs-bg"),
            state: SessionState::Idle,
            pid: 6001,
            last_used_at: old,
            idle_since: Some(old),
        },
    );
    {
        let mut st = bg_refs.state.lock().unwrap();
        st.backgrounds.register(
            "bg-1",
            crate::subagents::BackgroundKind::Subagent,
            "subagent",
            None,
            "2026-11-27T10:00:00Z",
        );
        st.backgrounds
            .complete("bg-1", true, "finished", "2026-11-27T10:01:00Z");
        assert_eq!(st.backgrounds.active_count(), 0);
        assert!(st.backgrounds.has_pending_wakes());
    }

    // 2. Open required ask waiting for suspend persistence.
    let _ask_refs = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-open-ask",
            run_id: "r-ask",
            rsid: Some("rs-ask"),
            state: SessionState::Idle,
            pid: 6002,
            last_used_at: old,
            idle_since: Some(old),
        },
    );
    mgr.open_required_asks.lock().unwrap().insert(
        "emp-open-ask".into(),
        vec![crate::SuspendedAskEntry {
            ask_id: "ask-1".into(),
            conversation_id: "conv-1".into(),
            question_summary: "Need approval".into(),
            waiting_for_member_id: Some("owner".into()),
            waiting_for_name: Some("Owner".into()),
            asked_by_subagent: false,
            opened_at_ms: 1_700_000_000_000,
        }],
    );

    assert!(mgr.hibernate_idle_sessions(Instant::now()).is_empty());
    assert!(mgr.enforce_rss_budget().is_empty());
    assert_eq!(mgr.hibernated_count(), 0);
    assert_eq!(mgr.rss_evicted_count(), 0);
    mgr.stop_all("test_done");
}

#[test]
fn task133_spec_hash_drift_consume_on_attempt_and_stop_all_invalidate_resume() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.resource_limits.idle_hibernation_ttl = Some(Duration::from_secs(5));
    let mgr = SessionManager::new(Arc::clone(&office), cfg);
    let old = Instant::now() - Duration::from_secs(60);

    let refs = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-spec",
            run_id: "r-spec-1",
            rsid: Some("rs-spec-1"),
            state: SessionState::Idle,
            pid: 6101,
            last_used_at: old,
            idle_since: Some(old),
        },
    );
    refs.state.lock().unwrap().spec_hash = "hash-v1".into();
    assert_eq!(
        mgr.hibernate_idle_sessions(Instant::now()),
        vec!["emp-spec".to_owned()]
    );

    // Drifted spec_hash is refused AND consumes the saved resume entry (no poison pill).
    assert_eq!(
        mgr.hibernation
            .matching_resume_id("emp-spec", "claude", "hash-v2", 0),
        None
    );
    assert_eq!(
        mgr.hibernation
            .matching_resume_id("emp-spec", "claude", "hash-v1", 0),
        None,
        "mismatched resume attempt must consume entry"
    );

    // Matching spec_hash returns the ID once and consumes it.
    let refs2 = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-spec-ok",
            run_id: "r-spec-2",
            rsid: Some("rs-spec-2"),
            state: SessionState::Idle,
            pid: 6102,
            last_used_at: old,
            idle_since: Some(old),
        },
    );
    refs2.state.lock().unwrap().spec_hash = "hash-match".into();
    assert_eq!(
        mgr.hibernate_idle_sessions(Instant::now()),
        vec!["emp-spec-ok".to_owned()]
    );
    assert_eq!(
        mgr.hibernation
            .matching_resume_id("emp-spec-ok", "claude", "hash-match", 0),
        Some("rs-spec-2".into())
    );
    assert_eq!(
        mgr.hibernation
            .matching_resume_id("emp-spec-ok", "claude", "hash-match", 0),
        None
    );

    // `stop_all` clears any remaining hibernated entries.
    let refs3 = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-stopall",
            run_id: "r-spec-3",
            rsid: Some("rs-spec-3"),
            state: SessionState::Idle,
            pid: 6103,
            last_used_at: old,
            idle_since: Some(old),
        },
    );
    refs3.state.lock().unwrap().spec_hash = "hash-stopall".into();
    assert_eq!(
        mgr.hibernate_idle_sessions(Instant::now()),
        vec!["emp-stopall".to_owned()]
    );
    mgr.stop_all("test_clear");
    assert_eq!(
        mgr.hibernation
            .matching_resume_id("emp-stopall", "claude", "hash-stopall", 0),
        None
    );
}

#[test]
fn task133_toctou_activity_during_rss_sample_and_turn_idle_update_protect_lru_session() {
    struct ActivityInjectingSampler {
        mgr: Mutex<Option<std::sync::Weak<SessionManager>>>,
        target_emp: &'static str,
        rss_bytes: u64,
    }
    impl ProcessMemorySampler for ActivityInjectingSampler {
        fn process_tree_rss_bytes(&self, _root_pid: u32) -> Option<u64> {
            if let Some(mgr) = self.mgr.lock().unwrap().as_ref().and_then(|w| w.upgrade()) {
                // Simulate a warm turn arriving and settling idle right during unlocked RSS sampling!
                let fresh = Instant::now();
                mgr.hibernation.note_activity(self.target_emp, fresh);
                mgr.hibernation.note_turn_idle(self.target_emp, fresh);
            }
            Some(self.rss_bytes)
        }
    }

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = tempfile::tempdir().unwrap();
    let sampler = Arc::new(ActivityInjectingSampler {
        mgr: Mutex::new(None),
        target_emp: "emp-raced",
        rss_bytes: 500 * 1024 * 1024,
    });
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.resource_limits.idle_hibernation_ttl = None;
    cfg.resource_limits.max_office_rss_bytes = Some(100 * 1024 * 1024);
    cfg.resource_limits.memory_sampler = Arc::clone(&sampler) as Arc<dyn ProcessMemorySampler>;
    let mgr = Arc::new(SessionManager::new(Arc::clone(&office), cfg));
    *sampler.mgr.lock().unwrap() = Some(Arc::downgrade(&mgr));

    let old = Instant::now() - Duration::from_secs(120);
    let raced = insert_stub_handle(
        &mgr,
        StubSessionSpec {
            emp_id: "emp-raced",
            run_id: "r-raced",
            rsid: Some("rs-raced"),
            state: SessionState::Idle,
            pid: 6201,
            last_used_at: old,
            idle_since: Some(old),
        },
    );

    // Because activity advanced `last_used_at` while RSS sampling ran unlocked, eviction MUST abort!
    let evicted = mgr.enforce_rss_budget_at(Instant::now());
    assert!(
        evicted.is_empty(),
        "raced active session must not be evicted: {evicted:?}"
    );
    assert!(!raced.stopped.load(Ordering::SeqCst));
    assert_eq!(mgr.rss_evicted_count(), 0);
    mgr.stop_all("test_done");
}
