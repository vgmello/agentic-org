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
    build_grants_summary, build_session_policy, decide_escalation, parse_preset,
    EscalationDisposition,
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

// ---------------------------------------------------------------------------
// INSTR-4, 5, 7, 8, 12, 15, 21
// ---------------------------------------------------------------------------

#[test]
fn builtin_instructions_stay_under_10kib_and_all_named_commands_exist_in_live_table() {
    assert!(
        BUILTIN_BEHAVIORAL_RULES.len() <= BUILTIN_INSTRUCTIONS_MAX_BYTES,
        "built-in rules exceeded 10 KiB: {} bytes",
        BUILTIN_BEHAVIORAL_RULES.len()
    );
    let table = org_agent_cli::table::table();
    let missing_core = verify_command_references(BUILTIN_BEHAVIORAL_RULES, &table);
    assert!(
        missing_core.is_empty(),
        "missing core cmds: {missing_core:?}"
    );
    let missing_hr = verify_command_references(HR_LEAD_GUIDANCE, &table);
    assert!(missing_hr.is_empty(), "missing HR cmds: {missing_hr:?}");

    let assembled = assemble_standing_instructions(
        &InstructionInput {
            organization_name: "Acme Org",
            office_name: "HQ Mac",
            employee_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1",
            employee_name: "Ada",
            handle: "ada",
            team_name: "Engineering",
            role_name: Some("Staff Engineer"),
            role_instructions: Some("Write clean Rust."),
            agent_instructions: "Prefer small diffs.",
            is_hr_lead: true,
            private_home: "/tmp/ada/home",
            workspace: "/tmp/ada/home",
            permission_preset: "workspace_edits",
            sandbox_enabled: true,
        },
        &table,
    );
    assert!(assembled.contains("Organization standards"));
    assert!(assembled.contains("NEVER override higher-precedence organization authority"));
    assert!(assembled.contains(SAME_USER_ISOLATION_NOTICE));
    assert!(assembled.contains("Built-in HR Lead Duties"));
    assert!(assembled.contains("agent-os session inspect"));
    assert!(assembled.contains("agent-os grant list"));
    assert!(assembled.contains("agent-os hr hire --draft"));

    let restart = compose_turn_objective(&TurnTrigger::RestartContinuation {
        previous_objective: "Ship C3".into(),
        accepted_commands: vec!["hr.hire.draft (0190...)".into()],
    });
    assert!(restart.contains("Office restarted"));
    assert!(restart.contains("hr.hire.draft"));

    let hr_obj = compose_turn_objective(&TurnTrigger::HrProposalRequest {
        requester: "owner".into(),
        request: "Hire a Rust engineer into Engineering".into(),
    });
    assert!(hr_obj.contains("agent-os hr hire --draft"));
    assert!(hr_obj.contains("requester confirmation"));
}

// ---------------------------------------------------------------------------
// ENROLL-7: Presets, default-on sandbox, escalation & bypass refusal
// ---------------------------------------------------------------------------

