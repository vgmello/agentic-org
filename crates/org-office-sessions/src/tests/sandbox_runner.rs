//! Contract and mock-adapter tests for [`SandboxRunner`], [`SandboxPlan`], and [`LocalOsSandbox`]
//! (`TASK-233`).

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use crate::manager::{
    LocalOsSandbox, SandboxConfinementMode, SandboxPlan, SandboxPrepareContext, SandboxRunner,
    SandboxVerifyContext,
};

struct RecordingSandboxRunner {
    enforce_calls: AtomicUsize,
    prepare_calls: AtomicUsize,
    adapt_calls: AtomicUsize,
    verify_calls: AtomicUsize,
    stale_calls: AtomicUsize,
    fail_enforce: AtomicBool,
    fail_verify: AtomicBool,
    fail_verify_on_call: AtomicUsize,
    stale: AtomicBool,
    adapted_marker: Mutex<Option<PermissionMode>>,
}

impl RecordingSandboxRunner {
    fn new() -> Self {
        Self {
            enforce_calls: AtomicUsize::new(0),
            prepare_calls: AtomicUsize::new(0),
            adapt_calls: AtomicUsize::new(0),
            verify_calls: AtomicUsize::new(0),
            stale_calls: AtomicUsize::new(0),
            fail_enforce: AtomicBool::new(false),
            fail_verify: AtomicBool::new(false),
            fail_verify_on_call: AtomicUsize::new(0),
            stale: AtomicBool::new(false),
            adapted_marker: Mutex::new(None),
        }
    }
}

impl SandboxRunner for RecordingSandboxRunner {
    fn prepare_launch(
        &self,
        _ctx: &SandboxPrepareContext<'_>,
    ) -> Result<SandboxPlan, org_proto::ApiError> {
        self.prepare_calls.fetch_add(1, Ordering::SeqCst);
        Ok(SandboxPlan::host_native().with_prune_absent_denies(true))
    }

    fn adapt_session_spec(&self, plan: &SandboxPlan, spec: &mut SessionSpec) {
        self.adapt_calls.fetch_add(1, Ordering::SeqCst);
        assert!(plan.prunes_absent_denies());
        spec.permission_mode = PermissionMode::Ask;
        *self.adapted_marker.lock().unwrap() = Some(spec.permission_mode);
    }

    fn verify_before_spawn(
        &self,
        _ctx: &SandboxVerifyContext<'_>,
        _plan: &SandboxPlan,
    ) -> Result<(), org_proto::ApiError> {
        let call = self.verify_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_verify.load(Ordering::SeqCst) {
            return Err(org_proto::ApiError::new(
                org_proto::ErrorCode::Conflict,
                "mock sandbox rejected pre-spawn verification",
            ));
        }
        if self.fail_verify_on_call.load(Ordering::SeqCst) == call {
            return Err(org_proto::ApiError::new(
                org_proto::ErrorCode::Unavailable,
                "custom pre-pump verification failure",
            ));
        }
        Ok(())
    }

    fn is_plan_stale(&self, _office: &Office, _employee_id: &str, _plan: &SandboxPlan) -> bool {
        self.stale_calls.fetch_add(1, Ordering::SeqCst);
        self.stale.load(Ordering::SeqCst)
    }

    fn enforce_sibling_isolation(
        &self,
        _mgr: &SessionManager,
        _starting_emp_id: &str,
    ) -> Result<(), org_proto::ApiError> {
        self.enforce_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_enforce.load(Ordering::SeqCst) {
            return Err(org_proto::ApiError::new(
                org_proto::ErrorCode::Conflict,
                "mock sandbox rejected sibling isolation",
            ));
        }
        Ok(())
    }
}

struct CustomSandboxFixture {
    owner_id: String,
    agent_id: String,
    runner: Arc<RecordingSandboxRunner>,
    mgr: SessionManager,
    _bin_dir: tempfile::TempDir,
    _sock_dir: tempfile::TempDir,
}

fn custom_sandbox_fixture() -> CustomSandboxFixture {
    let _ = org_platform::process::harden_self();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (agent_id, _) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "anthropic",
        "claude",
        "sandbox-seam-agent",
        "claude-sonnet-4-6",
    );

    let bin_dir = tempfile::tempdir().unwrap();
    let script_path = bin_dir.path().join("warm-scripted-agent");
    let script = "#!/bin/sh\necho '{\"kind\":\"ready\"}'\nwhile IFS= read -r line; do\n  printf '{\"kind\":\"reply\",\"text\":\"ack\"}\\n'\ndone\n";
    fs::write(&script_path, script).unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

    let mut registry = RuntimeRegistry::production();
    registry.register(Arc::new(ScriptedRuntime), Some(script_path));

    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(bin_dir.path().to_path_buf());

    let runner = Arc::new(RecordingSandboxRunner::new());
    let mgr = SessionManager::with_sandbox_runner(
        Arc::clone(&office),
        cfg,
        Arc::clone(&runner) as Arc<dyn SandboxRunner>,
    );
    CustomSandboxFixture {
        owner_id,
        agent_id,
        runner,
        mgr,
        _bin_dir: bin_dir,
        _sock_dir: sock_dir,
    }
}

