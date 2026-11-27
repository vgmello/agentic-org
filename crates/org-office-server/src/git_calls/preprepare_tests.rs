//! End-to-end integration & negative security tests for `TASK-135` AC #4 (auto-pre-prepare on
//! task assignment/claim before Turn 1) and `TASK-160` (mid-turn `worktree.prepare` RPC sandbox
//! refresh signaling & isolation).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_agent_runtime::{
    encode_line, AgentRuntime, Capabilities, Control, ControlError, CostBasis, ExitInfo,
    FileLocation, LaunchContext, LaunchPlan, Output, PlanError, PlannedFile, Probe, ProbeContext,
    ProbeOutput, ProbeResult, ProbeRun, RelPath, RuntimeHostProfile, RuntimeId, Session,
    SessionEvent, SessionId, SessionSpec, SignalTarget, Stdio, StopPlan, StopSignal, StopStep,
    TurnOutcome, Unavailable, UsageScope, VersionRange,
};
use org_office::git::OfficeGitState;
use org_office::{EngineChoice, Office, TestOffice};
use org_office_sessions::{
    RuntimeRegistry, SessionManager, SessionManagerConfig, TaskWorktreePreparer, WakeKind,
    MID_TURN_SANDBOX_REFRESH_HINT,
};
use org_proto::git::{
    RepoAttachParams, RepoRegisterParams, RepositoryKindWire, WorktreePrepareParams,
};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::org::{MemberAddPersonParams, ProjectCreateParams, TeamCreateParams};
use org_proto::sessions::MemberSessionStartParams;
use org_proto::tasks::{
    PatchField, TaskStatus, TasksAssign, TasksAssignParams, TasksCreate, TasksCreateParams,
    TasksTransitionParams,
};
use org_proto::{Outcome, Request};

use super::GitService;
use crate::live::LiveEvents;
use crate::task_calls::TaskService;

fn run_git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn seed_bare_origin(dir: &Path, name: &str) -> PathBuf {
    let bare = dir.join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "--initial-branch=main"]);
    let seed = dir.join(format!("seed-{name}"));
    std::fs::create_dir_all(&seed).unwrap();
    run_git(&seed, &["init", "--initial-branch=main"]);
    run_git(&seed, &["config", "user.name", "Seed"]);
    run_git(&seed, &["config", "user.email", "seed@example.test"]);
    std::fs::write(seed.join("README.md"), format!("# {name}\n")).unwrap();
    run_git(&seed, &["add", "README.md"]);
    run_git(&seed, &["commit", "-m", "initial"]);
    run_git(&seed, &["remote", "add", "origin", bare.to_str().unwrap()]);
    run_git(&seed, &["push", "origin", "main"]);
    bare
}

fn test_uuid(n: u64) -> String {
    format!("018bce99-1600-7000-8000-{n:012x}")
}