#[test]
fn presets_refuse_bypass_and_enforce_escalation_policy() {
    for bypass in [
        "bypass",
        "bypassPermissions",
        "bypass_permissions",
        "dangerously_skip_permissions",
        "unrestricted",
    ] {
        assert!(parse_preset(bypass).is_err(), "{bypass} must be refused");
    }
    let ws = SpecPath::try_from("/tmp/agent-home").unwrap();
    let sock = SpecPath::try_from("/tmp/office.sock").unwrap();
    let glob = org_agent_runtime::PathPattern::try_from("/office/**").unwrap();

    let safe = build_session_policy(
        PermissionPreset::SafeAsk,
        true,
        &ws,
        Some(&sock),
        std::slice::from_ref(&glob),
        1,
    )
    .unwrap();
    assert_eq!(safe.permission_mode, PermissionMode::Ask);
    assert!(safe.sandbox.enabled);

    let ro = build_session_policy(
        PermissionPreset::ReadOnly,
        true,
        &ws,
        Some(&sock),
        std::slice::from_ref(&glob),
        1,
    )
    .unwrap();
    assert_eq!(ro.permission_mode, PermissionMode::Plan);
    assert!(ro.sandbox.writable_roots.is_empty());

    let auto = build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &ws,
        Some(&sock),
        std::slice::from_ref(&glob),
        2,
    )
    .unwrap();
    assert_eq!(auto.permission_mode, PermissionMode::AcceptEdits);
    let grants = build_grants_summary("emp-1", &auto, &[]);
    assert_eq!(grants.permission_preset, "autonomous_sandboxed");
    assert!(grants.sandbox_enabled);
    assert!(!grants.rules.is_empty());

    // SandboxBypass is ALWAYS auto-denied even under AutonomousSandboxed.
    let bypass_req = ApprovalRequest::new(
        ApprovalId::new("a1"),
        ApprovalKind::Other,
        Escalation::SandboxBypass,
        "Bash",
        serde_json::json!({}),
    );
    assert!(matches!(
        decide_escalation(
            &bypass_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::AutoDeny { .. }
    ));

    // PermissionChange ALWAYS escalates to owner.
    let perm_req = ApprovalRequest::new(
        ApprovalId::new("a2"),
        ApprovalKind::Other,
        Escalation::PermissionChange,
        "Edit",
        serde_json::json!({}),
    );
    assert!(matches!(
        decide_escalation(
            &perm_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::EscalateToOwner { .. }
    ));

    // Path-scoped deny rule beats allow rule under AutonomousSandboxed (High #7).
    let denied_path_req = ApprovalRequest::new(
        ApprovalId::new("a-deny-path"),
        ApprovalKind::Other,
        Escalation::None,
        "Read",
        serde_json::json!({ "file_path": "/office/durable/state/org.sqlite" }),
    );
    assert!(matches!(
        decide_escalation(
            &denied_path_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::AutoDeny { .. }
    ));

    // Opaque Bash call fails closed (escalates, never auto-allows) when path denies exist (High #7).
    let opaque_bash_req = ApprovalRequest::new(
        ApprovalId::new("a-opaque-bash"),
        ApprovalKind::Other,
        Escalation::None,
        "Bash",
        serde_json::json!({}),
    );
    assert!(matches!(
        decide_escalation(
            &opaque_bash_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::EscalateToOwner { .. }
    ));

    // Normal allow rule works for safe file path under AutonomousSandboxed, escalates under SafeAsk.
    let normal_req = ApprovalRequest::new(
        ApprovalId::new("a3"),
        ApprovalKind::Other,
        Escalation::None,
        "Edit",
        serde_json::json!({ "file_path": "/tmp/agent-home/notes.md" }),
    );
    assert_eq!(
        decide_escalation(
            &normal_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::AllowByPolicy
    );
    assert!(matches!(
        decide_escalation(&normal_req, &safe.policy, PermissionPreset::SafeAsk),
        EscalationDisposition::EscalateToOwner { .. }
    ));
}

// ---------------------------------------------------------------------------
// SUB-1..5, SUB-7: Sub-agents & background task tracking/reaping
// ---------------------------------------------------------------------------

#[test]
fn subagents_enforce_tool_subset_usage_attribution_and_reaping() {
    let dir = tempfile::tempdir().unwrap();
    let parent = vec![
        ToolName::try_from("Read").unwrap(),
        ToolName::try_from("Grep").unwrap(),
    ];
    let requested = vec![
        ToolName::try_from("Read").unwrap(),
        ToolName::try_from("Bash").unwrap(),
    ];
    let sub = subset_tools(Some(&parent), Some(&requested)).unwrap();
    assert_eq!(sub, vec![ToolName::try_from("Read").unwrap()]);

    let def = build_general_subagent_definition(Some(&parent), Some(&requested));
    let written = write_subagent_definition(dir.path(), &def).unwrap();
    assert!(written.exists());
    let loaded: serde_json::Value = serde_json::from_slice(&fs::read(&written).unwrap()).unwrap();
    assert_eq!(loaded["tools"], serde_json::json!(["Read"]));

    let mut bg = BackgroundRegistry::new();
    bg.register(
        "sub-1",
        BackgroundKind::Subagent,
        "research",
        Some(1234),
        "t0",
    );
    bg.register(
        "bg-2",
        BackgroundKind::BackgroundTask,
        "build",
        Some(1235),
        "t0",
    );
    assert_eq!(bg.active_count(), 2);

    let mut u = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
    u.input = 100;
    u.output = 50;
    bg.record_usage(&u);
    assert_eq!(bg.usage().input_tokens, 100);
    assert_eq!(bg.usage().output_tokens, 50);
    assert_eq!(bg.usage().subagent_invocations, 1);

    // Completing sub-1 emits a wake across turns while bg-2 stays running.
    let wake = bg.complete("sub-1", true, "found 3 files", "t1").unwrap();
    assert_eq!(wake.task_id, "sub-1");
    assert_eq!(bg.active_count(), 1);
    assert_eq!(bg.drain_wakes().len(), 1);

    // Pause/Stop/Offboard/Stall reaps remaining bg-2.
    let reaped = bg.reap_all("t2");
    assert_eq!(reaped, vec![("bg-2".to_owned(), Some(1235))]);
    assert_eq!(bg.active_count(), 0);
    assert!(bg
        .views()
        .iter()
        .any(|v| v.task_id == "bg-2" && v.state == "reaped"));
}

// ---------------------------------------------------------------------------
// CLI-12, SUB-3: Run binding token & descendant verification
// ---------------------------------------------------------------------------

#[test]
fn run_binding_enforces_token_descendant_and_revocation() {
    let reg = RunBindingRegistry::new();
    let self_pid = std::process::id() as libc::pid_t;
    let binding = reg.issue("emp-1", "run-1", "sess-1", self_pid).unwrap();
    assert!(!format!("{binding:?}").contains(binding.expose_token()));

    // Missing token -> refused
    assert_eq!(
        reg.verify_peer("emp-1", None, Some(self_pid)).unwrap_err(),
        BindingRejection::MissingToken
    );
    // Wrong token -> refused
    assert_eq!(
        reg.verify_peer("emp-1", Some("wrong"), Some(self_pid))
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
    // Valid token + self PID -> accepted
    assert!(reg
        .verify_peer("emp-1", Some(binding.expose_token()), Some(self_pid))
        .is_ok());
    // Valid token + real child process (sub-agent / child shell, SUB-3) -> accepted
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "sleep 3"])
        .spawn()
        .unwrap();
    let child_pid = child.id() as libc::pid_t;
    let child_ok = reg.verify_peer("emp-1", Some(binding.expose_token()), Some(child_pid));
    let _ = child.kill();
    let _ = child.wait();
    assert!(child_ok.is_ok(), "child process must pass descendant check");

    // Non-descendant PID (init pid 1) -> refused
    assert_eq!(
        reg.verify_peer("emp-1", Some(binding.expose_token()), Some(1))
            .unwrap_err(),
        BindingRejection::NotDescendant
    );

    // Revocation invalidates token immediately
    reg.revoke("emp-1", Some("run-1"));
    assert_eq!(
        reg.verify_peer("emp-1", Some(binding.expose_token()), Some(self_pid))
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
}

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

#[test]
fn enrollment_vault_wraps_credentials_enforces_single_active_wipes_temp_and_redacts() {
    let dir = tempfile::tempdir().unwrap();
    let key = [0x42u8; 32];
    let vault = CredentialVault::new(dir.path().to_path_buf(), "org-1", "office-1", Some(key));
    let secret = "sk-ant-oat-secret-token-value-9999";
    let mgr = EnrollmentManager::with_timeout(
        vault.clone(),
        dir.path().join("enroll-tmp"),
        Arc::new(FakeEnrollmentDriver {
            secret: secret.to_owned(),
        }),
        Duration::from_secs(60),
    );

    // ENROLL-1: start creates 0700 temp dir; ENROLL-4: second concurrent start conflicts.
    let started = mgr.start("owner-1", "conn-1", "anthropic").unwrap();
    assert_eq!(started.state, EnrollmentState::AwaitingCode);
    let tmp_dir = mgr.temp_dir_for(&started.login_id);
    assert!(tmp_dir.exists());
    assert_eq!(
        fs::metadata(&tmp_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(matches!(
        mgr.start("owner-1", "conn-1", "anthropic"),
        Err(EnrollmentVaultError::Conflict(_))
    ));

    // Wrong caller cannot inspect, submit, or cancel another caller's login_id (High #3).
    assert!(matches!(
        mgr.status("other-member", "conn-1", &started.login_id),
        Err(EnrollmentVaultError::NotAuthorized(_))
    ));
    assert!(matches!(
        mgr.submit_code("other-member", "conn-1", &started.login_id, "code-1234"),
        Err(EnrollmentVaultError::NotAuthorized(_))
    ));
    assert!(matches!(
        mgr.cancel("other-member", "conn-1", &started.login_id),
        Err(EnrollmentVaultError::NotAuthorized(_))
    ));

    // ENROLL-2: malformed code with whitespace/quotes rejected.
    assert!(matches!(
        mgr.submit_code(
            "owner-1",
            "conn-1",
            &started.login_id,
            "bad code\nwith newline"
        ),
        Err(EnrollmentVaultError::InvalidInput(_))
    ));

    // ENROLL-2/3: valid code wraps credential under KeyClass::ProviderCredential & wipes temp dir.
    let done = mgr
        .submit_code(
            "owner-1",
            "conn-1",
            &started.login_id,
            "callback-code-abc123",
        )
        .unwrap();
    assert_eq!(done.state, EnrollmentState::Completed);
    assert!(!tmp_dir.exists(), "temp dir must be wiped on completion");
    let raw_file = fs::read_to_string(vault.credential_path("conn-1").unwrap()).unwrap();
    assert!(
        !raw_file.contains(secret),
        "plaintext secret must never hit disk"
    );
    let unwrapped = vault.load_unwrapped("conn-1").unwrap().unwrap();
    assert_eq!(unwrapped.as_slice(), secret.as_bytes());

    // ENROLL-3: redaction scrubs known tokens, prefixes, and Bearer headers.
    let dirty = format!("Auth: Bearer {secret} and token sk-ant-api1234567890");
    let clean = redact_secrets(&dirty, &[secret]);
    assert!(!clean.contains(secret));
    assert!(!clean.contains("sk-ant-api1234567890"));
    assert!(clean.contains("[REDACTED]"));

    // ENROLL-5: revoke removes credential and status reports enrolled=false.
    assert!(vault.status("conn-1").unwrap().enrolled);
    let revoked = vault.revoke("conn-1").unwrap();
    assert!(!revoked.enrolled);
    assert!(vault.load_unwrapped("conn-1").unwrap().is_none());

    // ENROLL-4: expiry wipes temp dir automatically.
    let short_mgr = EnrollmentManager::with_timeout(
        vault,
        dir.path().join("enroll-tmp-short"),
        Arc::new(FakeEnrollmentDriver {
            secret: secret.to_owned(),
        }),
        Duration::from_millis(150),
    );
    let exp = short_mgr.start("owner-1", "conn-2", "openai").unwrap();
    let exp_dir = short_mgr.temp_dir_for(&exp.login_id);
    assert!(exp_dir.exists());
    std::thread::sleep(Duration::from_millis(200));
    let polled = short_mgr
        .status("owner-1", "conn-2", &exp.login_id)
        .unwrap();
    assert_eq!(polled.state, EnrollmentState::Expired);
    assert!(!exp_dir.exists(), "temp dir must be wiped on expiry");
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

#[test]
fn recovery_handles_clean_resume_unprovable_exit_resume_cap_and_stall_lock() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path(), "office-1");
    let turn = ActiveTurnRecord {
        version: 1,
        employee_id: "emp-1".into(),
        run_id: "run-1".into(),
        session_id: "sess-1".into(),
        runtime_session_id: Some("rs-1".into()),
        mark: "m-1".into(),
        pid: Some(99999),
        pgid: Some(99999),
        started_identity: Some("1:1".into()),
        boot_identity: Some("boot-1".into()),
        objective: "Finish feature".into(),
        accepted_commands: vec!["hr.hire.draft".into()],
        resume_attempts: 0,
        consecutive_stalls: 0,
        stall_locked: false,
        recovery_required: false,
        active: true,
        updated_at: "2026-11-21T00:00:00.000Z".into(),
    };
    store.write_active_turn(&turn).unwrap();
    store
        .write_pending_start(&PendingStartRecord {
            version: 1,
            employee_id: "emp-1".into(),
            run_id: "run-1".into(),
            session_id: "sess-1".into(),
            mark: "m-1".into(),
            started_at: "2026-11-21T00:00:00.000Z".into(),
        })
        .unwrap();

    // LIVE-10: proven exited -> clears pending-start and returns Resume with accepted commands.
    let report = store.recover_on_startup(&FixedOracle(Ok(true))).unwrap();
    assert_eq!(report.actions.len(), 1);
    assert!(matches!(
        &report.actions[0],
        RecoveryAction::Resume {
            employee_id,
            accepted_commands,
            attempt: 1,
            ..
        } if employee_id == "emp-1" && accepted_commands == &vec!["hr.hire.draft".to_owned()]
    ));
    assert!(store.read_pending_start("emp-1").unwrap().is_none());

    // LIVE-11: after 3 resumes, 4th restart enters recovery_required.
    let _ = store.recover_on_startup(&FixedOracle(Ok(true))).unwrap(); // attempt 2
    let _ = store.recover_on_startup(&FixedOracle(Ok(true))).unwrap(); // attempt 3
    let fourth = store.recover_on_startup(&FixedOracle(Ok(true))).unwrap();
    assert!(matches!(
        &fourth.actions[0],
        RecoveryAction::RecoveryRequired { .. }
    ));

    // Unprovable exit fails closed immediately into recovery_required.
    let mut turn2 = turn;
    turn2.employee_id = "emp-2".into();
    store.write_active_turn(&turn2).unwrap();
    let failed = store
        .recover_on_startup(&FixedOracle(Err(io::ErrorKind::PermissionDenied)))
        .unwrap();
    assert!(failed
        .actions
        .iter()
        .any(|a| matches!(a, RecoveryAction::RecoveryRequired { employee_id, .. } if employee_id == "emp-2")));

    // Pending-start-only crash with unprovable exit still persists recovery_required=true (Medium #14).
    store
        .write_pending_start(&PendingStartRecord {
            version: 1,
            employee_id: "emp-pending-only".into(),
            run_id: "run-p".into(),
            session_id: "sess-p".into(),
            mark: "mark-p".into(),
            started_at: "2026-11-21T00:00:00.000Z".into(),
        })
        .unwrap();
    let pending_fail = store.recover_on_startup(&FixedOracle(Ok(false))).unwrap();
    assert!(pending_fail
        .actions
        .iter()
        .any(|a| matches!(a, RecoveryAction::RecoveryRequired { employee_id, .. } if employee_id == "emp-pending-only")));
    let persisted_pending = store
        .read_active_turn("emp-pending-only")
        .unwrap()
        .expect("synthesized active turn record persisted");
    assert!(persisted_pending.recovery_required);
    assert!(!persisted_pending.active);

    // LIVE-12: StallWatchdog locks after 3 consecutive stalls across restarts and unlocks on clear.
    let t0 = Instant::now();
    let mut wd = StallWatchdog::new(Duration::from_millis(50), t0);
    assert!(!wd.is_stalled(t0 + Duration::from_millis(10)));
    assert!(wd.is_stalled(t0 + Duration::from_millis(60)));
    assert!(!wd.record_stall(t0 + Duration::from_millis(60)));
    assert!(!wd.record_stall(t0 + Duration::from_millis(120)));
    // Simulate process restart preserving consecutive_stalls = 2 (High #5).
    let mut wd_restarted = StallWatchdog::from_persisted(
        Duration::from_millis(50),
        t0 + Duration::from_millis(130),
        2,
        false,
    );
    assert!(wd_restarted.record_stall(t0 + Duration::from_millis(190)));
    assert!(wd_restarted.is_locked());
    assert_eq!(wd_restarted.consecutive_stalls(), 3);
    wd_restarted.clear_stall_lock(t0 + Duration::from_millis(200));
    assert!(!wd_restarted.is_locked());
    assert_eq!(wd_restarted.consecutive_stalls(), 0);
}

// ---------------------------------------------------------------------------
// Architecture Rule 1 & TASK-83: Production registry wires claude, codex, agy;
// unknown runtimes fail closed.
// ---------------------------------------------------------------------------

#[test]
fn production_registry_wires_claude_codex_and_agy_and_refuses_unknown() {
    let reg = RuntimeRegistry::production();
    for (alias, expected_id) in [
        ("claude", "claude"),
        ("anthropic", "claude"),
        ("codex", "codex"),
        ("openai", "codex"),
        ("gpt", "codex"),
        ("agy", "agy"),
        ("antigravity", "agy"),
        ("google", "agy"),
    ] {
        let adapter = reg.resolve(alias).expect("adapter resolves");
        assert_eq!(adapter.id().as_str(), expected_id);
        let ws = SpecPath::try_from("/tmp/ws").unwrap();
        let spec = SessionSpec {
            runtime: adapter.id(),
            kind: org_agent_runtime::SessionKind::Home {},
            agent: org_agent_runtime::AgentIdentity {
                name: org_agent_runtime::AgentName::try_from("ada").unwrap(),
                description: "dev".into(),
                prompt: "hi".into(),
            },
            model: None,
            effort: None,
            workspace: ws.clone(),
            read_only_dirs: Vec::new(),
            instructions: "test".into(),
            permission_mode: PermissionMode::Ask,
            policy: org_agent_runtime::PermissionPolicy {
                version: 1,
                allow: Vec::new(),
                deny: Vec::new(),
            },
            sandbox: org_agent_runtime::SandboxSpec {
                enabled: true,
                writable_roots: vec![ws.clone()],
                deny_write: Vec::new(),
                deny_read: Vec::new(),
                allowed_unix_sockets: Vec::new(),
            },
            mcp_servers: Default::default(),
            tools: None,
        };
        let ctx = LaunchContext {
            runtime_path: PathBuf::from("/bin/true"),
            runtime_version: None,
            runtime_dir: ws.clone(),
            tmpdir: ws,
            office_env_names: Vec::new(),
            new_session_id: Some(org_agent_runtime::SessionId::try_from("sess-test-1").unwrap()),
            resume: None,
            config_dir: None,
        };
        let (plan, _) = adapter
            .plan_session(&spec, &ctx)
            .expect("built adapter plans session");
        assert!(!plan.args.is_empty());
    }
    assert!(matches!(
        reg.resolve("nonexistent-runtime"),
        Err(crate::RuntimeRegistryError::UnknownRuntime(_))
    ));
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

#[test]
fn real_supervised_agent_completes_instruction_in_private_home_and_redacts_token() {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let snap = office.org_snapshot().unwrap();
    let root_team_id = snap.teams[0].id.clone();

    let conn_id = new_uuid_v7();
    let prof_id = new_uuid_v7();
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id,
            default_profile_id: prof_id.clone(),
            key: "claude-test".into(),
            name: "Claude Test".into(),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    let conn_resp = office
        .dispatch_hiring_rpc(Some(&owner_id), &conn_req)
        .expect("provider connection dispatched");
    assert!(matches!(conn_resp.outcome, org_proto::Outcome::Result(_)));

    // Hire an agent through C2 HrHire
    let req_id = new_uuid_v7();
    let emp_id = new_uuid_v7();
    let hire_req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: req_id,
            employee_id: emp_id,
            home_team_id: root_team_id,
            role_id: None,
            proposal: HireProposalWire {
                name: "Ada Lovelace".into(),
                handle: "ada".into(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner_id.clone()),
                office_id: None,
                model_profile_id: Some(prof_id),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("workspace_edits".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: Some("Answer directly.".into()),
            },
            reason: "Need an engineer".into(),
        },
    )
    .unwrap();
    let resp = office
        .dispatch_hiring_rpc(Some(&owner_id), &hire_req)
        .expect("hire dispatched");
    let hired: org_proto::hiring::HireOpResult = match resp.outcome {
        org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
    };
    let agent_id = hired.employee_id.expect("hired employee id");

    // Create a tiny executable script speaking the scripted JSON-lines protocol, proving:
    // - cwd == private 0700 home
    // - AGENT_OS_SESSION_TOKEN is injected (and redacted in history.jsonl!)
    let bin_dir = tempfile::tempdir().unwrap();
    let script_path = bin_dir.path().join("scripted-agent");
    let script = r#"#!/bin/sh
echo '{"kind":"ready"}'
read line
pwd_now=$(pwd)
printf '{"kind":"reply","text":"Completed in %s with token=%s"}\n' "$pwd_now" "$AGENT_OS_SESSION_TOKEN"
"#;
    fs::write(&script_path, script).unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

    let mut registry = RuntimeRegistry::production();
    registry.register(Arc::new(ScriptedRuntime), Some(script_path));

    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(bin_dir.path().to_path_buf());
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    let started = manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("Say hello from your private home.".into()),
                workspace: None,
            },
        )
        .expect("session started");
    assert_eq!(started.employee_id, agent_id);
    assert_eq!(
        started.same_user_isolation_notice,
        SAME_USER_ISOLATION_NOTICE
    );

    // Wait for scripted child to complete the turn and exit.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut reply = None;
    while Instant::now() < deadline {
        let inspected = manager.inspect_session(&owner_id, Some(&agent_id)).unwrap();
        if let Some(r) = inspected.session.last_reply {
            reply = Some(r);
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let reply = reply.expect("agent produced reply");
    let expected_home = office
        .agent_private_home_path(&agent_id)
        .canonicalize()
        .unwrap();
    assert!(
        reply.contains(&expected_home.display().to_string()),
        "reply `{reply}` did not run inside private home `{}`",
        expected_home.display()
    );
    assert!(
        reply.contains("[REDACTED]"),
        "AGENT_OS_SESSION_TOKEN must be redacted in reply/history: {reply}"
    );
    let history = fs::read_to_string(manager.store().history_path(&agent_id).unwrap()).unwrap();
    assert!(history.contains("[REDACTED]"));

    // High #1: Cross-agent inspect & grant-list are refused (NotAuthorized), while self is allowed.
    let hr_lead_id = snap
        .members
        .iter()
        .find(|m| m.kind == org_proto::org::MemberKind::Agent)
        .expect("built-in HR lead exists")
        .id
        .clone();
    assert!(manager.inspect_session(&agent_id, None).is_ok());
    assert!(manager.list_grants(&agent_id, None).is_ok());
    let cross_inspect = manager
        .inspect_session(&agent_id, Some(&hr_lead_id))
        .unwrap_err();
    assert_eq!(cross_inspect.code, org_proto::ErrorCode::NotAuthorized);
    let cross_grants = manager
        .list_grants(&agent_id, Some(&hr_lead_id))
        .unwrap_err();
    assert_eq!(cross_grants.code, org_proto::ErrorCode::NotAuthorized);

    // High #3: Non-owner (agent) is refused on enrollment, credential, approval, and clear-stall.
    let agent_enroll = Request::new::<ProviderEnrollmentStart>(
        99,
        &ProviderEnrollmentStartParams {
            command_id: new_uuid_v7(),
            connection_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0".into(),
            provider: Some("anthropic".into()),
        },
    )
    .unwrap();
    let agent_enroll_resp = manager
        .dispatch_rpc(Some(&agent_id), &agent_enroll)
        .expect("enrollment dispatched");
    assert!(matches!(
        agent_enroll_resp.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::NotAuthorized
    ));
    let agent_clear = manager.clear_stall_lock(&agent_id, &agent_id).unwrap_err();
    assert_eq!(agent_clear.code, org_proto::ErrorCode::NotAuthorized);
    let agent_approval = manager
        .decide_approval(&agent_id, &agent_id, "a-missing", "allow_once", None)
        .unwrap_err();
    assert_eq!(agent_approval.code, org_proto::ErrorCode::NotAuthorized);

    // High #5: Persisted active-turn record retains runtime_session_id and marks active=false on turn end.
    let stored_turn = manager
        .store()
        .read_active_turn(&agent_id)
        .unwrap()
        .expect("active turn record persisted");
    assert_eq!(
        stored_turn.runtime_session_id.as_deref(),
        Some("rs-scripted-1")
    );
    assert!(!stored_turn.active);

    let _ = manager.stop_session(&agent_id, "test complete");
    let grants = manager.list_grants(&owner_id, Some(&agent_id)).unwrap();
    assert_eq!(grants.permission_preset, "workspace_edits");
    assert!(grants.sandbox_enabled);

    // Also verify enrollment RPC dispatch through SessionManager (owner succeeds)
    let enroll_cmd_id = new_uuid_v7();
    let enroll_req = Request::new::<ProviderEnrollmentStart>(
        2,
        &ProviderEnrollmentStartParams {
            command_id: enroll_cmd_id.clone(),
            connection_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0".into(),
            provider: Some("anthropic".into()),
        },
    )
    .unwrap();
    let enroll_resp = manager
        .dispatch_rpc(Some(&owner_id), &enroll_req)
        .expect("enrollment dispatched");
    assert!(matches!(enroll_resp.outcome, org_proto::Outcome::Result(_)));

    // High #1: Same command_id replayed by unauthorized agent or unbound caller is REFUSED;
    // same owner with different params returns Conflict; same owner with identical params replays.
    let agent_replay = manager
        .dispatch_rpc(Some(&agent_id), &enroll_req)
        .expect("dispatched");
    assert!(matches!(
        agent_replay.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::NotAuthorized
    ));
    let unbound_replay = manager.dispatch_rpc(None, &enroll_req).expect("dispatched");
    assert!(matches!(
        unbound_replay.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::NotAuthorized
    ));
    let mismatch_req = Request::new::<ProviderEnrollmentStart>(
        3,
        &ProviderEnrollmentStartParams {
            command_id: enroll_cmd_id,
            connection_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff".into(),
            provider: Some("openai".into()),
        },
    )
    .unwrap();
    let mismatch_resp = manager
        .dispatch_rpc(Some(&owner_id), &mismatch_req)
        .expect("dispatched");
    assert!(matches!(
        mismatch_resp.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::Conflict
    ));
    let idempotent_replay = manager
        .dispatch_rpc(Some(&owner_id), &enroll_req)
        .expect("dispatched");
    assert!(matches!(
        idempotent_replay.outcome,
        org_proto::Outcome::Result(_)
    ));

    // Medium #5: denied_office_globs includes enroll-tmp and sibling runtime/.
    let denied_strs: Vec<String> = manager
        .denied_office_globs()
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    assert!(
        denied_strs.iter().any(|s| s.contains("enroll-tmp")),
        "denied_office_globs must include enroll-tmp: {denied_strs:?}"
    );
    assert!(
        denied_strs.iter().any(|s| s.contains("/runtime/**")),
        "denied_office_globs must include runtime/: {denied_strs:?}"
    );

    let _guard = Mutex::new(());
    assert_eq!(
        manager
            .inspect_session(&owner_id, Some(&agent_id))
            .unwrap()
            .session
            .state,
        SessionState::Stopped
    );
}

#[test]
fn enrollment_concurrent_cancel_during_submit_never_resurrects_or_stores_credential() {
    use std::sync::Barrier;

    struct BarrierDriver {
        entered_submit: Arc<Barrier>,
        allow_finish: Arc<Barrier>,
    }

    impl EnrollmentDriver for BarrierDriver {
        fn start(
            &self,
            _provider: &str,
            _temp_dir: &Path,
        ) -> Result<EnrollmentPrompt, EnrollmentVaultError> {
            Ok(EnrollmentPrompt {
                state: EnrollmentState::AwaitingCode,
                auth_url: Some("https://example.test/auth".into()),
                device_code: None,
                message: None,
            })
        }

        fn submit_code(
            &self,
            _provider: &str,
            _temp_dir: &Path,
            _code: &str,
        ) -> Result<EnrollmentOutcome, EnrollmentVaultError> {
            self.entered_submit.wait();
            self.allow_finish.wait();
            Ok(EnrollmentOutcome::Completed {
                secret: zeroize::Zeroizing::new(b"sk-ant-oat-should-never-store".to_vec()),
                expires_at: None,
                account_hint: None,
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

    let dir = tempfile::tempdir().unwrap();
    let vault = CredentialVault::new(
        dir.path().to_path_buf(),
        "org-1",
        "office-1",
        Some([0x55u8; 32]),
    );
    let entered = Arc::new(Barrier::new(2));
    let allow = Arc::new(Barrier::new(2));
    let mgr = Arc::new(EnrollmentManager::with_timeout(
        vault.clone(),
        dir.path().join("enroll-race"),
        Arc::new(BarrierDriver {
            entered_submit: Arc::clone(&entered),
            allow_finish: Arc::clone(&allow),
        }),
        Duration::from_secs(60),
    ));

    let started = mgr.start("owner-1", "conn-race", "anthropic").unwrap();
    let mgr_submit = Arc::clone(&mgr);
    let login_id = started.login_id.clone();
    let handle = std::thread::spawn(move || {
        mgr_submit.submit_code("owner-1", "conn-race", &login_id, "valid-code-123")
    });

    entered.wait();
    let cancelled = mgr
        .cancel("owner-1", "conn-race", &started.login_id)
        .unwrap();
    assert_eq!(cancelled.state, EnrollmentState::Cancelled);
    allow.wait();

    let submit_res = handle.join().unwrap();
    assert!(
        matches!(submit_res, Err(EnrollmentVaultError::Conflict(_))),
        "cancelled submit_code must fail with Conflict, got {submit_res:?}"
    );
    assert!(
        vault.load_unwrapped("conn-race").unwrap().is_none(),
        "credential must NOT be stored after cancel"
    );
    let polled = mgr
        .status("owner-1", "conn-race", &started.login_id)
        .unwrap();
    assert_eq!(polled.state, EnrollmentState::Cancelled);
    assert!(!mgr.temp_dir_for(&started.login_id).exists());
}

#[test]
fn recovery_reports_locked_or_recovery_required_on_subsequent_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path(), "office-1");
    let locked_turn = ActiveTurnRecord {
        version: 1,
        employee_id: "emp-locked".into(),
        run_id: "run-l".into(),
        session_id: "sess-l".into(),
        runtime_session_id: None,
        mark: "m-l".into(),
        pid: Some(11111),
        pgid: Some(11111),
        started_identity: Some("1:1".into()),
        boot_identity: Some("boot-1".into()),
        objective: "Stalled objective".into(),
        accepted_commands: vec!["hr.hire.draft".into()],
        resume_attempts: 0,
        consecutive_stalls: 3,
        stall_locked: true,
        recovery_required: false,
        active: false,
        updated_at: "2026-11-21T00:00:00.000Z".into(),
    };
    store.write_active_turn(&locked_turn).unwrap();
    let r1 = store.recover_on_startup(&FixedOracle(Ok(true))).unwrap();
    assert!(r1.actions.iter().any(|a| matches!(
        a,
        RecoveryAction::RecoveryRequired { employee_id, .. } if employee_id == "emp-locked"
    )));
    // Second restart must STILL report RecoveryRequired (Medium #4).
    let r2 = store.recover_on_startup(&FixedOracle(Ok(true))).unwrap();
    assert!(r2.actions.iter().any(|a| matches!(
        a,
        RecoveryAction::RecoveryRequired { employee_id, .. } if employee_id == "emp-locked"
    )));
}

#[test]
fn multi_turn_persists_second_objective_and_accepted_commands_survive_stop_and_dead_pump_respawns()
{
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let snap = office.org_snapshot().unwrap();
    let root_team_id = snap.teams[0].id.clone();

    let conn_id = new_uuid_v7();
    let prof_id = new_uuid_v7();
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id,
            default_profile_id: prof_id.clone(),
            key: "claude-t2".into(),
            name: "Claude T2".into(),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    assert!(matches!(
        office
            .dispatch_hiring_rpc(Some(&owner_id), &conn_req)
            .unwrap()
            .outcome,
        org_proto::Outcome::Result(_)
    ));

    let emp_id = new_uuid_v7();
    let hire_req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: emp_id.clone(),
            home_team_id: root_team_id,
            role_id: None,
            proposal: HireProposalWire {
                name: "Alan Turing".into(),
                handle: "alan".into(),
                title: Some("Mathematician".into()),
                focus: None,
                role_text: None,
                title_in_team: None,
                reports_to_employee_id: Some(owner_id.clone()),
                office_id: None,
                model_profile_id: Some(prof_id),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("workspace_edits".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: None,
            },
            reason: "Multi-turn test".into(),
        },
    )
    .unwrap();
    let resp = office
        .dispatch_hiring_rpc(Some(&owner_id), &hire_req)
        .unwrap();
    let hired: org_proto::hiring::HireOpResult = match resp.outcome {
        org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
    };
    let agent_id = hired.employee_id.unwrap();

    // Two-turn script that stays alive after turn 1, answers turn 2, then exits.
    let bin_dir = tempfile::tempdir().unwrap();
    let script_path = bin_dir.path().join("scripted-two-turn");
    let script = r#"#!/bin/sh
echo '{"kind":"ready"}'
read line1
printf '{"kind":"reply","text":"reply-one"}\n'
read line2
printf '{"kind":"reply","text":"reply-two"}\n'
"#;
    fs::write(&script_path, script).unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

    let mut registry = RuntimeRegistry::production();
    registry.register(Arc::new(ScriptedRuntime), Some(script_path));

    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(bin_dir.path().to_path_buf());
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("first objective".into()),
                workspace: None,
            },
        )
        .unwrap();

    // Wait for turn 1 completion.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let view = manager.inspect_session(&owner_id, Some(&agent_id)).unwrap();
        if view.session.last_reply.as_deref() == Some("reply-one") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Record accepted command mid-turn (High #2).
    manager.record_accepted_command(&agent_id, "hr.hire.draft");
    let mid_turn = manager
        .store()
        .read_active_turn(&agent_id)
        .unwrap()
        .unwrap();
    assert!(
        mid_turn
            .accepted_commands
            .contains(&"hr.hire.draft".to_owned()),
        "accepted command must be persisted mid-turn"
    );

    // Send turn #2 objective on same live session (Medium #6).
    manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("second objective".into()),
                workspace: None,
            },
        )
        .unwrap();
    let deadline2 = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline2 {
        let view = manager.inspect_session(&owner_id, Some(&agent_id)).unwrap();
        if view.session.last_reply.as_deref() == Some("reply-two") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let _ = manager.stop_session(&agent_id, "done");
    let after_stop = manager
        .store()
        .read_active_turn(&agent_id)
        .unwrap()
        .unwrap();
    assert!(
        after_stop.objective.contains("second objective"),
        "turn #2 objective must be persisted, got: {}",
        after_stop.objective
    );
    assert!(
        after_stop
            .accepted_commands
            .contains(&"hr.hire.draft".to_owned()),
        "accepted_commands must NOT be wiped by stop_session/finalize_child: {:?}",
        after_stop.accepted_commands
    );

    // Starting again after the pump exited cleanly respawns a fresh session (Medium #6).
    let respawned = manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("third objective after exit".into()),
                workspace: None,
            },
        )
        .unwrap();
    assert_eq!(respawned.employee_id, agent_id);
    let _ = manager.stop_session(&agent_id, "cleanup");
}

#[test]
fn session_store_update_active_turn_serializes_concurrent_writers() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path(), "office-1");
    let initial = ActiveTurnRecord {
        version: 1,
        employee_id: "emp-conc".into(),
        run_id: "run-c".into(),
        session_id: "sess-c".into(),
        runtime_session_id: None,
        mark: "m-c".into(),
        pid: Some(42),
        pgid: Some(42),
        started_identity: Some("1:1".into()),
        boot_identity: Some("boot-1".into()),
        objective: "initial".into(),
        accepted_commands: Vec::new(),
        resume_attempts: 0,
        consecutive_stalls: 0,
        stall_locked: false,
        recovery_required: false,
        active: true,
        updated_at: "2026-11-21T00:00:00.000Z".into(),
    };
    store.write_active_turn(&initial).unwrap();

    let s1 = store.clone();
    let s2 = store.clone();
    let t1 = std::thread::spawn(move || {
        for i in 0..20 {
            s1.update_active_turn("emp-conc", |rec| {
                rec.accepted_commands.push(format!("cmd-{i}"));
            })
            .unwrap();
        }
    });
    let t2 = std::thread::spawn(move || {
        for i in 0..20 {
            s2.update_active_turn("emp-conc", |rec| {
                rec.objective = format!("obj-{i}");
                rec.runtime_session_id = Some("rs-final".into());
            })
            .unwrap();
        }
    });
    t1.join().unwrap();
    t2.join().unwrap();

    let final_rec = store.read_active_turn("emp-conc").unwrap().unwrap();
    assert_eq!(
        final_rec.accepted_commands.len(),
        20,
        "all 20 concurrent accepted_commands appends must survive"
    );
    assert_eq!(final_rec.runtime_session_id.as_deref(), Some("rs-final"));
    assert_eq!(final_rec.objective, "obj-19");
}