fn start_params(employee_id: &str, instruction: &str) -> MemberSessionStartParams {
    MemberSessionStartParams {
        command_id: new_uuid_v7(),
        employee_id: employee_id.to_owned(),
        instruction: Some(instruction.to_owned()),
        workspace: None,
    }
}

fn wait_for_idle(mgr: &SessionManager, owner_id: &str, agent_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if mgr
            .inspect_session(owner_id, Some(agent_id))
            .unwrap()
            .session
            .state
            == SessionState::Idle
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        mgr.inspect_session(owner_id, Some(agent_id))
            .unwrap()
            .session
            .state,
        SessionState::Idle
    );
}

#[test]
fn custom_sandbox_runner_aborts_on_enforce_and_pre_spawn_verify_failures() {
    let fx = custom_sandbox_fixture();

    // 1. Failing `enforce_sibling_isolation` aborts before `prepare_launch`.
    fx.runner.fail_enforce.store(true, Ordering::SeqCst);
    let err = fx
        .mgr
        .start_session(&fx.owner_id, &start_params(&fx.agent_id, "blocked enforce"))
        .unwrap_err();
    assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    assert_eq!(fx.runner.enforce_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fx.runner.prepare_calls.load(Ordering::SeqCst), 0);
    assert!(!fx
        .mgr
        .registry
        .lock()
        .unwrap()
        .starting
        .contains(&fx.agent_id));
    fx.runner.fail_enforce.store(false, Ordering::SeqCst);

    // 2. Failing `verify_before_spawn` aborts launch and leaves no live handle or leaked slot.
    fx.runner.fail_verify.store(true, Ordering::SeqCst);
    let err = fx
        .mgr
        .start_session(&fx.owner_id, &start_params(&fx.agent_id, "blocked verify"))
        .unwrap_err();
    assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    assert_eq!(fx.runner.prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fx.runner.adapt_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fx.runner.verify_calls.load(Ordering::SeqCst), 1);
    assert!(fx.mgr.registry.lock().unwrap().live.is_empty());
    assert!(!fx
        .mgr
        .registry
        .lock()
        .unwrap()
        .starting
        .contains(&fx.agent_id));
}

#[test]
fn custom_sandbox_runner_propagates_pre_pump_verify_error() {
    let fx = custom_sandbox_fixture();

    // Call 1 (pre-spawn in launch.rs) succeeds; call 2 (pre-pump in install.rs) returns a custom
    // `Unavailable` ApiError that must be preserved verbatim on non-shutdown abort.
    fx.runner.fail_verify_on_call.store(2, Ordering::SeqCst);
    let err = fx
        .mgr
        .start_session(
            &fx.owner_id,
            &start_params(&fx.agent_id, "blocked pre-pump verify"),
        )
        .unwrap_err();
    assert_eq!(err.code, org_proto::ErrorCode::Unavailable);
    assert_eq!(err.message, "custom pre-pump verification failure");
    assert_eq!(fx.runner.verify_calls.load(Ordering::SeqCst), 2);
    assert!(fx.mgr.registry.lock().unwrap().live.is_empty());
    assert!(!fx
        .mgr
        .registry
        .lock()
        .unwrap()
        .starting
        .contains(&fx.agent_id));
}

#[test]
fn custom_sandbox_runner_adapts_spec_and_recycles_stale_warm_handle() {
    let fx = custom_sandbox_fixture();

    // 1. Happy path flows through enforce -> prepare -> adapt -> verify (pre-spawn + pre-pump)
    //    -> is_plan_stale (registration).
    let started = fx
        .mgr
        .start_session(&fx.owner_id, &start_params(&fx.agent_id, "turn 1"))
        .expect("launch succeeds via custom SandboxRunner");
    assert_eq!(started.employee_id, fx.agent_id);
    assert_eq!(
        *fx.runner.adapted_marker.lock().unwrap(),
        Some(PermissionMode::Ask)
    );
    assert_eq!(fx.runner.prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fx.runner.adapt_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fx.runner.verify_calls.load(Ordering::SeqCst), 2);
    assert!(fx.runner.stale_calls.load(Ordering::SeqCst) >= 1);

    wait_for_idle(&fx.mgr, &fx.owner_id, &fx.agent_id);

    // 2. Marking the plan stale triggers warm-handle recycle and registration staleness abort.
    fx.runner.stale.store(true, Ordering::SeqCst);
    let stale_err = fx
        .mgr
        .start_session(
            &fx.owner_id,
            &start_params(&fx.agent_id, "turn 2 while stale"),
        )
        .unwrap_err();
    assert_eq!(stale_err.code, org_proto::ErrorCode::Conflict);
    assert_eq!(fx.runner.prepare_calls.load(Ordering::SeqCst), 2);
    fx.mgr.stop_all("test_done");
}

