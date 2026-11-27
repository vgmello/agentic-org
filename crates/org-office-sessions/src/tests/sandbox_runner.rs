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
        self.verify_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_verify.load(Ordering::SeqCst) {
            return Err(org_proto::ApiError::new(
                org_proto::ErrorCode::Conflict,
                "mock sandbox rejected pre-spawn verification",
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

#[test]
fn custom_sandbox_runner_flows_through_launch_adapt_verify_and_staleness() {
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
    let script = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  printf '{"kind":"reply","text":"ack"}\n'
done
"#;
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

    // 1. Failing `enforce_sibling_isolation` aborts before `prepare_launch`.
    runner.fail_enforce.store(true, Ordering::SeqCst);
    let err = mgr
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("blocked by enforce".into()),
                workspace: None,
            },
        )
        .unwrap_err();
    assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    assert_eq!(runner.enforce_calls.load(Ordering::SeqCst), 1);
    assert_eq!(runner.prepare_calls.load(Ordering::SeqCst), 0);
    assert!(!mgr.registry.lock().unwrap().starting.contains(&agent_id));
    runner.fail_enforce.store(false, Ordering::SeqCst);

    // 2. Failing `verify_before_spawn` aborts launch and leaves no live handle or leaked slot.
    runner.fail_verify.store(true, Ordering::SeqCst);
    let err = mgr
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("blocked by verify".into()),
                workspace: None,
            },
        )
        .unwrap_err();
    assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    assert_eq!(runner.prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(runner.adapt_calls.load(Ordering::SeqCst), 1);
    assert_eq!(runner.verify_calls.load(Ordering::SeqCst), 1);
    assert!(mgr.registry.lock().unwrap().live.is_empty());
    assert!(!mgr.registry.lock().unwrap().starting.contains(&agent_id));
    runner.fail_verify.store(false, Ordering::SeqCst);

    // 3. Happy path flows through enforce -> prepare -> adapt -> verify (pre-spawn + pre-pump)
    //    -> is_plan_stale (registration).
    let started = mgr
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("turn 1".into()),
                workspace: None,
            },
        )
        .expect("launch succeeds via custom SandboxRunner");
    assert_eq!(started.employee_id, agent_id);
    assert_eq!(
        *runner.adapted_marker.lock().unwrap(),
        Some(PermissionMode::Ask)
    );
    assert_eq!(runner.prepare_calls.load(Ordering::SeqCst), 2);
    assert_eq!(runner.adapt_calls.load(Ordering::SeqCst), 2);
    // 1 failed + 2 succeeded (pre-spawn & pre-pump).
    assert_eq!(runner.verify_calls.load(Ordering::SeqCst), 3);
    assert!(runner.stale_calls.load(Ordering::SeqCst) >= 1);

    // Wait for Turn 1 to reach Idle.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if mgr
            .inspect_session(&owner_id, Some(&agent_id))
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
        mgr.inspect_session(&owner_id, Some(&agent_id))
            .unwrap()
            .session
            .state,
        SessionState::Idle
    );

    // 4. Mark plan stale for warm reuse check, then allow the fresh relaunch to register.
    runner.stale.store(true, Ordering::SeqCst);
    // Flip stale back after warm freshness sees it? Even cleaner: verify `is_plan_stale` causes
    // warm recycle — wait, if stale stays true at registration, registration will abort cleanly
    // with Conflict proving BOTH warm recycle AND registration staleness flowed through the trait!
    let stale_err = mgr
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("turn 2 while stale".into()),
                workspace: None,
            },
        )
        .unwrap_err();
    assert_eq!(stale_err.code, org_proto::ErrorCode::Conflict);
    // Warm handle was recycled and a fresh cold launch ran `prepare_launch` a 3rd time!
    assert_eq!(runner.prepare_calls.load(Ordering::SeqCst), 3);
    mgr.stop_all("test_done");
}