#[test]
fn enrollment_submit_command_cache_hashes_params_without_retaining_plaintext_code() {
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.enrollment_driver = Arc::new(FakeEnrollmentDriver {
        secret: "sk-ant-oat-secret-vault-value".into(),
    });
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    let conn_id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c9".to_owned();
    let start_req = Request::new::<ProviderEnrollmentStart>(
        1,
        &ProviderEnrollmentStartParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id.clone(),
            provider: Some("anthropic".into()),
        },
    )
    .unwrap();
    let start_resp = manager.dispatch_rpc(Some(&owner_id), &start_req).unwrap();
    let login_id = match start_resp.outcome {
        org_proto::Outcome::Result(v) => v["login_id"].as_str().unwrap().to_owned(),
        org_proto::Outcome::Error(e) => panic!("start failed: {e:?}"),
    };

    let raw_code = "sk-ant-oat-raw-callback-code-must-not-be-cached-9988";
    let submit_cmd_id = new_uuid_v7();
    let submit_req = Request::new::<org_proto::sessions::ProviderEnrollmentSubmit>(
        2,
        &org_proto::sessions::ProviderEnrollmentSubmitParams {
            command_id: submit_cmd_id.clone(),
            connection_id: conn_id.clone(),
            login_id: login_id.clone(),
            code: raw_code.to_owned(),
        },
    )
    .unwrap();
    let first = manager.dispatch_rpc(Some(&owner_id), &submit_req).unwrap();
    assert!(matches!(first.outcome, org_proto::Outcome::Result(_)));

    // Identical replay succeeds.
    let replay = manager.dispatch_rpc(Some(&owner_id), &submit_req).unwrap();
    assert!(matches!(replay.outcome, org_proto::Outcome::Result(_)));

    // Reusing same command_id with different code returns Conflict.
    let mismatch_req = Request::new::<org_proto::sessions::ProviderEnrollmentSubmit>(
        3,
        &org_proto::sessions::ProviderEnrollmentSubmitParams {
            command_id: submit_cmd_id,
            connection_id: conn_id,
            login_id,
            code: "different-code-123".into(),
        },
    )
    .unwrap();
    let mismatch = manager
        .dispatch_rpc(Some(&owner_id), &mismatch_req)
        .unwrap();
    assert!(matches!(
        mismatch.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::Conflict
    ));

    // Fingerprints in command_cache are SHA-256 hex digests and NEVER retain raw code.
    let fps = manager.command_cache_fingerprints();
    assert!(!fps.is_empty());
    for fp in &fps {
        assert_eq!(fp.len(), 64, "fingerprint must be 64-char SHA-256 hex");
        assert!(
            !fp.contains(raw_code),
            "raw enrollment code must never be stored in command_cache"
        );
    }
}