fn hire_sandboxed_agent(
    office: &Office,
    owner: &str,
    team_id: &str,
    handle: &str,
    seq_base: u64,
) -> (String, String) {
    let conn_id = test_uuid(seq_base);
    let prof_id = test_uuid(seq_base + 1);
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: test_uuid(seq_base + 2),
            connection_id: conn_id.clone(),
            default_profile_id: prof_id.clone(),
            key: format!("{handle}-conn"),
            name: format!("{handle} conn"),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    assert!(matches!(
        office
            .dispatch_hiring_rpc(Some(owner), &conn_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));
    office
        .provider_binding_set(
            owner,
            org_proto::providers::ProviderBindingSetParams {
                operation_id: test_uuid(seq_base + 3),
                connection_id: conn_id.clone(),
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
            owner,
            org_proto::providers::ProviderConnectionRefreshParams {
                connection_id: conn_id.clone(),
                driver_version: Some("2.1.311".into()),
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

    let hire_req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: test_uuid(seq_base + 4),
            request_id: test_uuid(seq_base + 5),
            employee_id: test_uuid(seq_base + 6),
            home_team_id: team_id.to_owned(),
            role_id: None,
            proposal: HireProposalWire {
                name: format!("Agent {handle}"),
                handle: handle.into(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner.to_owned()),
                office_id: None,
                model_profile_id: Some(prof_id),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("autonomous_sandboxed".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: Some("Work safely.".into()),
                effort: None,
            },
            reason: "Test hire".into(),
        },
    )
    .unwrap();
    let hired: org_proto::hiring::HireOpResult = match office
        .dispatch_hiring_rpc(Some(owner), &hire_req)
        .unwrap()
        .outcome
    {
        Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        Outcome::Error(e) => panic!("hire failed: {e:?}"),
    };
    (hired.employee_id.unwrap(), conn_id)
}

fn verify_real_git_workflow(wt_path: &Path, filename: &str) {
    let status = run_git(wt_path, &["status", "--porcelain"]);
    assert!(status.trim().is_empty(), "fresh worktree must be clean");
    std::fs::write(wt_path.join(filename), "change-160\n").unwrap();
    let dirty = run_git(wt_path, &["status", "--porcelain"]);
    assert!(
        dirty.contains(filename),
        "git status must see edit: {dirty}"
    );
    run_git(wt_path, &["add", filename]);
    let diff = run_git(wt_path, &["diff", "--cached"]);
    assert!(diff.contains("change-160"), "git diff must see staged edit");
    run_git(
        wt_path,
        &[
            "-c",
            "user.name=Agent",
            "-c",
            "user.email=agent@example.test",
            "commit",
            "-m",
            "agent commit",
        ],
    );
}

struct StubSandboxedRuntime;

struct StubSandboxedSession {
    runtime_session_id: String,
}

impl Session for StubSandboxedSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }
    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("ready") => vec![
                Output::Event(SessionEvent::SessionStarted {
                    runtime_session_id: SessionId::try_from(self.runtime_session_id.as_str())
                        .unwrap(),
                }),
                Output::Event(SessionEvent::Ready {
                    runtime_version: Some("1.0.0".into()),
                    pid: None,
                    agents: vec!["self-eng".into()],
                    permission_mode: Some("default".into()),
                }),
            ],
            Some("reply") => vec![
                Output::Event(SessionEvent::AssistantText {
                    message_id: Some("m1".into()),
                    text: "ok".into(),
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

impl AgentRuntime for StubSandboxedRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }
    fn binary_name(&self) -> &'static str {
        "stub-sandboxed"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["anthropic", "claude-code"]
    }
    fn auth_env_var(&self, _subscription_cli: bool, _secret: &str) -> Option<&'static str> {
        Some("ANTHROPIC_API_KEY")
    }
    fn host_profile(&self) -> RuntimeHostProfile {
        RuntimeHostProfile {
            host_config_subdir: None,
            config_credential_files: &[],
            host_credential_files: &[],
            uses_host_oauth_token_injection: false,
            uses_mount_mask_sandbox: false,
            needs_explicit_sandbox_ro_dirs: false,
            sandbox_settings_file: Some("stub-settings.json"),
        }
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
            enforces_writable_roots: true,
            enforces_deny_write: true,
            enforces_deny_read: true,
            enforces_unix_socket_allowlist: false,
            enforces_tool_allowlist: false,
            enforces_deny_rules: true,
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
            .unwrap_or_else(|| "00000000-0000-0000-0000-000000000960".into());
        let writable: Vec<&str> = spec
            .sandbox
            .writable_roots
            .iter()
            .map(|p| p.as_str())
            .collect();
        let deny_read: Vec<&str> = spec.sandbox.deny_read.iter().map(|p| p.as_str()).collect();
        let contents = serde_json::to_vec(&serde_json::json!({
            "sandbox": {
                "enabled": spec.sandbox.enabled,
                "filesystem": {
                    "allowWrite": writable,
                    "denyRead": deny_read,
                }
            }
        }))
        .unwrap();
        let plan = LaunchPlan {
            program: ctx.runtime_path.clone(),
            args: Vec::new(),
            cwd: spec.workspace.clone(),
            stdio: Stdio::Piped,
            files: vec![PlannedFile {
                location: FileLocation::RuntimeDir,
                rel_path: RelPath::try_from("stub-settings.json").unwrap(),
                contents,
                git_exclude: false,
            }],
            dirs: Vec::new(),
            env_add: Vec::new(),
            auth_env_names: vec![org_agent_runtime::EnvName::try_from("ANTHROPIC_API_KEY").unwrap()],
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
            Box::new(StubSandboxedSession {
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

struct TestHarness {
    office: Arc<Office>,
    owner: String,
    project_id: String,
    repo1_id: String,
    repo2_id: String,
    origin1_dir: PathBuf,
    self_agent: String,
    self_conn: String,
    peer_agent: String,
    sessions: Arc<SessionManager>,
    git: Arc<GitService>,
    tasks: TaskService,
    _origins_tmp: tempfile::TempDir,
    _sock_tmp: tempfile::TempDir,
}

fn build_harness(fake_claude: Option<PathBuf>, wire_sessions_on_tasks: bool) -> TestHarness {
    org_office_sqlite::install();
    let office = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    let team_id = test_uuid(10);
    let project_id = test_uuid(11);
    office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::TeamCreate(TeamCreateParams {
                command_id: test_uuid(12),
                team_id: team_id.clone(),
                name: "Backend".into(),
                owner_team_id: Some(root_team),
            }),
        )
        .unwrap()
        .unwrap();
    let proj_params: ProjectCreateParams = serde_json::from_value(serde_json::json!({
        "command_id": test_uuid(13),
        "project_id": project_id,
        "name": "Core",
        "slug": "core",
        "owner_team_id": team_id,
        "contributing_team_ids": [],
    }))
    .unwrap();
    office
        .submit_org(
            &owner,
            org_office::rules::OrgCommand::ProjectCreate(proj_params),
        )
        .unwrap()
        .unwrap();

    let (self_agent, self_conn) = hire_sandboxed_agent(&office, &owner, &team_id, "self-eng", 100);
    let (peer_agent, _) = hire_sandboxed_agent(&office, &owner, &team_id, "peer-eng", 200);

    let sock_tmp = tempfile::tempdir().unwrap();
    let mut registry = RuntimeRegistry::empty();
    registry.register(Arc::new(StubSandboxedRuntime), fake_claude);
    let mut cfg =
        SessionManagerConfig::with_runtimes(sock_tmp.path().join("office.sock"), registry);
    cfg.helper_dir = Some(sock_tmp.path().to_path_buf());
    let sessions = Arc::new(SessionManager::new(Arc::clone(&office), cfg));
    SessionManager::attach_self(&sessions);

    let live = Arc::new(LiveEvents::for_owner(Box::new(|| {}), owner.clone()));
    let git = Arc::new(
        GitService::new(Arc::clone(&office), Arc::clone(&live))
            .with_sessions(Arc::downgrade(&sessions)),
    );
    sessions.set_task_worktree_preparer(Arc::clone(&git) as Arc<dyn TaskWorktreePreparer>);
    let tasks = TaskService::new(
        Arc::clone(&office),
        live,
        wire_sessions_on_tasks.then(|| Arc::clone(&sessions)),
    )
    .with_git_service(Arc::clone(&git));

    let origins_tmp = tempfile::tempdir().unwrap();
    let origin1_dir = seed_bare_origin(origins_tmp.path(), "r1");
    let origin2_dir = seed_bare_origin(origins_tmp.path(), "r2");
    let repo1_id = test_uuid(301);
    let repo2_id = test_uuid(302);
    for (rid, cmd_seq, key, origin) in [
        (&repo1_id, 310, "r1", &origin1_dir),
        (&repo2_id, 311, "r2", &origin2_dir),
    ] {
        git.handle_register(
            &owner,
            RepoRegisterParams {
                command_id: test_uuid(cmd_seq),
                repository_id: rid.clone(),
                key: Some(key.into()),
                kind: RepositoryKindWire::Local,
                remote_url: None,
                origin_path: Some(origin.to_string_lossy().into_owned()),
            },
        )
        .expect("register repo");
    }
    git.handle_attach(
        &owner,
        RepoAttachParams {
            command_id: test_uuid(312),
            project: "core".into(),
            repository: "r1".into(),
            integration_branch: "main".into(),
        },
    )
    .expect("attach r1 to project");

    TestHarness {
        office,
        owner,
        project_id,
        repo1_id,
        repo2_id,
        origin1_dir,
        self_agent,
        self_conn,
        peer_agent,
        sessions,
        git,
        tasks,
        _origins_tmp: origins_tmp,
        _sock_tmp: sock_tmp,
    }
}

#[test]
fn task135_assign_and_self_claim_auto_preprepare_worktrees_and_allow_real_git_workflow() {
    let h = build_harness(None, false);

    // 1. Creating a task assigned to `self_agent` auto-pre-prepares its worktree immediately!
    let t1_id = test_uuid(401);
    let create_req = Request::new::<TasksCreate>(
        1,
        &TasksCreateParams {
            command_id: test_uuid(402),
            task_id: t1_id.clone(),
            team_id: None,
            project_id: Some(h.project_id.clone()),
            assigned_team_id: None,
            assignee_id: Some(h.self_agent.clone()),
            task_type: Some("task".into()),
            work_type: None,
            title: "Pre-prepare on assign".into(),
            body_markdown: "Verify pre-prepare".into(),
            priority: None,
            effort: None,
            severity: None,
            rank_order: Some(1),
            labels: vec![],
            reference_files: vec![],
            modified_files: vec![],
            depends_on: vec![],
            details: None,
            dod: None,
            attachment_ids: vec![],
        },
    )
    .unwrap();
    let resp = h.tasks.dispatch(Some(&h.owner), &create_req).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)));

    let state = OfficeGitState::load(h.office.runtime_root(), &h.office.info().office_id).unwrap();
    let rec = state
        .workspaces
        .get(&OfficeGitState::workspace_key(
            &t1_id,
            &h.repo1_id,
            &h.self_agent,
        ))
        .expect("assigning repo-linked task must auto-pre-prepare worktree in git-state.json");
    assert_eq!(rec.state, "active");
    let wt_path = PathBuf::from(&rec.worktree_path);
    assert!(wt_path.join(".git").is_file());
    verify_real_git_workflow(&wt_path, "preprepared.txt");

    // 2. Unassigned task does NOT pre-prepare; self-claiming via `tasks.assign` DOES!
    let t2_id = test_uuid(410);
    let create_unassigned = Request::new::<TasksCreate>(
        2,
        &TasksCreateParams {
            command_id: test_uuid(411),
            task_id: t2_id.clone(),
            team_id: None,
            project_id: Some(h.project_id.clone()),
            assigned_team_id: None,
            assignee_id: None,
            task_type: Some("task".into()),
            work_type: None,
            title: "Self-claim pre-prepare".into(),
            body_markdown: "Claimed by agent".into(),
            priority: None,
            effort: None,
            severity: None,
            rank_order: Some(2),
            labels: vec![],
            reference_files: vec![],
            modified_files: vec![],
            depends_on: vec![],
            details: None,
            dod: None,
            attachment_ids: vec![],
        },
    )
    .unwrap();
    assert!(matches!(
        h.tasks
            .dispatch(Some(&h.owner), &create_unassigned)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));
    let before_claim =
        OfficeGitState::load(h.office.runtime_root(), &h.office.info().office_id).unwrap();
    assert!(
        !before_claim.workspaces.values().any(|w| w.task_id == t2_id),
        "unassigned task must not pre-prepare any worktree"
    );

    let claim_req = Request::new::<TasksAssign>(
        3,
        &TasksAssignParams {
            command_id: test_uuid(412),
            task: t2_id.clone(),
            assigned_team_id: PatchField::Unchanged,
            assignee_id: PatchField::Set(Some(h.self_agent.clone())),
            expected_revision: 1,
        },
    )
    .unwrap();
    assert!(matches!(
        h.tasks
            .dispatch(Some(&h.self_agent), &claim_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));
    let after_claim =
        OfficeGitState::load(h.office.runtime_root(), &h.office.info().office_id).unwrap();
    let rec2 = after_claim
        .workspaces
        .get(&OfficeGitState::workspace_key(
            &t2_id,
            &h.repo1_id,
            &h.self_agent,
        ))
        .expect("self-claiming repo-linked task must auto-pre-prepare worktree");
    verify_real_git_workflow(Path::new(&rec2.worktree_path), "claimed.txt");

    // 3. Idempotent reuse: even if origin disappears later, pre-prepare reuses active worktree
    // without failing or performing a redundant network fetch.
    std::fs::remove_dir_all(&h.origin1_dir).unwrap();
    let reuse = h
        .git
        .preprepare_task_worktrees_for_assignee(&h.self_agent, &t1_id);
    assert_eq!(reuse.prepared, 0);
    assert_eq!(reuse.reused, 1);
    assert_eq!(reuse.failed, 0);
}

#[test]
fn task135_auto_preprepare_fails_closed_on_unauthorized_closed_projectless_and_broken_origin() {
    let h = build_harness(None, false);

    // Seed a human employee to verify human assignees are skipped.
    let human_id = test_uuid(500);
    let root_team = h.office.org_snapshot().unwrap().teams[0].id.clone();
    h.office
        .submit_org(
            &h.owner,
            org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: test_uuid(501),
                member_id: human_id.clone(),
                name: "Human Dev".into(),
                handle: Some("humandev".into()),
                home_team_id: Some(root_team.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();

    // Create task assigned to `self_agent` via Office directly (bypassing TaskService auto-hook).
    let t_auth = test_uuid(510);
    h.office
        .task_create(
            &h.owner,
            TasksCreateParams {
                command_id: test_uuid(511),
                task_id: t_auth.clone(),
                team_id: None,
                project_id: Some(h.project_id.clone()),
                assigned_team_id: None,
                assignee_id: Some(h.self_agent.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Auth boundary".into(),
                body_markdown: "Only assignee".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();

    // Non-assignee `peer_agent` MUST NOT get a worktree for `self_agent`'s task!
    let unauth = h
        .git
        .preprepare_task_worktrees_for_assignee(&h.peer_agent, &t_auth);
    assert_eq!(unauth.prepared, 0);
    assert_eq!(unauth.reused, 0);
    assert_eq!(unauth.failed, 0);

    // Human member MUST be skipped.
    let human_rep = h
        .git
        .preprepare_task_worktrees_for_assignee(&human_id, &t_auth);
    assert_eq!(human_rep.prepared, 0);

    // Projectless task MUST be skipped.
    let t_projless = test_uuid(520);
    h.office
        .task_create(
            &h.owner,
            TasksCreateParams {
                command_id: test_uuid(521),
                task_id: t_projless.clone(),
                team_id: Some(root_team),
                project_id: None,
                assigned_team_id: None,
                assignee_id: Some(h.self_agent.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "No project".into(),
                body_markdown: "Standalone".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(2),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    let projless_rep = h
        .git
        .preprepare_task_worktrees_for_assignee(&h.self_agent, &t_projless);
    assert_eq!(projless_rep.prepared, 0);

    // Closed (`Done`) task MUST be skipped.
    let t_rev = h
        .office
        .task_transition(
            &h.owner,
            TasksTransitionParams {
                command_id: test_uuid(530),
                task: t_auth.clone(),
                status: TaskStatus::InProgress,
                resolution: None,
                blocked_reason: None,
                note: None,
                expected_revision: 1,
                done_anyway_reason: None,
                final_summary: None,
            },
        )
        .unwrap()
        .unwrap()
        .task
        .revision;
    h.office
        .task_transition(
            &h.owner,
            TasksTransitionParams {
                command_id: test_uuid(531),
                task: t_auth.clone(),
                status: TaskStatus::Done,
                resolution: Some("fixed".into()),
                blocked_reason: None,
                note: None,
                expected_revision: t_rev,
                done_anyway_reason: None,
                final_summary: Some("done".into()),
            },
        )
        .unwrap()
        .unwrap();
    let closed_rep = h
        .git
        .preprepare_task_worktrees_for_assignee(&h.self_agent, &t_auth);
    assert_eq!(closed_rep.prepared, 0);

    // Broken/missing origin BEFORE initial clone MUST fail safely without crashing or leaking!
    std::fs::remove_dir_all(&h.origin1_dir).unwrap();
    let t_broken = test_uuid(540);
    h.office
        .task_create(
            &h.owner,
            TasksCreateParams {
                command_id: test_uuid(541),
                task_id: t_broken.clone(),
                team_id: None,
                project_id: Some(h.project_id.clone()),
                assigned_team_id: None,
                assignee_id: Some(h.self_agent.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Broken origin".into(),
                body_markdown: "Origin vanished".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(3),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    let broken_rep = h
        .git
        .preprepare_task_worktrees_for_assignee(&h.self_agent, &t_broken);
    assert_eq!(broken_rep.prepared, 0);
    assert_eq!(broken_rep.failed, 1);

    let state = OfficeGitState::load(h.office.runtime_root(), &h.office.info().office_id).unwrap();
    assert!(
        state.workspaces.is_empty(),
        "failed pre-prepare must never leave partial/unauthorized records in git-state.json"
    );
}

fn project_task_params(
    cmd_seq: u64,
    task_id: &str,
    project_id: &str,
    assignee: Option<&str>,
    title: &str,
    rank: u64,
) -> TasksCreateParams {
    TasksCreateParams {
        command_id: test_uuid(cmd_seq),
        task_id: task_id.to_owned(),
        team_id: None,
        project_id: Some(project_id.to_owned()),
        assigned_team_id: None,
        assignee_id: assignee.map(str::to_owned),
        task_type: Some("task".into()),
        work_type: None,
        title: title.to_owned(),
        body_markdown: title.to_owned(),
        priority: None,
        effort: None,
        severity: None,
        rank_order: Some(rank),
        labels: vec![],
        reference_files: vec![],
        modified_files: vec![],
        depends_on: vec![],
        details: None,
        dod: None,
        attachment_ids: vec![],
    }
}

fn write_gated_stub_cli(dir: &Path, busy: &Path, release: &Path) -> PathBuf {
    let fake_cli = dir.join("stub-sandboxed");
    let script = format!(
        r#"#!/bin/sh
echo '{{"kind":"ready"}}'
read user_line
touch "{busy}"
while [ ! -f "{rel}" ]; do sleep 0.02; done
echo '{{"kind":"reply"}}'
while IFS= read -r _x; do :; done
"#,
        busy = busy.display(),
        rel = release.display(),
    );
    std::fs::write(&fake_cli, script).unwrap();
    std::fs::set_permissions(&fake_cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    fake_cli
}

fn assert_midturn_settings_isolation(h: &TestHarness, self_task_id: &str, peer_admin: &str) {
    let settings_path = h
        .office
        .durable_root()
        .join("local")
        .join("offices")
        .join(&h.office.info().office_id)
        .join("agents")
        .join(&h.self_agent)
        .join("runtime")
        .join("stub-settings.json");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    let allow_write: Vec<&str> = settings
        .pointer("/sandbox/filesystem/allowWrite")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let deny_read: Vec<&str> = settings
        .pointer("/sandbox/filesystem/denyRead")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let self_state =
        OfficeGitState::load(h.office.runtime_root(), &h.office.info().office_id).unwrap();
    let self_admin = self_state
        .workspaces
        .get(&OfficeGitState::workspace_key(
            self_task_id,
            &h.repo1_id,
            &h.self_agent,
        ))
        .unwrap()
        .admin_dir_name
        .clone();
    assert_ne!(self_admin, peer_admin);
    let self_suffix = format!("/worktrees/{self_admin}");
    let peer_suffix = format!("/worktrees/{peer_admin}");
    assert!(allow_write.iter().any(|p| p.ends_with(&self_suffix)));
    assert!(!allow_write.iter().any(|p| p.ends_with("/worktrees")));
    assert!(!allow_write
        .iter()
        .any(|p| p.ends_with(&peer_suffix) || p.contains(&h.peer_agent)));
    assert!(!allow_write.iter().any(|p| p.contains(&h.repo2_id)));
    assert!(deny_read
        .iter()
        .any(|p| p.ends_with(&peer_suffix) || p.ends_with(&format!("{peer_suffix}/**"))));
    assert!(deny_read.iter().any(|p| p.contains(&h.repo2_id)));
    assert!(!deny_read
        .iter()
        .any(|p| p.ends_with(&self_suffix) || p.ends_with(&format!("{self_suffix}/**"))));
}

#[test]
fn task160_mid_turn_worktree_prepare_rpc_signals_refresh_and_preserves_isolation() {
    let _ = org_platform::process::harden_self();
    let cli_tmp = tempfile::tempdir().unwrap();
    let turn1_busy = cli_tmp.path().join("turn1-busy");
    let turn1_release = cli_tmp.path().join("turn1-release");
    let fake_cli = write_gated_stub_cli(cli_tmp.path(), &turn1_busy, &turn1_release);

    let h = build_harness(Some(fake_cli), true);
    let enrolled = h
        .sessions
        .enrollment()
        .start(&h.owner, &h.self_conn, "claude")
        .unwrap();
    h.sessions
        .enrollment()
        .submit_code(
            &h.owner,
            &h.self_conn,
            &enrolled.login_id,
            "sk-ant-test-960",
        )
        .unwrap();

    // Give `peer_agent` an active worktree on `r1` first so sibling isolation is exercised.
    let t_peer = test_uuid(601);
    h.office
        .task_create(
            &h.owner,
            project_task_params(
                602,
                &t_peer,
                &h.project_id,
                Some(&h.peer_agent),
                "Peer work",
                1,
            ),
        )
        .unwrap()
        .unwrap();
    let peer_res = h
        .git
        .handle_worktree_prepare(
            &h.peer_agent,
            WorktreePrepareParams {
                command_id: test_uuid(603),
                task: t_peer,
                repository: "r1".into(),
                base_branch: None,
            },
        )
        .unwrap();
    assert!(!peer_res.sandbox_refresh_required);
    let peer_admin = OfficeGitState::load(h.office.runtime_root(), &h.office.info().office_id)
        .unwrap()
        .workspaces
        .values()
        .find(|w| w.employee_id == h.peer_agent)
        .unwrap()
        .admin_dir_name
        .clone();

    // Ensure an untargeted `r2` bare cache exists on disk so we prove it stays explicitly denied!
    std::fs::create_dir_all(
        h.office
            .runtime_root()
            .join(&h.office.info().office_id)
            .join("repositories")
            .join(format!("{}.git", h.repo2_id)),
    )
    .unwrap();

    // Start `self_agent` with 0 active worktrees (busy on Turn 1).
    h.sessions
        .start_session(
            &h.owner,
            &MemberSessionStartParams {
                command_id: test_uuid(610),
                employee_id: h.self_agent.clone(),
                instruction: Some("Begin turn with 0 worktrees".into()),
                workspace: None,
            },
        )
        .expect("start busy Turn 1");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !turn1_busy.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(turn1_busy.exists(), "Turn 1 must be active");

    // `self_agent` calls `WorktreePrepare` RPC mid-turn.
    let t_self = test_uuid(620);
    h.office
        .task_create(
            &h.owner,
            project_task_params(
                621,
                &t_self,
                &h.project_id,
                Some(&h.self_agent),
                "Mid-turn prepare",
                2,
            ),
        )
        .unwrap()
        .unwrap();
    let prep = h
        .git
        .handle_worktree_prepare(
            &h.self_agent,
            WorktreePrepareParams {
                command_id: test_uuid(622),
                task: t_self.clone(),
                repository: "r1".into(),
                base_branch: None,
            },
        )
        .expect("mid-turn worktree.prepare succeeds");

    assert!(prep.sandbox_refresh_required);
    assert_eq!(
        prep.sandbox_refresh_hint.as_deref(),
        Some(MID_TURN_SANDBOX_REFRESH_HINT)
    );
    verify_real_git_workflow(Path::new(&prep.worktree.worktree_path), "midturn.txt");
    assert_midturn_settings_isolation(&h, &t_self, &peer_admin);

    let wakes = h
        .sessions
        .store()
        .read_pending_wakes(&h.self_agent)
        .unwrap()
        .expect("pending wake queue present");
    assert!(wakes
        .items
        .iter()
        .any(|w| w.kind == WakeKind::SandboxRefresh));

    std::fs::write(&turn1_release, b"go").unwrap();
    let _ = h.sessions.stop_session(&h.self_agent, "done");
}