#[test]
fn local_os_sandbox_selects_modes_prunes_denies_and_verifies_siblings() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = office.durable_root().join("run-sock");
    fs::create_dir_all(&sock_dir).unwrap();
    let office_socket = sock_dir.join("office.sock");
    let me = office.owner_member_id().to_owned();
    let sandbox = LocalOsSandbox;

    // 1. Unsandboxed mode.
    let unsandboxed = sandbox
        .prepare_launch(&SandboxPrepareContext {
            office: &office,
            office_socket: &office_socket,
            agent_os_state_roots: &[],
            employee_id: &me,
            sandbox_enabled: false,
            host_profile: org_agent_runtime::RuntimeHostProfile {
                uses_linux_bwrap_sandbox: true,
                ..org_agent_runtime::RuntimeHostProfile::default()
            },
        })
        .unwrap();
    assert_eq!(unsandboxed.mode(), SandboxConfinementMode::Unsandboxed);
    assert!(unsandboxed.masked_siblings().is_none());
    assert!(!unsandboxed.prunes_absent_denies());
    assert!(!sandbox.is_plan_stale(&office, &me, &unsandboxed));

    // 2. Host-native mode (sandboxed runtime that does not use Linux bwrap).
    let host_native = sandbox
        .prepare_launch(&SandboxPrepareContext {
            office: &office,
            office_socket: &office_socket,
            agent_os_state_roots: &[],
            employee_id: &me,
            sandbox_enabled: true,
            host_profile: org_agent_runtime::RuntimeHostProfile {
                uses_linux_bwrap_sandbox: false,
                ..org_agent_runtime::RuntimeHostProfile::default()
            },
        })
        .unwrap();
    assert_eq!(host_native.mode(), SandboxConfinementMode::HostNative);
    assert!(host_native.masked_siblings().is_none());
    assert!(!host_native.prunes_absent_denies());
    assert!(!sandbox.is_plan_stale(&office, &me, &host_native));

    // 3. Linux bwrap mode (when platform lacks Unix-socket path filtering, e.g., Linux).
    let bwrap_plan = sandbox
        .prepare_launch(&SandboxPrepareContext {
            office: &office,
            office_socket: &office_socket,
            agent_os_state_roots: &[],
            employee_id: &me,
            sandbox_enabled: true,
            host_profile: org_agent_runtime::RuntimeHostProfile {
                uses_linux_bwrap_sandbox: true,
                ..org_agent_runtime::RuntimeHostProfile::default()
            },
        })
        .unwrap();
    if !org_platform::confinement::unix_socket_path_filter_supported() {
        assert_eq!(bwrap_plan.mode(), SandboxConfinementMode::LinuxBwrap);
        assert!(bwrap_plan.prunes_absent_denies());
        assert!(bwrap_plan.masked_siblings().is_some());
        assert!(!sandbox.is_plan_stale(&office, &me, &bwrap_plan));
        // Verification passes when no unmasked sibling is active.
        sandbox
            .verify_before_spawn(
                &SandboxVerifyContext {
                    office: &office,
                    employee_id: &me,
                    active_employees: &[],
                },
                &bwrap_plan,
            )
            .unwrap();
        // Verification fails closed when an unmasked sibling is active/starting.
        let unmasked = vec!["emp-raced-sibling".to_owned()];
        let err = sandbox
            .verify_before_spawn(
                &SandboxVerifyContext {
                    office: &office,
                    employee_id: &me,
                    active_employees: &unmasked,
                },
                &bwrap_plan,
            )
            .unwrap_err();
        assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    }

    // 4. `adapt_session_spec` only prunes absent denies when `plan.prunes_absent_denies()` is true.
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
    sandbox.adapt_session_spec(&host_native, &mut native_spec);
    assert_eq!(native_spec.sandbox.deny_read.len(), 1);
    assert_eq!(native_spec.permission_mode, PermissionMode::AcceptEdits);

    let mut pruned_spec = make_spec();
    let linux_plan = SandboxPlan::linux_bwrap(BTreeSet::new());
    sandbox.adapt_session_spec(&linux_plan, &mut pruned_spec);
    assert!(pruned_spec.sandbox.deny_read.is_empty());
    assert!(pruned_spec.policy.allow.is_empty());
    assert_eq!(pruned_spec.permission_mode, PermissionMode::Ask);
}