#[test]
fn c4_session_list_and_transcript_rpcs_enforce_auth_and_redaction() {
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let snap = office.org_snapshot().unwrap();
    let hr_agent_id = snap
        .members
        .iter()
        .find(|m| m.kind == org_proto::org::MemberKind::Agent)
        .expect("built-in HR agent exists")
        .id
        .clone();

    let home = office.ensure_private_home(&hr_agent_id).unwrap();
    let proj_dir = home.join(".claude/projects/-workspace");
    fs::create_dir_all(&proj_dir).unwrap();
    let sample = [
        r#"{"type":"user","timestamp":"2026-11-22T04:00:00Z","message":{"role":"user","content":"Check sk-ant-api03-SECRETVALUE12345"}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-22T04:00:01Z","message":{"model":"claude-sonnet-4-6","content":[{"type":"text","text":"Checked."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"README.md"}}]}}"#,
    ]
    .join("\n");
    fs::write(proj_dir.join("sess-hr-1.jsonl"), sample).unwrap();

    let sock_dir = tempfile::tempdir().unwrap();
    let cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    // Owner lists sessions for HR agent.
    let list_req = Request::new::<org_proto::sessions::MemberSessionList>(
        1,
        &org_proto::sessions::MemberSessionListParams {
            employee_id: Some(hr_agent_id.clone()),
            limit: Some(10),
        },
    )
    .unwrap();
    let list_resp = manager.dispatch_rpc(Some(&owner_id), &list_req).unwrap();
    let listed: org_proto::sessions::SessionListResult = match list_resp.outcome {
        org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        org_proto::Outcome::Error(e) => panic!("list failed: {e:?}"),
    };
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].session_id, "sess-hr-1");
    assert!(!listed.sessions[0]
        .preview
        .as_deref()
        .unwrap_or("")
        .contains("SECRETVALUE12345"));

    // Owner reads transcript with toggles.
    let tx_req = Request::new::<org_proto::sessions::MemberSessionTranscript>(
        2,
        &org_proto::sessions::MemberSessionTranscriptParams {
            employee_id: Some(hr_agent_id.clone()),
            session_id: Some("sess-hr-1".into()),
            include_tool_calls: Some(false),
            include_file_changes: Some(true),
            include_subagents: Some(true),
            cursor: Some(0),
            limit: Some(50),
        },
    )
    .unwrap();
    let tx_resp = manager.dispatch_rpc(Some(&owner_id), &tx_req).unwrap();
    let tx: org_proto::sessions::SessionTranscriptResult = match tx_resp.outcome {
        org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        org_proto::Outcome::Error(e) => panic!("transcript failed: {e:?}"),
    };
    assert_eq!(tx.session_id, "sess-hr-1");
    assert_eq!(tx.files.len(), 1);
    assert_eq!(tx.files[0].path, "README.md");
    assert!(tx
        .entries
        .iter()
        .all(|e| e.kind != org_proto::sessions::TranscriptEntryKind::ToolCall));
    assert!(!serde_json::to_string(&tx)
        .unwrap()
        .contains("SECRETVALUE12345"));

    // Unbound caller and path-traversal session_id are refused.
    let unbound = manager.dispatch_rpc(None, &tx_req).unwrap();
    assert!(matches!(
        unbound.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::NotAuthorized
    ));
    let bad_sid = Request::new::<org_proto::sessions::MemberSessionTranscript>(
        3,
        &org_proto::sessions::MemberSessionTranscriptParams {
            employee_id: Some(hr_agent_id.clone()),
            session_id: Some("../etc/passwd".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let bad_resp = manager.dispatch_rpc(Some(&owner_id), &bad_sid).unwrap();
    assert!(matches!(
        bad_resp.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::InvalidRequest
    ));

    // Another agent cannot inspect HR's transcript.
    let fake_other_agent = new_uuid_v7();
    let cross = manager
        .dispatch_rpc(Some(&fake_other_agent), &tx_req)
        .unwrap();
    assert!(matches!(
        cross.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::NotAuthorized
    ));
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

#[test]
fn real_process_fake_cli_runs_native_agy_and_codex_adapters_end_to_end() {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (agy_id, agy_conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "google",
        "agy",
        "neo-agy",
        "gemini-2.5-pro",
    );
    let (codex_id, codex_conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "openai",
        "codex",
        "gpt-codex",
        "gpt-5.4",
    );

    let bin_dir = tempfile::tempdir().unwrap();
    // Because enforces_deny_rules=false and fixed denies exist, workspace_edits narrows
    // to Ask ("ask" on Agy, "workspace-write" + "untrusted" on Codex).
    let fake_agy = write_fake_cli(
        bin_dir.path(),
        "agy",
        r#"#!/bin/sh
read init_line
echo '{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["neo-agy"],"mode":"ask","version":"1.5.14"}}}'
read user_line
mkdir -p "$HOME/.gemini/antigravity-cli/brain/agy-live-1/.system_generated/logs"
printf '{"step_index":1,"type":"PLANNER_RESPONSE","created_at":"2026-11-22T06:00:00Z","content":"Transcript leak key=%s"}\n' "$GEMINI_API_KEY" > "$HOME/.gemini/antigravity-cli/brain/agy-live-1/.system_generated/logs/transcript.jsonl"
echo '{"type":"system","subtype":"init","conversation_id":"agy-live-1","mode":"ask"}'
printf '{"type":"assistant","message":{"id":"m1","model":"gemini-2.5-pro","content":[{"type":"text","text":"Agy reply tok=%s key=%s"}],"usage":{"input_tokens":20,"output_tokens":10}}}\n' "$AGENT_OS_SESSION_TOKEN" "$GEMINI_API_KEY"
echo '{"type":"result","subtype":"success","is_error":false}'
"#,
    );
    let fake_codex = write_fake_cli(
        bin_dir.path(),
        "codex",
        r#"#!/bin/sh
read init_line
echo '{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"codex","version":"0.169.0"}}}'
read initialized_line
read thread_line
echo '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"codex-live-1","sandbox":"workspace-write","approvalPolicy":"untrusted"}}}'
read turn_line
mkdir -p "$HOME/.codex/sessions/2026/11/22"
printf '{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"Transcript leak key=%s"}}\n' "$OPENAI_API_KEY" > "$HOME/.codex/sessions/2026/11/22/codex-live-1.jsonl"
echo '{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"t1"}}}'
printf '{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"id":"i1","type":"agentMessage","text":"Codex reply tok=%s key=%s"}}}\n' "$AGENT_OS_SESSION_TOKEN" "$OPENAI_API_KEY"
echo '{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":30,"outputTokens":15}}}}'
echo '{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"t1","status":"completed"}}}'
"#,
    );

    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_agy::AgyRuntime::new()),
        Some(fake_agy),
    );
    registry.register(
        Arc::new(org_agent_runtime_codex::CodexRuntime::new()),
        Some(fake_codex),
    );
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(bin_dir.path().to_path_buf());
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    // Verify CLI enrollment aliases ("agy" and "gpt") and credential vault injection.
    let agy_secret = "AIzaSyAgyTestKeySecretValue987654";
    let codex_secret = "sk-proj-CodexTestKeySecretValue987654";
    let agy_enroll = manager
        .enrollment()
        .start(&owner_id, &agy_conn_id, "agy")
        .expect("agy alias enrollment starts");
    assert_eq!(agy_enroll.state, EnrollmentState::AwaitingCode);
    let agy_done = manager
        .enrollment()
        .submit_code(&owner_id, &agy_conn_id, &agy_enroll.login_id, agy_secret)
        .expect("agy credential stored");
    assert_eq!(agy_done.state, EnrollmentState::Completed);

    let codex_enroll = manager
        .enrollment()
        .start(&owner_id, &codex_conn_id, "gpt")
        .expect("gpt alias enrollment starts");
    assert_eq!(codex_enroll.state, EnrollmentState::PendingBrowser);
    let codex_done = manager
        .enrollment()
        .submit_code(
            &owner_id,
            &codex_conn_id,
            &codex_enroll.login_id,
            codex_secret,
        )
        .expect("codex credential stored");
    assert_eq!(codex_done.state, EnrollmentState::Completed);

    for (agent_id, expected_prefix, expected_sid, secret) in [
        (
            &agy_id,
            "Agy reply tok=[REDACTED] key=[REDACTED]",
            "agy-live-1",
            agy_secret,
        ),
        (
            &codex_id,
            "Codex reply tok=[REDACTED] key=[REDACTED]",
            "codex-live-1",
            codex_secret,
        ),
    ] {
        manager
            .start_session(
                &owner_id,
                &MemberSessionStartParams {
                    command_id: new_uuid_v7(),
                    employee_id: agent_id.clone(),
                    instruction: Some("Say hello".into()),
                    workspace: None,
                },
            )
            .expect("session started");
        let reply = wait_for_reply(&manager, &owner_id, agent_id);
        assert!(reply.contains(expected_prefix), "unexpected reply: {reply}");
        assert!(!reply.contains(secret), "secret leaked in reply: {reply}");
        let stored = manager.store().read_active_turn(agent_id).unwrap().unwrap();
        assert_eq!(stored.runtime_session_id.as_deref(), Some(expected_sid));

        // Verify C4 transcript list + read with secret redaction for both Agy and Codex.
        let list = manager
            .list_sessions(
                &owner_id,
                &org_proto::sessions::MemberSessionListParams {
                    employee_id: Some(agent_id.clone()),
                    limit: Some(10),
                },
            )
            .expect("list_sessions");
        assert!(
            list.sessions.iter().any(|s| s.session_id == expected_sid),
            "expected {expected_sid} in {:?}",
            list.sessions
        );
        let tx = manager
            .read_session_transcript(
                &owner_id,
                &org_proto::sessions::MemberSessionTranscriptParams {
                    employee_id: Some(agent_id.clone()),
                    session_id: Some(expected_sid.into()),
                    include_tool_calls: Some(true),
                    include_file_changes: Some(true),
                    include_subagents: Some(true),
                    cursor: None,
                    limit: Some(50),
                },
            )
            .expect("read_session_transcript");
        let combined: String = tx
            .entries
            .iter()
            .filter_map(|e| e.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            combined.contains("Transcript leak key=[REDACTED]"),
            "unexpected transcript text: {combined}"
        );
        assert!(
            !combined.contains(secret),
            "secret leaked in transcript: {combined}"
        );

        let _ = manager.stop_session(agent_id, "done");
    }
}