#[test]
fn local_os_sandbox_selects_modes_and_verifies_siblings() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = office.durable_root().join("run-sock");
    fs::create_dir_all(&sock_dir).unwrap();
    let office_socket = sock_dir.join("office.sock");
    let me = office.owner_member_id().to_owned();
    let sandbox = LocalOsSandbox;
    let make_ctx = |sandbox_enabled, uses_mount_mask_sandbox| SandboxPrepareContext {
        office: &office,
        office_socket: &office_socket,
        agent_os_state_roots: &[],
        employee_id: &me,
        sandbox_enabled,
        host_profile: org_agent_runtime::RuntimeHostProfile {
            uses_mount_mask_sandbox,
            ..org_agent_runtime::RuntimeHostProfile::default()
        },
    };

    let unsandboxed = sandbox.prepare_launch(&make_ctx(false, true)).unwrap();
    assert_eq!(unsandboxed.mode(), SandboxConfinementMode::Unsandboxed);
    assert!(unsandboxed.masked_siblings().is_none());
    assert!(!unsandboxed.prunes_absent_denies());
    assert!(!sandbox.is_plan_stale(&office, &me, &unsandboxed));

    let host_native = sandbox.prepare_launch(&make_ctx(true, false)).unwrap();
    assert_eq!(host_native.mode(), SandboxConfinementMode::HostNative);
    assert!(host_native.masked_siblings().is_none());
    assert!(!host_native.prunes_absent_denies());
    assert!(!sandbox.is_plan_stale(&office, &me, &host_native));

    let bwrap_plan = sandbox.prepare_launch(&make_ctx(true, true)).unwrap();
    if !org_platform::confinement::unix_socket_path_filter_supported() {
        assert_eq!(bwrap_plan.mode(), SandboxConfinementMode::LinuxBwrap);
        assert!(bwrap_plan.prunes_absent_denies());
        assert!(bwrap_plan.masked_siblings().is_some());
        assert!(!sandbox.is_plan_stale(&office, &me, &bwrap_plan));
        let verify_ctx = |active_employees| SandboxVerifyContext {
            office: &office,
            employee_id: &me,
            active_employees,
        };
        sandbox
            .verify_before_spawn(&verify_ctx(&[]), &bwrap_plan)
            .unwrap();
        let unmasked = vec!["emp-raced-sibling".to_owned()];
        let err = sandbox
            .verify_before_spawn(&verify_ctx(&unmasked), &bwrap_plan)
            .unwrap_err();
        assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    }
}

#[test]
fn local_os_sandbox_prunes_absent_denies_only_when_plan_requests_it() {
    let sandbox = LocalOsSandbox;
    let dir = tempfile::tempdir().unwrap();
    let ws = SpecPath::try_from(dir.path().to_str().unwrap()).unwrap();
    let absent = org_agent_runtime::PathPattern::try_from(
        dir.path().join("does-not-exist").to_str().unwrap(),
    )
    .unwrap();
    let make_spec = || SessionSpec {
        runtime: RuntimeId::try_from("claude").unwrap(),
        kind: org_agent_runtime::SessionKind::Home {},
        agent: org_agent_runtime::AgentIdentity {
            name: org_agent_runtime::AgentName::try_from("eng").unwrap(),
            description: "Engineer".into(),
            prompt: "Engineer".into(),
        },
        model: None,
        effort: None,
        workspace: ws.clone(),
        read_only_dirs: Vec::new(),
        instructions: String::new(),
        repository_instructions: String::new(),
        permission_mode: PermissionMode::AcceptEdits,
        policy: org_agent_runtime::PermissionPolicy {
            version: 1,
            allow: vec![org_agent_runtime::Rule {
                tool: org_agent_runtime::RuleTool::try_from("Read").unwrap(),
                scope: org_agent_runtime::RuleScope::Any,
            }],
            deny: vec![org_agent_runtime::Rule {
                tool: org_agent_runtime::RuleTool::try_from("*").unwrap(),
                scope: org_agent_runtime::RuleScope::Path(absent.clone()),
            }],
        },
        sandbox: org_agent_runtime::SandboxSpec {
            enabled: true,
            writable_roots: vec![ws.clone()],
            deny_write: vec![absent.clone()],
            deny_read: vec![absent.clone()],
            allowed_unix_sockets: Vec::new(),
        },
        mcp_servers: Default::default(),
        tools: None,
        operator_shell_prefixes: Vec::new(),
    };

    let mut native_spec = make_spec();
    sandbox.adapt_session_spec(&SandboxPlan::host_native(), &mut native_spec);
    assert_eq!(native_spec.sandbox.deny_read.len(), 1);
    assert_eq!(native_spec.permission_mode, PermissionMode::AcceptEdits);

    let mut pruned_spec = make_spec();
    let linux_plan = SandboxPlan::linux_bwrap(BTreeSet::new());
    sandbox.adapt_session_spec(&linux_plan, &mut pruned_spec);
    assert!(pruned_spec.sandbox.deny_read.is_empty());
    assert!(pruned_spec.policy.allow.is_empty());
    assert_eq!(pruned_spec.permission_mode, PermissionMode::Ask);
}
