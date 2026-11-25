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
            process_instructions: None,
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

#[test]
fn denied_office_globs_cover_durable_private_notes_and_protected_companion_trees() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    );
    let globs: Vec<String> = mgr
        .denied_office_globs()
        .into_iter()
        .map(|g| g.as_str().to_owned())
        .collect();
    let durable = office
        .durable_root()
        .canonicalize()
        .unwrap_or_else(|_| office.durable_root().to_path_buf());
    let office_local = durable.join("local/offices").join(&office.info().office_id);
    for expected in [
        durable.join("private"),
        durable.join("messaging"),
        durable.join("budgets"),
        durable.join("accounting"),
        durable.join("recovery"),
        office_local.join("security"),
        office_local.join("subscription-usage"),
    ] {
        let needle = format!("{}/**", expected.display());
        assert!(
            globs.contains(&needle),
            "denied_office_globs missing `{needle}`; got {globs:?}"
        );
    }
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
    let mut bypass_req = ApprovalRequest::new(
        ApprovalId::new("a1"),
        ApprovalKind::Command,
        Escalation::SandboxBypass,
        "Bash",
        serde_json::json!({ "command": "ls" }),
    );
    bypass_req.capability = Some(org_agent_runtime::ToolCapability::Shell);
    assert!(matches!(
        decide_escalation(
            &bypass_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::AutoDeny { .. }
    ));

    // PermissionChange ALWAYS escalates to owner.
    let mut perm_req = ApprovalRequest::new(
        ApprovalId::new("a2"),
        ApprovalKind::Permissions,
        Escalation::PermissionChange,
        "Edit",
        serde_json::json!({}),
    );
    perm_req.capability = Some(org_agent_runtime::ToolCapability::Edit);
    assert!(matches!(
        decide_escalation(
            &perm_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::EscalateToOwner { .. }
    ));

    // Path-scoped deny rule beats allow rule under AutonomousSandboxed (High #7).
    let mut denied_path_req = ApprovalRequest::new(
        ApprovalId::new("a-deny-path"),
        ApprovalKind::Other,
        Escalation::None,
        "Read",
        serde_json::json!({ "file_path": "/office/durable/state/org.sqlite" }),
    );
    denied_path_req.capability = Some(org_agent_runtime::ToolCapability::Read);
    assert!(matches!(
        decide_escalation(
            &denied_path_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::AutoDeny { .. }
    ));

    // Opaque Bash/shell call fails closed (escalates, never auto-allows) when path denies exist (High #7).
    let mut opaque_bash_req = ApprovalRequest::new(
        ApprovalId::new("a-opaque-bash"),
        ApprovalKind::Command,
        Escalation::None,
        "Bash",
        serde_json::json!({}),
    );
    opaque_bash_req.capability = Some(org_agent_runtime::ToolCapability::Shell);
    assert!(matches!(
        decide_escalation(
            &opaque_bash_req,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::EscalateToOwner { .. }
    ));

    // Normal allow rule works for safe file path under AutonomousSandboxed, escalates under SafeAsk.
    let mut normal_req = ApprovalRequest::new(
        ApprovalId::new("a3"),
        ApprovalKind::FileChange,
        Escalation::None,
        "Edit",
        serde_json::json!({ "file_path": "/tmp/agent-home/notes.md" }),
    );
    normal_req.capability = Some(org_agent_runtime::ToolCapability::Edit);
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

#[test]
fn presets_support_multi_runtime_capabilities_and_safe_agent_os_prefix() {
    use org_agent_runtime::{PathPattern, ToolCapability};

    let ws = SpecPath::try_from("/tmp/agent-home").unwrap();
    let sock = SpecPath::try_from("/tmp/office.sock").unwrap();
    let glob = PathPattern::try_from("/office/durable/**").unwrap();

    let ro = build_session_policy(
        PermissionPreset::ReadOnly,
        true,
        &ws,
        Some(&sock),
        std::slice::from_ref(&glob),
        1,
    )
    .unwrap();
    let we = build_session_policy(
        PermissionPreset::WorkspaceEdits,
        true,
        &ws,
        Some(&sock),
        std::slice::from_ref(&glob),
        2,
    )
    .unwrap();
    let auto = build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &ws,
        Some(&sock),
        std::slice::from_ref(&glob),
        3,
    )
    .unwrap();

    // Presets emit runtime-neutral cap:* tokens, never hardcoded Claude tool names.
    for cfg in [&ro, &we, &auto] {
        for r in &cfg.policy.allow {
            assert!(
                ToolCapability::is_capability_token(r.tool.as_str()),
                "preset allow rule must use cap:* token, got {}",
                r.tool.as_str()
            );
        }
    }
    assert_eq!(
        ro.allowed_tools,
        Some(vec![
            ToolCapability::Read.to_tool_name(),
            ToolCapability::Search.to_tool_name(),
            ToolCapability::Shell.to_tool_name(),
        ])
    );

    // Multi-runtime shell requests for `agent-os` auto-allow in ReadOnly and WorkspaceEdits.
    let shell_cases = [
        (
            "Bash",
            serde_json::json!({ "command": "agent-os inbox --json" }),
        ),
        (
            "shell",
            serde_json::json!({ "command": ["agent-os", "inbox", "--json"] }),
        ),
        (
            "shell",
            serde_json::json!({ "command": ["/bin/zsh", "-lc", "agent-os status"] }),
        ),
        (
            "shell",
            serde_json::json!({ "command": "/bin/zsh -lc 'agent-os inbox --json'" }),
        ),
        (
            "run_command",
            serde_json::json!({ "CommandLine": "  agent-os" }),
        ),
    ];
    for (tool, input) in shell_cases {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("ok"),
            ApprovalKind::Command,
            Escalation::None,
            tool,
            input,
        );
        req.capability = Some(ToolCapability::Shell);
        assert_eq!(
            decide_escalation(&req, &ro.policy, PermissionPreset::ReadOnly),
            EscalationDisposition::AllowByPolicy,
            "ReadOnly should allow safe agent-os call on {tool}"
        );
        assert_eq!(
            decide_escalation(&req, &we.policy, PermissionPreset::WorkspaceEdits),
            EscalationDisposition::AllowByPolicy,
            "WorkspaceEdits should allow safe agent-os call on {tool}"
        );
    }

    // Word-boundary violations, shell metacharacters, zsh substitutions, injection, and spoofed fields all fail closed.
    let unsafe_inputs = [
        serde_json::json!({ "command": "agent-os-evil inbox" }),
        serde_json::json!({ "command": "agent-os2 inbox" }),
        serde_json::json!({ "command": "FOO=1 agent-os inbox" }),
        serde_json::json!({ "command": "agent-os inbox; rm -rf /tmp/x" }),
        serde_json::json!({ "command": "agent-os inbox && rm -rf /tmp/x" }),
        serde_json::json!({ "command": "agent-os inbox || id" }),
        serde_json::json!({ "command": "agent-os inbox | cat" }),
        serde_json::json!({ "command": "agent-os `whoami`" }),
        serde_json::json!({ "command": "agent-os $(whoami)" }),
        serde_json::json!({ "command": "agent-os status =(id)" }),
        serde_json::json!({ "command": "/bin/zsh -lc 'agent-os status =(id)'" }),
        serde_json::json!({ "command": ["/bin/zsh", "-lc", "agent-os status =(id)"] }),
        serde_json::json!({ "command": "/bin/zsh -lc 'agent-os status *(e:id:)'" }),
        serde_json::json!({ "command": "agent-os status ${x:-$(id)}" }),
        serde_json::json!({ "command": "agent-os status \"$HOME\"" }),
        serde_json::json!({ "command": "/bin/zsh -lc 'agent-os inbox' && id" }),
        serde_json::json!({ "command": "agent-os inbox > /tmp/out" }),
        serde_json::json!({ "command": "agent-os inbox < /etc/passwd" }),
        serde_json::json!({ "command": "agent-os inbox\nrm -rf /tmp/x" }),
        serde_json::json!({ "command": "agent-os inbox\rrm -rf /tmp/x" }),
        serde_json::json!({ "command": ["/bin/sh", "-c", "agent-os inbox && id"] }),
        serde_json::json!({ "command": ["/bin/sh", "-c", "agent-os inbox", "extra"] }),
        serde_json::json!({ "command": ["agent-os-fake", "inbox"] }),
        serde_json::json!({ "command": ["agent-os", "inbox;id"] }),
        serde_json::json!({ "command": "rm -rf /tmp/x", "description": "agent-os inbox" }),
        serde_json::json!({ "command": "agent-os inbox", "CommandLine": "rm -rf /tmp/x" }),
        serde_json::json!({ "reason": "agent-os inbox" }),
    ];
    for input in unsafe_inputs {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("bad"),
            ApprovalKind::Command,
            Escalation::None,
            "shell",
            input.clone(),
        );
        req.capability = Some(ToolCapability::Shell);
        assert!(
            matches!(
                decide_escalation(&req, &ro.policy, PermissionPreset::ReadOnly),
                EscalationDisposition::EscalateToOwner { .. }
            ),
            "ReadOnly must NOT auto-allow unsafe input: {input}"
        );
        assert!(
            matches!(
                decide_escalation(&req, &we.policy, PermissionPreset::WorkspaceEdits),
                EscalationDisposition::EscalateToOwner { .. }
            ),
            "WorkspaceEdits must NOT auto-allow unsafe input: {input}"
        );
    }

    // Codex `apply_patch` and Agy `edit_file` auto-allow in WorkspaceEdits & AutonomousSandboxed,
    // escalate in ReadOnly (or AutoDeny when ReadOnly `allowed_tools` is enforced).
    for tool in ["Edit", "apply_patch", "edit_file", "replace_file_content"] {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("ed"),
            ApprovalKind::FileChange,
            Escalation::None,
            tool,
            serde_json::json!({ "file_path": "/tmp/agent-home/src/lib.rs" }),
        );
        req.capability = Some(ToolCapability::Edit);
        assert!(matches!(
            decide_escalation(&req, &ro.policy, PermissionPreset::ReadOnly),
            EscalationDisposition::EscalateToOwner { .. }
        ));
        assert!(matches!(
            decide_escalation_with_tools(
                &req,
                &ro.policy,
                PermissionPreset::ReadOnly,
                ro.allowed_tools.as_deref()
            ),
            EscalationDisposition::AutoDeny { .. }
        ));
        assert_eq!(
            decide_escalation(&req, &we.policy, PermissionPreset::WorkspaceEdits),
            EscalationDisposition::AllowByPolicy
        );
        assert_eq!(
            decide_escalation(&req, &auto.policy, PermissionPreset::AutonomousSandboxed),
            EscalationDisposition::AllowByPolicy
        );
    }

    // Opaque Codex patch approval without target paths fails closed when path denies exist,
    // while correlated `changes` targets are checked against fixed denies and allowed when safe.
    let mut opaque_patch = ApprovalRequest::new(
        ApprovalId::new("patch-opaque"),
        ApprovalKind::FileChange,
        Escalation::None,
        "apply_patch",
        serde_json::json!({ "itemId": "fc_1", "threadId": "thr_1", "turnId": "t1" }),
    );
    opaque_patch.capability = Some(ToolCapability::Edit);
    assert!(matches!(
        decide_escalation(&opaque_patch, &we.policy, PermissionPreset::WorkspaceEdits),
        EscalationDisposition::EscalateToOwner { .. }
    ));

    let mut denied_patch = opaque_patch.clone();
    denied_patch.input = serde_json::json!({
        "itemId": "fc_2",
        "changes": [{ "path": "/office/durable/state/org.sqlite", "kind": "update" }]
    });
    assert!(matches!(
        decide_escalation(&denied_patch, &we.policy, PermissionPreset::WorkspaceEdits),
        EscalationDisposition::AutoDeny { .. }
    ));

    let mut safe_patch = opaque_patch.clone();
    safe_patch.input = serde_json::json!({
        "itemId": "fc_3",
        "changes": [{ "path": "/tmp/agent-home/src/lib.rs", "kind": "update" }]
    });
    assert_eq!(
        decide_escalation(&safe_patch, &we.policy, PermissionPreset::WorkspaceEdits),
        EscalationDisposition::AllowByPolicy
    );

    // Map-shaped `changes` object keys are checked against fixed path denies using the same
    // extracted target list used by `has_concrete_file_target`.
    let mut map_denied_patch = opaque_patch.clone();
    map_denied_patch.input = serde_json::json!({
        "itemId": "fc_map_deny",
        "changes": { "/office/durable/state/org.sqlite": { "type": "update" } }
    });
    assert!(matches!(
        decide_escalation(
            &map_denied_patch,
            &we.policy,
            PermissionPreset::WorkspaceEdits
        ),
        EscalationDisposition::AutoDeny { .. }
    ));

    let mut map_safe_patch = opaque_patch.clone();
    map_safe_patch.input = serde_json::json!({
        "itemId": "fc_map_ok",
        "changes": { "/tmp/agent-home/src/lib.rs": { "type": "update" } }
    });
    assert_eq!(
        decide_escalation(
            &map_safe_patch,
            &we.policy,
            PermissionPreset::WorkspaceEdits
        ),
        EscalationDisposition::AllowByPolicy
    );

    // Legacy `fileChanges` map keys also hit fixed path denies directly.
    let mut legacy_fc_denied = opaque_patch.clone();
    legacy_fc_denied.input = serde_json::json!({
        "callId": "fc_legacy_deny",
        "fileChanges": { "/office/durable/state/org.sqlite": { "type": "update" } }
    });
    assert!(matches!(
        decide_escalation(
            &legacy_fc_denied,
            &we.policy,
            PermissionPreset::WorkspaceEdits
        ),
        EscalationDisposition::AutoDeny { .. }
    ));

    // Explicitly unknown wire capability (`Some(ToolCapability::Unknown)`) never auto-approves,
    // even if a concrete-tool or wildcard allow rule would otherwise match.
    let mut unknown_cap = ApprovalRequest::new(
        ApprovalId::new("unk-cap"),
        ApprovalKind::Other,
        Escalation::None,
        "custom_tool",
        serde_json::json!({ "path": "/tmp/agent-home/ok.txt" }),
    );
    unknown_cap.capability = Some(ToolCapability::Unknown);
    let mut wildcard_allow_policy = auto.policy.clone();
    wildcard_allow_policy.allow.push(org_agent_runtime::Rule {
        tool: org_agent_runtime::RuleTool::try_from("custom_tool").unwrap(),
        scope: org_agent_runtime::RuleScope::Any,
    });
    assert!(matches!(
        decide_escalation(
            &unknown_cap,
            &wildcard_allow_policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::EscalateToOwner { .. }
    ));

    // ApprovalKind::Permissions with Escalation::None still fails closed via effective_escalation().
    let mut sneaky_perm = ApprovalRequest::new(
        ApprovalId::new("sp"),
        ApprovalKind::Permissions,
        Escalation::None,
        "Edit",
        serde_json::json!({ "file_path": "/tmp/agent-home/a.rs" }),
    );
    sneaky_perm.capability = Some(ToolCapability::Edit);
    assert!(matches!(
        decide_escalation(
            &sneaky_perm,
            &auto.policy,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::EscalateToOwner { .. }
    ));

    // blocked_path targeting a denied office glob auto-denies even when input omits the path.
    let mut blocked = ApprovalRequest::new(
        ApprovalId::new("bp"),
        ApprovalKind::Command,
        Escalation::None,
        "shell",
        serde_json::json!({ "command": "agent-os inbox" }),
    );
    blocked.capability = Some(ToolCapability::Shell);
    blocked.blocked_path = Some("/office/durable/state/org.sqlite".to_owned());
    assert!(matches!(
        decide_escalation(&blocked, &ro.policy, PermissionPreset::ReadOnly),
        EscalationDisposition::AutoDeny { .. }
    ));

    // Escalated requests (`PermissionChange` via `grantRoot`) still hit hard refusals
    // (`allowed_tools` / fixed denies) BEFORE routing to the owner.
    let mut ro_grant_root = ApprovalRequest::new(
        ApprovalId::new("ro-grant-root"),
        ApprovalKind::FileChange,
        Escalation::PermissionChange,
        "apply_patch",
        serde_json::json!({
            "itemId": "fc_ro",
            "grantRoot": "/etc",
            "changes": [{ "path": "/tmp/agent-home/a.rs", "kind": "update" }]
        }),
    );
    ro_grant_root.capability = Some(ToolCapability::Edit);
    assert!(matches!(
        decide_escalation_with_tools(
            &ro_grant_root,
            &ro.policy,
            PermissionPreset::ReadOnly,
            ro.allowed_tools.as_deref()
        ),
        EscalationDisposition::AutoDeny { .. }
    ));

    let mut denied_perm_change = ApprovalRequest::new(
        ApprovalId::new("deny-perm-change"),
        ApprovalKind::FileChange,
        Escalation::PermissionChange,
        "apply_patch",
        serde_json::json!({
            "itemId": "fc_office",
            "grantRoot": "/office/durable/state",
            "changes": [{ "path": "/office/durable/state/org.sqlite", "kind": "update" }]
        }),
    );
    denied_perm_change.capability = Some(ToolCapability::Edit);
    assert!(matches!(
        decide_escalation(
            &denied_perm_change,
            &we.policy,
            PermissionPreset::WorkspaceEdits
        ),
        EscalationDisposition::AutoDeny { .. }
    ));

    // Command-prefix denies decode quoted executables & shell wrappers consistently with allows.
    let mut deny_policy = we.policy.clone();
    deny_policy.deny.push(org_agent_runtime::Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: org_agent_runtime::RuleScope::CommandPrefix(
            org_agent_runtime::CommandPrefix::try_from("agent-os").unwrap(),
        ),
    });
    for quoted_cmd in [
        serde_json::json!({ "command": "'agent-os' inbox" }),
        serde_json::json!({ "command": "\"agent-os\" inbox" }),
        serde_json::json!({ "command": "/bin/zsh -lc \"'agent-os' inbox\"" }),
        serde_json::json!({ "command": ["/bin/zsh", "-lc", "'agent-os' inbox"] }),
        serde_json::json!({ "command": "'agent-os' inbox && echo ok" }),
        serde_json::json!({ "command": "echo ok; \"agent-os\" inbox" }),
    ] {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("deny-quoted"),
            ApprovalKind::Command,
            Escalation::None,
            "shell",
            quoted_cmd.clone(),
        );
        req.capability = Some(ToolCapability::Shell);
        assert!(
            matches!(
                decide_escalation(&req, &deny_policy, PermissionPreset::WorkspaceEdits),
                EscalationDisposition::AutoDeny { .. }
            ),
            "quoted command must hit CommandPrefix deny consistently: {quoted_cmd}"
        );
        assert!(
            matches!(
                decide_escalation(&req, &deny_policy, PermissionPreset::AutonomousSandboxed),
                EscalationDisposition::AutoDeny { .. }
            ),
            "quoted/compound denied command must never fall through to AutonomousSandboxed shell Any allow: {quoted_cmd}"
        );
    }

    // Quote-aware CommandPrefix deny must NOT mistake quoted argument text or metadata
    // (`description`/`reason`) for an executable invocation.
    let mut quoted_arg_req = ApprovalRequest::new(
        ApprovalId::new("quoted-arg-not-denied"),
        ApprovalKind::Command,
        Escalation::None,
        "shell",
        serde_json::json!({ "command": "printf 'note; agent-os help'" }),
    );
    quoted_arg_req.capability = Some(ToolCapability::Shell);
    assert!(
        matches!(
            decide_escalation(
                &quoted_arg_req,
                &deny_policy,
                PermissionPreset::WorkspaceEdits
            ),
            EscalationDisposition::EscalateToOwner { .. }
        ),
        "quoted argument containing `; agent-os` must NOT be falsely auto-denied"
    );

    let mut metadata_desc_req = ApprovalRequest::new(
        ApprovalId::new("metadata-desc-not-denied"),
        ApprovalKind::Command,
        Escalation::None,
        "shell",
        serde_json::json!({
            "command": "git status",
            "description": "Check repo before running agent-os help"
        }),
    );
    metadata_desc_req.capability = Some(ToolCapability::Shell);
    let mut auto_with_agent_os_deny = auto.policy.clone();
    auto_with_agent_os_deny.deny.push(org_agent_runtime::Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: org_agent_runtime::RuleScope::CommandPrefix(
            org_agent_runtime::CommandPrefix::try_from("agent-os").unwrap(),
        ),
    });
    assert_eq!(
        decide_escalation(
            &metadata_desc_req,
            &auto_with_agent_os_deny,
            PermissionPreset::AutonomousSandboxed
        ),
        EscalationDisposition::AllowByPolicy,
        "non-command description metadata must NOT trigger a CommandPrefix deny"
    );

    // Unbalanced quote or missing command with an active CommandPrefix deny escalates (never false-denies or auto-allows).
    for undecodable in [
        serde_json::json!({ "command": "echo 'unterminated" }),
        serde_json::json!({ "description": "agent-os help" }),
    ] {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("undecodable-deny"),
            ApprovalKind::Command,
            Escalation::None,
            "shell",
            undecodable.clone(),
        );
        req.capability = Some(ToolCapability::Shell);
        assert!(
            matches!(
                decide_escalation(
                    &req,
                    &auto_with_agent_os_deny,
                    PermissionPreset::AutonomousSandboxed
                ),
                EscalationDisposition::EscalateToOwner { .. }
            ),
            "undecodable input must escalate rather than auto-deny or auto-allow: {undecodable}"
        );
    }

    // Bash `#` comments and bare CR outside comments must NOT trigger false denies, while an
    // apostrophe or CRLF newline after a comment must NOT mask a real denied command.
    for safe_comment_cmd in [
        serde_json::json!({ "command": "printf ok # note; agent-os help" }),
        serde_json::json!({ "command": "printf ok # note\ragent-os help" }),
        serde_json::json!({ "command": "printf foo\ragent-os help" }),
    ] {
        let mut comment_only_req = ApprovalRequest::new(
            ApprovalId::new("comment-not-denied"),
            ApprovalKind::Command,
            Escalation::None,
            "shell",
            safe_comment_cmd.clone(),
        );
        comment_only_req.capability = Some(ToolCapability::Shell);
        assert!(
            matches!(
                decide_escalation(
                    &comment_only_req,
                    &deny_policy,
                    PermissionPreset::WorkspaceEdits
                ),
                EscalationDisposition::EscalateToOwner { .. }
            ),
            "Bash comment must NOT falsely auto-deny: {safe_comment_cmd}"
        );
    }

    for denied_with_comment in [
        serde_json::json!({ "command": "agent-os inbox # don't touch" }),
        serde_json::json!({ "command": "echo ok # comment\nagent-os inbox" }),
        serde_json::json!({ "command": "echo ok # comment\r\nagent-os inbox" }),
        serde_json::json!({ "command": "echo foo#bar; agent-os inbox" }),
    ] {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("comment-denied"),
            ApprovalKind::Command,
            Escalation::None,
            "shell",
            denied_with_comment.clone(),
        );
        req.capability = Some(ToolCapability::Shell);
        assert!(
            matches!(
                decide_escalation(
                    &req,
                    &auto_with_agent_os_deny,
                    PermissionPreset::AutonomousSandboxed
                ),
                EscalationDisposition::AutoDeny { .. }
            ),
            "denied command with trailing/multiline/mid-word `#` must still AutoDeny: {denied_with_comment}"
        );
    }

    // Trailing bare CR argument stays preserved (matches `CommandPrefix("printf ")`), and
    // backslash-LF line continuations match multi-word denies identically across one or two lines.
    let mut cr_and_cont_policy = auto.policy.clone();
    cr_and_cont_policy.deny.push(org_agent_runtime::Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: org_agent_runtime::RuleScope::CommandPrefix(
            org_agent_runtime::CommandPrefix::try_from("printf ").unwrap(),
        ),
    });
    cr_and_cont_policy.deny.push(org_agent_runtime::Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: org_agent_runtime::RuleScope::CommandPrefix(
            org_agent_runtime::CommandPrefix::try_from("agent-os inbox").unwrap(),
        ),
    });
    for denied_cmd in [
        serde_json::json!({ "command": "printf \r" }),
        serde_json::json!({ "command": "printf \\ " }),
        serde_json::json!({ "command": "printf \\\t" }),
        serde_json::json!({ "command": "agent-os inbox" }),
        serde_json::json!({ "command": "agent-os \\\ninbox" }),
        serde_json::json!({ "command": "ag\\\nent-os inbox" }),
        serde_json::json!({ "command": "\"agent-\\\nos\" inbox" }),
        serde_json::json!({ "command": "printf \\\n%s hello" }),
    ] {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("cr-or-cont-deny"),
            ApprovalKind::Command,
            Escalation::None,
            "shell",
            denied_cmd.clone(),
        );
        req.capability = Some(ToolCapability::Shell);
        assert!(
            matches!(
                decide_escalation(
                    &req,
                    &cr_and_cont_policy,
                    PermissionPreset::AutonomousSandboxed
                ),
                EscalationDisposition::AutoDeny { .. }
            ),
            "bare CR arg or backslash-LF continuation must hit CommandPrefix deny: {denied_cmd}"
        );
    }

    // Quoted literal CommandPrefix deny rules still match commands carrying those quotes verbatim.
    let mut quoted_prefix_deny = auto.policy.clone();
    quoted_prefix_deny.deny.push(org_agent_runtime::Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: org_agent_runtime::RuleScope::CommandPrefix(
            org_agent_runtime::CommandPrefix::try_from("'rm' -rf").unwrap(),
        ),
    });
    quoted_prefix_deny.deny.push(org_agent_runtime::Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: org_agent_runtime::RuleScope::CommandPrefix(
            org_agent_runtime::CommandPrefix::try_from("\"agent-os\"").unwrap(),
        ),
    });
    for quoted_literal_cmd in [
        serde_json::json!({ "command": "'rm' -rf /tmp/x" }),
        serde_json::json!({ "command": "echo ok; 'rm' -rf /tmp/x" }),
        serde_json::json!({ "command": "FOO=1 'rm' -rf /tmp/x" }),
        serde_json::json!({ "command": "FOO+=1 'rm' -rf /tmp/x" }),
        serde_json::json!({ "command": "FOO+=1 /bin/sh -c \"'rm' -rf /tmp/x\"" }),
        serde_json::json!({ "command": "FOO=\"a b\" BAR='x=y' 'rm' -rf /tmp/x" }),
        serde_json::json!({ "command": "/bin/zsh -lc \"FOO=1 'rm' -rf /tmp/x\"" }),
        serde_json::json!({ "command": "\"agent-os\" inbox" }),
        serde_json::json!({ "command": "ENV=1 \"agent-os\" inbox" }),
        serde_json::json!({ "command": "ENV+=1 \"agent-os\" inbox" }),
    ] {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("quoted-prefix-deny"),
            ApprovalKind::Command,
            Escalation::None,
            "shell",
            quoted_literal_cmd.clone(),
        );
        req.capability = Some(ToolCapability::Shell);
        assert!(
            matches!(
                decide_escalation(
                    &req,
                    &quoted_prefix_deny,
                    PermissionPreset::AutonomousSandboxed
                ),
                EscalationDisposition::AutoDeny { .. }
            ),
            "literal quoted CommandPrefix deny must match verbatim: {quoted_literal_cmd}"
        );
    }

    // Compound undecodable command with an applicable CommandPrefix deny fails closed
    // (escalates, never auto-allows) even under AutonomousSandboxed broad shell `Any` allow.
    let mut auto_with_rm_deny = auto.policy.clone();
    auto_with_rm_deny.deny.push(org_agent_runtime::Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: org_agent_runtime::RuleScope::CommandPrefix(
            org_agent_runtime::CommandPrefix::try_from("rm").unwrap(),
        ),
    });
    for ambiguous_cmd in [
        serde_json::json!({ "command": "echo hi && $(printf rm) -rf /tmp/x" }),
        serde_json::json!({ "command": "FOO=1 /bin/sh -c '${cmd}'" }),
        serde_json::json!({ "command": "FOO+=1 /bin/zsh -lc 'echo ok'" }),
        serde_json::json!({ "command": "/bin/zsh -lc 'FOO=1 git status'" }),
    ] {
        let mut compound_req = ApprovalRequest::new(
            ApprovalId::new("compound-undecodable"),
            ApprovalKind::Command,
            Escalation::None,
            "shell",
            ambiguous_cmd.clone(),
        );
        compound_req.capability = Some(ToolCapability::Shell);
        assert!(
            matches!(
                decide_escalation(
                    &compound_req,
                    &auto_with_rm_deny,
                    PermissionPreset::AutonomousSandboxed
                ),
                EscalationDisposition::EscalateToOwner { .. }
            ),
            "ambiguous or assignment-prefixed shell command must fail closed when a CommandPrefix deny exists: {ambiguous_cmd}"
        );
    }
}

#[test]
fn pump_evaluates_unredacted_approval_input_before_history_redaction() {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (emp_id, _conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "anthropic",
        "claude",
        "ada-redact",
        "claude-sonnet-4-6",
    );

    // Fake Claude CLI sends an approval whose command hides `;id` immediately after a Bearer
    // token so `redact_secrets` strips the `;id` suffix if redaction runs before policy.
    // Under WorkspaceEdits (which allows safe `agent-os` shell calls), the pump MUST evaluate
    // the original unredacted command (escalating to owner, never auto-allowing) while still
    // redacting the bearer secret in persisted history.
    let bin_dir = tempfile::tempdir().unwrap();
    let fake_claude = write_fake_cli(
        bin_dir.path(),
        "claude",
        r#"#!/bin/sh
read init_line
echo '{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ada-redact"],"current_permission_mode":"default","pid":1234,"version":"2.1.311"}}}'
read user_line
echo '{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","permissionMode":"default"}'
echo '{"type":"control_request","request_id":"apr-redact-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"agent-os inbox --body Bearer secret_tok_999;id"}}}'
read decision_line
echo '{"type":"result","subtype":"success","is_error":false,"result":"done"}'
"#,
    );

    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()),
        Some(fake_claude),
    );
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
                employee_id: emp_id.clone(),
                instruction: Some("Check inbox".into()),
                workspace: None,
            },
        )
        .expect("session started");

    // Poll until the session reaches WaitingApproval (proving it was NOT auto-allowed!).
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut escalated = false;
    while Instant::now() < deadline {
        if let Ok(res) = manager.inspect_session(&owner_id, Some(&emp_id)) {
            if res
                .session
                .pending_approvals
                .iter()
                .any(|p| p.approval_id == "apr-redact-1")
            {
                escalated = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        escalated,
        "approval hiding `;id` after Bearer token must escalate to owner (evaluated unredacted), never auto-allow"
    );

    // Verify persisted history redacted the bearer token.
    let history_path = manager.store().history_path(&emp_id).unwrap();
    let history = fs::read_to_string(&history_path).unwrap_or_default();
    assert!(
        !history.contains("secret_tok_999"),
        "persisted history must redact bearer secret: {history}"
    );
    assert!(
        history.contains("apr-redact-1"),
        "persisted history should contain the redacted approval event: {history}"
    );

    // Cleanly stop the waiting session.
    let _ = manager.stop_session(&emp_id, "test done");
}

#[test]
fn decide_approval_refuses_allow_always_for_permission_change_and_unknown_escalations() {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (emp_id, _conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "anthropic",
        "claude",
        "ada-esc",
        "claude-sonnet-4-6",
    );

    let bin_dir = tempfile::tempdir().unwrap();
    let fake_claude = write_fake_cli(
        bin_dir.path(),
        "claude",
        r#"#!/bin/sh
read init_line
echo '{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ada-esc"],"current_permission_mode":"default","pid":1235,"version":"2.1.311"}}}'
read user_line
echo '{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000002","permissionMode":"default"}'
echo '{"type":"control_request","request_id":"apr-perm-1","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{}}}'
echo '{"type":"control_request","request_id":"apr-unk-1","request":{"subtype":"can_use_tool","tool_name":"mystery_tool","input":{}}}'
read decision_line
echo '{"type":"result","subtype":"success","is_error":false,"result":"done"}'
"#,
    );

    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()),
        Some(fake_claude),
    );
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
                employee_id: emp_id.clone(),
                instruction: Some("Check escalations".into()),
                workspace: None,
            },
        )
        .expect("session started");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut both_pending = false;
    while Instant::now() < deadline {
        if let Ok(res) = manager.inspect_session(&owner_id, Some(&emp_id)) {
            let has_perm = res
                .session
                .pending_approvals
                .iter()
                .any(|p| p.approval_id == "apr-perm-1" && p.escalation == "permission_change");
            let has_unk = res
                .session
                .pending_approvals
                .iter()
                .any(|p| p.approval_id == "apr-unk-1" && p.escalation == "unknown");
            if has_perm && has_unk {
                both_pending = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        both_pending,
        "expected both permission_change and unknown escalations pending"
    );

    for apr_id in ["apr-perm-1", "apr-unk-1"] {
        let err = manager
            .decide_approval(&owner_id, &emp_id, apr_id, "allow_always", None)
            .expect_err("allow_always must be refused for escalated approvals");
        assert_eq!(err.code, org_proto::ErrorCode::InvalidRequest);
    }

    let _ = manager.stop_session(&emp_id, "test done");
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
            connection_id: conn_id.clone(),
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
    bind_and_refresh_test_connection(&office, &owner_id, &conn_id, "claude-sonnet-4-6");

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

    // Medium #5 + TASK-120/124/126: denied_office_globs_for surgically protects runtime DBs,
    // protected companion stores, git-state/git-home, snapshot-import, sibling agent worktrees,
    // host.sock, and Linux privileged sockets while keeping self agent home & worktree root open.
    let denied_strs: Vec<String> = manager
        .denied_office_globs_for(Some(&agent_id))
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    assert!(
        denied_strs.iter().any(|s| s.contains("enroll-tmp")),
        "denied_office_globs must include enroll-tmp: {denied_strs:?}"
    );
    assert!(
        !denied_strs.iter().any(|s| s.ends_with("/runtime/**")),
        "denied_office_globs must NOT blanket-deny runtime/** (blocks self task worktrees): {denied_strs:?}"
    );
    for needle in [
        "/runtime/snapshot-import/**",
        "/office.db",
        "/protected/**",
        "/git-state.json",
        "/git-home/**",
    ] {
        assert!(
            denied_strs.iter().any(|s| s.contains(needle)),
            "denied_office_globs must protect `{needle}`: {denied_strs:?}"
        );
    }
    let (wt_writable, wt_write_denies) = manager.worktree_sandbox_grants(&agent_id, false);
    assert!(
        wt_writable
            .iter()
            .any(|p| p.as_str().ends_with(&format!("/agents/{agent_id}"))),
        "worktree_sandbox_grants must include agent's own worktree root: {wt_writable:?}"
    );
    assert!(
        wt_write_denies
            .iter()
            .all(|p| !p.as_str().contains("/**/") && !p.as_str().contains("/*/")),
        "worktree_sandbox_grants must never emit interior wildcards (breaks Codex path_pattern_base): {wt_write_denies:?}"
    );
    assert!(
        denied_strs.iter().any(|s| s.ends_with("/host.sock")),
        "denied_office_globs must deny host.sock: {denied_strs:?}"
    );
    assert!(
        denied_strs.iter().any(|s| s.ends_with("/r/**")),
        "denied_office_globs must deny host rendezvous sockets r/**: {denied_strs:?}"
    );
    assert!(
        denied_strs.iter().any(|s| s == "/var/run/docker.sock"),
        "denied_office_globs must deny docker.sock: {denied_strs:?}"
    );
    assert!(
        !denied_strs.iter().any(|s| s.starts_with("/run/user/*")),
        "denied_office_globs must not emit /run/user/* wildcard (breaks Codex path_pattern_base): {denied_strs:?}"
    );
    let sibling_slug = crate::host_auth::agent_tmpdir_slug(&office.info().office_id, &owner_id);
    assert!(
        denied_strs
            .iter()
            .any(|s| s.contains(&format!("/t/{sibling_slug}/**"))),
        "denied_office_globs must pre-deny sibling short tmpdir before creation: {denied_strs:?}"
    );
    assert!(
        denied_strs
            .iter()
            .any(|s| s.contains(&format!("/agents/{owner_id}/**"))),
        "denied_office_globs must deny sibling agent dirs: {denied_strs:?}"
    );
    assert!(
        !denied_strs
            .iter()
            .any(|s| s.contains(&format!("/agents/{agent_id}/**"))),
        "denied_office_globs must NOT deny self agent dir: {denied_strs:?}"
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
            connection_id: conn_id.clone(),
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
    bind_and_refresh_test_connection(&office, &owner_id, &conn_id, "claude-sonnet-4-6");

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
    // to Ask ("ask" on Agy, "workspace-write" + "on-request" on Codex).
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
echo '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"codex-live-1"},"activePermissionProfile":{"id":"office","extends":null},"sandbox":{"type":"workspaceWrite","writableRoots":[]},"approvalPolicy":{"granular":{"sandbox_approval":false,"rules":true,"mcp_elicitations":false,"request_permissions":false,"skill_approval":false}}}}'
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

#[test]
fn session_launch_injects_effective_processes_and_refuses_mandatory_budget_overflow() {
    use org_proto::processes::{
        DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
    };

    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (agent_id, conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "google",
        "agy",
        "proc-bot",
        "gemini-2.5-pro",
    );

    // Seed a non-overridable org standard and an overridable guidance process.
    office
        .process_write(
            &owner_id,
            ProcessWriteParams {
                command_id: new_uuid_v7(),
                document_id: None,
                kind: DocumentKindWire::Standard,
                scope: ProcessScopeWire::Organization,
                key: Some("no-secrets".into()),
                title: "Never Log Secrets".into(),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(owner_id.clone()),
                overridable: false,
                body_markdown: "MANDATORY_SENTINEL: Never print API keys in stdout.".into(),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap();

    office
        .process_write(
            &owner_id,
            ProcessWriteParams {
                command_id: new_uuid_v7(),
                document_id: None,
                kind: DocumentKindWire::Process,
                scope: ProcessScopeWire::Organization,
                key: Some("pr-style".into()),
                title: "PR Style Guide".into(),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(owner_id.clone()),
                overridable: true,
                body_markdown: "GUIDANCE_SENTINEL: Keep PRs concise.".into(),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap();

    // Verify rendered instructions include mandatory standard in full and overridable guidance.
    let rendered = office
        .render_agent_process_instructions(
            &agent_id,
            None,
            None,
            std::collections::BTreeMap::new(),
            128 * 1024,
        )
        .unwrap()
        .unwrap();
    assert!(
        rendered
            .text
            .contains("MANDATORY_SENTINEL: Never print API keys in stdout."),
        "mandatory non-overridable standard missing: {}",
        rendered.text
    );
    assert!(
        rendered.text.contains("PR Style Guide"),
        "overridable guidance missing: {}",
        rendered.text
    );

    // Prepare a real SessionManager with a stub agy binary + enrolled credential.
    let tmp = tempfile::tempdir().unwrap();
    let capture_path = tmp.path().join("captured-init.txt");
    let fake_agy = write_fake_cli(
        tmp.path(),
        "agy",
        &format!(
            r#"#!/bin/sh
settings=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--settings" ]; then
    shift
    settings="$1"
  fi
  shift
done
cat "$(dirname "$settings")/agy-instructions.md" > "{capture}"
read init_line
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"host-1","response":{{"agents":["proc-bot"],"mode":"ask","version":"1.5.14"}}}}}}'
read user_line
echo '{{"type":"system","subtype":"init","conversation_id":"agy-proc-1","mode":"ask"}}'
echo '{{"type":"assistant","message":{{"id":"m1","model":"gemini-2.5-pro","content":[{{"type":"text","text":"Ack"}}],"usage":{{"input_tokens":5,"output_tokens":2}}}}}}'
echo '{{"type":"result","subtype":"success","is_error":false}}'
"#,
            capture = capture_path.display()
        ),
    );
    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_agy::AgyRuntime::new()),
        Some(fake_agy),
    );
    let mut cfg = SessionManagerConfig::production(tmp.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(tmp.path().to_path_buf());
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    // Enroll API key credential for the agent's connection so start_session reaches build_instructions.
    let enrolled = manager
        .enrollment()
        .start(&owner_id, &conn_id, "agy")
        .unwrap();
    manager
        .enrollment()
        .submit_code(
            &owner_id,
            &conn_id,
            &enrolled.login_id,
            "test-gemini-key-for-c5",
        )
        .unwrap();

    // 1. Live launch succeeds and injects the mandatory standard into the runtime's instructions!
    manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("Hello".into()),
                workspace: None,
            },
        )
        .expect("session starts with effective processes");
    let _ = wait_for_reply(&manager, &owner_id, &agent_id);
    let _ = manager.stop_session(&agent_id, "check capture");
    let captured = fs::read_to_string(&capture_path).expect("captured init");
    assert!(
        captured.contains("MANDATORY_SENTINEL: Never print API keys in stdout."),
        "launched runtime did not receive mandatory org standard in instructions: {captured}"
    );
    assert!(
        captured.contains("PR Style Guide"),
        "launched runtime did not receive overridable guidance in instructions: {captured}"
    );

    // 2. Seed a huge non-overridable standard that overflows the 128 KiB launch budget.
    office
        .process_write(
            &owner_id,
            ProcessWriteParams {
                command_id: new_uuid_v7(),
                document_id: None,
                kind: DocumentKindWire::Standard,
                scope: ProcessScopeWire::Organization,
                key: Some("huge-mandatory-policy".into()),
                title: "Oversized Mandatory Policy".into(),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(owner_id.clone()),
                overridable: false,
                body_markdown: "X".repeat(135 * 1024),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap();

    // Launch MUST fail closed when mandatory non-overridable standards exceed budget (doc-34 Decision 4).
    let err = manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id,
                instruction: Some("Start".into()),
                workspace: None,
            },
        )
        .expect_err("launch must refuse when mandatory standards overflow budget");
    assert_eq!(err.code, org_proto::ErrorCode::InvalidRequest);
    assert!(
        err.message.contains("Oversized Mandatory Policy")
            || err.message.contains("huge-mandatory-policy"),
        "refusal should name offending mandatory doc: {}",
        err.message
    );
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

#[test]
fn task_assignment_wake_injects_project_owning_chain_for_contributing_agent() {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (contrib_agent_id, conn_id, task_id, task_title) =
        seed_project_task_wake_fixture(&office, &owner_id, &root_team_id);

    let tmp = tempfile::tempdir().unwrap();
    let capture_path = tmp.path().join("captured-task-instructions.txt");
    let fake_agy = write_fake_cli(
        tmp.path(),
        "agy",
        &format!(
            r#"#!/bin/sh
settings=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--settings" ]; then
    shift
    settings="$1"
  fi
  shift
done
cat "$(dirname "$settings")/agy-instructions.md" > "{capture}"
read init_line
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"host-1","response":{{"agents":["qa-bot"],"mode":"ask","version":"1.5.14"}}}}}}'
read user_line
echo '{{"type":"system","subtype":"init","conversation_id":"agy-task-1","mode":"ask"}}'
echo '{{"type":"assistant","message":{{"id":"m1","model":"gemini-2.5-pro","content":[{{"type":"text","text":"Audited"}}],"usage":{{"input_tokens":5,"output_tokens":2}}}}}}'
echo '{{"type":"result","subtype":"success","is_error":false}}'
"#,
            capture = capture_path.display()
        ),
    );
    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_agy::AgyRuntime::new()),
        Some(fake_agy),
    );
    let mut cfg = SessionManagerConfig::production(tmp.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(tmp.path().to_path_buf());
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    let enrolled = manager
        .enrollment()
        .start(&owner_id, &conn_id, "agy")
        .unwrap();
    manager
        .enrollment()
        .submit_code(
            &owner_id,
            &conn_id,
            &enrolled.login_id,
            "test-key-task-wake",
        )
        .unwrap();

    // Wake contributing-team agent for the project task.
    let outcome = manager.on_task_assigned(
        &contrib_agent_id,
        &owner_id,
        "Owner",
        true,
        &task_id,
        &task_title,
    );
    assert_eq!(outcome, crate::WakeOutcome::Started);
    let _ = wait_for_reply(&manager, &owner_id, &contrib_agent_id);
    let _ = manager.stop_session(&contrib_agent_id, "done");

    let captured = fs::read_to_string(&capture_path).expect("captured task instructions");
    assert!(
        captured.contains("OWNING_TEAM_MANDATORY_SENTINEL"),
        "project owning team standard missing on task wake: {captured}"
    );
    assert!(
        captured.contains("PROJECT_MANDATORY_SENTINEL"),
        "project mandatory standard missing on task wake: {captured}"
    );
    assert!(
        !captured.contains("CONTRIB_TEAM_MUST_NOT_APPEAR"),
        "contributing team standard leaked into project task wake: {captured}"
    );
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

#[test]
fn task126_worktree_sandbox_grants_protect_git_metadata_and_compile_cleanly_for_codex() {
    use org_agent_runtime::{
        AgentIdentity, AgentName, AgentRuntime as _, LaunchContext, RuntimeId, SessionKind,
        SessionSpec, SpecPath,
    };
    use org_office::git::{OfficeGitState, TaskWorkspaceRecord};

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let hire_agent = |name: &str, handle: &str| -> String {
        let req = Request::new::<HrHire>(
            1,
            &HrHireParams {
                command_id: new_uuid_v7(),
                request_id: new_uuid_v7(),
                employee_id: new_uuid_v7(),
                home_team_id: root_team_id.clone(),
                role_id: None,
                proposal: HireProposalWire {
                    name: name.into(),
                    handle: handle.into(),
                    title: Some("Engineer".into()),
                    focus: None,
                    role_text: None,
                    title_in_team: Some("Engineer".into()),
                    reports_to_employee_id: Some(owner_id.clone()),
                    office_id: None,
                    model_profile_id: None,
                    model_id: None,
                    permission_preset: Some("autonomous_sandboxed".into()),
                    tools_config: Vec::new(),
                    system_prompt_markdown: None,
                    body: Some("Work.".into()),
                },
                reason: "Test hire".into(),
            },
        )
        .unwrap();
        let resp = office.dispatch_hiring_rpc(Some(&owner_id), &req).unwrap();
        let hired: org_proto::hiring::HireOpResult = match resp.outcome {
            org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
        };
        hired.employee_id.expect("hired employee id")
    };
    let self_emp_id = hire_agent("Self Agent", "self-agent");
    let peer_emp_id = hire_agent("Peer Agent", "peer-agent");

    let office_id = office.info().office_id.clone();
    let office_rt = office.runtime_root().join(&office_id);
    let repo_id = "01a212a8-982e-71ca-a6de-322dba762da2";
    let untargeted_repo_id = "01a212a8-982e-71ca-a6de-322dba762da9";
    let cache_dir = office_rt
        .join("repositories")
        .join(format!("{repo_id}.git"));
    let untargeted_cache = office_rt
        .join("repositories")
        .join(format!("{untargeted_repo_id}.git"));
    let self_wt = office_rt
        .join("teams")
        .join(&root_team_id)
        .join("agents")
        .join(&self_emp_id)
        .join("tasks/t-1/repositories")
        .join(repo_id);
    let peer_wt = office_rt
        .join("teams")
        .join(&root_team_id)
        .join("agents")
        .join(&peer_emp_id)
        .join("tasks/t-2/repositories")
        .join(repo_id);
    std::fs::create_dir_all(cache_dir.join("worktrees/self-admin")).unwrap();
    std::fs::create_dir_all(cache_dir.join("worktrees/peer-admin")).unwrap();
    std::fs::create_dir_all(cache_dir.join("worktrees/unknown-admin")).unwrap();
    std::fs::create_dir_all(&untargeted_cache).unwrap();
    std::fs::create_dir_all(&self_wt).unwrap();
    std::fs::create_dir_all(&peer_wt).unwrap();
    std::fs::write(self_wt.join(".git"), "gitdir: /dummy\n").unwrap();

    let mut git_state = OfficeGitState::default();
    git_state.workspaces.insert(
        OfficeGitState::workspace_key("t-1", repo_id, &self_emp_id),
        TaskWorkspaceRecord {
            office_id: office_id.clone(),
            employee_id: self_emp_id.clone(),
            task_id: "t-1".into(),
            task_key: "T-1".into(),
            repository_id: repo_id.into(),
            repository_key: Some("org".into()),
            branch: "task/T-1-test".into(),
            worktree_path: self_wt.display().to_string(),
            admin_dir_name: "self-admin".into(),
            base_commit: "abc".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    git_state.workspaces.insert(
        OfficeGitState::workspace_key("t-2", repo_id, &peer_emp_id),
        TaskWorkspaceRecord {
            office_id: office_id.clone(),
            employee_id: peer_emp_id.clone(),
            task_id: "t-2".into(),
            task_key: "T-2".into(),
            repository_id: repo_id.into(),
            repository_key: Some("org".into()),
            branch: "task/T-2-test".into(),
            worktree_path: peer_wt.display().to_string(),
            admin_dir_name: "peer-admin".into(),
            base_commit: "abc".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    git_state.save(office.runtime_root(), &office_id).unwrap();

    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    );
    let denied = mgr.denied_office_globs_for(Some(&self_emp_id));
    let denied_strs: Vec<String> = denied.iter().map(|p| p.as_str().to_owned()).collect();
    for needle in [
        "/worktrees/peer-admin/**",
        "/worktrees/unknown-admin/**",
        &format!("/repositories/{untargeted_repo_id}.git/**"),
        "/snapshot-export/**",
        "/office.db-journal",
        "/office.db.new",
        "/office.db.new-wal",
        "/credentials.toml",
        &format!("/teams/{root_team_id}/teams/**"),
    ] {
        assert!(
            denied_strs.iter().any(|s| s.ends_with(needle)),
            "denied_office_globs must deny `{needle}`: {denied_strs:?}"
        );
    }
    assert!(
        !denied_strs
            .iter()
            .any(|s| s.ends_with("/.cargo/config.toml") || s.ends_with("/.cargo/config")),
        "must not read-deny ~/.cargo/config.toml (breaks cargo under Linux bwrap): {denied_strs:?}"
    );
    assert!(
        !denied_strs
            .iter()
            .any(|s| s.ends_with("/worktrees/self-admin/**")),
        "self git admin dir must not be read-denied: {denied_strs:?}"
    );

    let (wt_writable, wt_write_denies) = mgr.worktree_sandbox_grants(&self_emp_id, false);
    let writable_strs: Vec<String> = wt_writable.iter().map(|p| p.as_str().to_owned()).collect();
    assert!(
        !writable_strs.iter().any(|s| s.ends_with("/worktrees")),
        "parent <cache>/worktrees must NEVER be in writable_roots: {writable_strs:?}"
    );
    assert!(
        writable_strs
            .iter()
            .any(|s| s.ends_with("/worktrees/self-admin")),
        "concrete self admin dir must be in writable_roots: {writable_strs:?}"
    );
    assert!(
        !writable_strs.iter().any(|s| s.contains(untargeted_repo_id)),
        "untargeted repo must NEVER be in writable_roots: {writable_strs:?}"
    );
    let deny_strs: Vec<String> = wt_write_denies
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    assert!(
        deny_strs
            .iter()
            .all(|s| !s.contains("/**/") && !s.contains("/*/")),
        "must have zero interior wildcards for Codex compatibility: {deny_strs:?}"
    );
    for needle in [
        "/hooks",
        "/hooks/**",
        "/objects/info",
        "/objects/info/**",
        "/config",
        "/HEAD",
        "/refs/remotes/**",
        "/worktrees/self-admin/commondir",
        "/worktrees/self-admin/gitdir",
        "/.git",
    ] {
        assert!(
            deny_strs.iter().any(|s| s.ends_with(needle)),
            "worktree_sandbox_grants missing write deny `{needle}`: {deny_strs:?}"
        );
    }
    // Optional non-existent bare-cache files (`packed-refs`, `shallow`, `config.worktree`) must
    // NOT be emitted into `deny_write` (Linux bwrap turns missing denyWrite files into EACCES stubs).
    for absent in [
        "/packed-refs",
        "/shallow",
        "/worktrees/self-admin/config.worktree",
        "/.git/hooks",
    ] {
        assert!(
            !deny_strs.iter().any(|s| s.ends_with(absent)),
            "absent optional git path `{absent}` must not be emitted into deny_write: {deny_strs:?}"
        );
    }
    // When `packed-refs` DOES exist on disk, it MUST be protected in `deny_write`.
    std::fs::write(cache_dir.join("packed-refs"), "# pack-refs\n").unwrap();
    let (_, wt_write_denies_with_packed) = mgr.worktree_sandbox_grants(&self_emp_id, false);
    assert!(
        wt_write_denies_with_packed
            .iter()
            .any(|p| p.as_str().ends_with("/packed-refs")),
        "existing packed-refs must be write-denied"
    );

    // Verify Codex adapter compiles the resulting policy without downgrading self worktree root.
    let ws_dir = tempfile::tempdir().unwrap();
    let rt_dir = tempfile::tempdir().unwrap();
    let tmp_dir = tempfile::tempdir().unwrap();
    let ws_spec =
        SpecPath::try_from(ws_dir.path().canonicalize().unwrap().to_str().unwrap()).unwrap();
    let mut preset = crate::preset::build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &ws_spec,
        None,
        &denied,
        1,
    )
    .unwrap();
    crate::preset::apply_worktree_sandbox_rules(&mut preset, &wt_writable, &wt_write_denies);
    let spec = SessionSpec {
        runtime: RuntimeId::try_from("codex").unwrap(),
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: AgentName::try_from("self-agent").unwrap(),
            description: "Engineer".into(),
            prompt: "Work".into(),
        },
        model: None,
        effort: None,
        workspace: ws_spec,
        read_only_dirs: vec![],
        instructions: "Work".into(),
        permission_mode: preset.permission_mode,
        policy: preset.policy,
        sandbox: preset.sandbox,
        mcp_servers: Default::default(),
        tools: None,
    };
    let ctx = LaunchContext {
        runtime_path: std::path::PathBuf::from("/usr/bin/true"),
        runtime_version: Some("0.172.0".into()),
        runtime_dir: SpecPath::try_from(rt_dir.path().canonicalize().unwrap().to_str().unwrap())
            .unwrap(),
        tmpdir: SpecPath::try_from(tmp_dir.path().canonicalize().unwrap().to_str().unwrap())
            .unwrap(),
        office_env_names: vec![],
        new_session_id: Some(
            org_agent_runtime::SessionId::try_from(new_uuid_v7().as_str()).unwrap(),
        ),
        resume: None,
        config_dir: None,
    };
    let (plan, _session) = org_agent_runtime_codex::CodexRuntime::new()
        .plan_session(&spec, &ctx)
        .expect("Codex must compile worktree sandbox grants cleanly");
    let self_root_canon = office_rt
        .join("teams")
        .join(&root_team_id)
        .join("agents")
        .join(&self_emp_id)
        .canonicalize()
        .unwrap();
    let fs_arg = plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.filesystem="))
        .expect("Codex profile filesystem arg present");
    let expected_write = format!("\"{}\"=\"write\"", self_root_canon.display());
    assert!(
        fs_arg.contains(&expected_write),
        "Codex must keep self worktree root writable (`{expected_write}`), got: {fs_arg}"
    );
}

#[test]
fn task126_warm_permit_turns_stale_on_execution_config_revision_change_and_401_reply_filter() {
    use org_proto::hiring::{AgentExecutionSet, AgentExecutionSetParams};

    assert!(crate::pump::is_claude_auth_expired_reply(
        r#"Failed to authenticate. API Error: 401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired."}}"#
    ));
    assert!(crate::pump::is_claude_auth_expired_reply(
        "401 OAuth access token has expired"
    ));
    assert!(!crate::pump::is_claude_auth_expired_reply(
        "I fixed the 401 OAuth access token has expired bug in pump.rs and ran cargo test."
    ));

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let conn_id = new_uuid_v7();
    let prof_id = new_uuid_v7();
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id.clone(),
            default_profile_id: prof_id.clone(),
            key: "claude-sub-test".into(),
            name: "Claude Sub Test".into(),
            provider_type: "anthropic".into(),
            auth_mode: "subscription_cli".into(),
            subscription_identity: Some("dev@example.com".into()),
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    let conn_resp = office
        .dispatch_hiring_rpc(Some(&owner_id), &conn_req)
        .unwrap();
    assert!(
        matches!(conn_resp.outcome, org_proto::Outcome::Result(_)),
        "provider connection create failed: {:?}",
        conn_resp.outcome
    );
    office
        .provider_binding_set(
            &owner_id,
            org_proto::providers::ProviderBindingSetParams {
                operation_id: new_uuid_v7(),
                connection_id: conn_id.clone(),
                expected_revision: None,
                auth_context_ref: "subscription://local".into(),
                runtime_binary_path: None,
                account_identity_ref: Some("dev@example.com".into()),
            },
        )
        .unwrap()
        .unwrap();
    office
        .provider_connection_refresh(
            &owner_id,
            org_proto::providers::ProviderConnectionRefreshParams {
                connection_id: conn_id.clone(),
                driver_version: Some("2.1.323".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: Some("dev@example.com".into()),
                operator_attested_subscription: true,
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
                name: "Warm Exec".into(),
                handle: "warm-exec".into(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner_id.clone()),
                office_id: None,
                model_profile_id: Some(prof_id.clone()),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("autonomous_sandboxed".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: Some("Prompt v1".into()),
                body: Some("Work.".into()),
            },
            reason: "Test warm exec".into(),
        },
    )
    .unwrap();
    let hired: org_proto::hiring::HireOpResult = match office
        .dispatch_hiring_rpc(Some(&owner_id), &hire_req)
        .unwrap()
        .outcome
    {
        org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
    };
    let agent_id = hired.employee_id.unwrap();

    let registry = crate::runtime_registry::RuntimeRegistry::production();
    let info = office.info();
    let vault = crate::credentials::CredentialVault::new(
        office.durable_root().to_path_buf(),
        info.org_id.clone(),
        info.office_id.clone(),
        None,
    );
    let excluded = std::collections::HashSet::new();
    let admitted = crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
        office: &office,
        vault: &vault,
        runtimes: &registry,
        hq_permits: None,
        employee_id: &agent_id,
        task_id: None,
        is_failover_switch: false,
        excluded_connections: &excluded,
    })
    .unwrap();
    let digest_v1 = admitted.route_digest_suffix.clone();
    if let Some((pid, _)) = admitted.permit {
        let _ = office.release_turn_permit(&pid);
    }

    match crate::router::acquire_warm_permit_for_office(
        &office,
        None,
        &conn_id,
        &agent_id,
        None,
        Some(&digest_v1),
    ) {
        crate::router::WarmPermitOutcome::Granted((pid, _)) => {
            let _ = office.release_turn_permit(&pid);
        }
        other => panic!("expected Granted for unchanged exec rev, got {other:?}"),
    }

    // Bump agent execution config instructions (revision 1 -> 2) and verify warm digest goes stale.
    let set_req = Request::new::<AgentExecutionSet>(
        3,
        &AgentExecutionSetParams {
            command_id: new_uuid_v7(),
            employee_id: agent_id.clone(),
            expected_version: None,
            model_profile_id: prof_id,
            model_id: "claude-sonnet-4-6".into(),
            tools_config: Vec::new(),
            permission_preset: Some("autonomous_sandboxed".into()),
            enabled: true,
            system_prompt_markdown: "Prompt v2".into(),
        },
    )
    .unwrap();
    assert!(matches!(
        office
            .dispatch_hiring_rpc(Some(&owner_id), &set_req)
            .unwrap()
            .outcome,
        org_proto::Outcome::Result(_)
    ));
    assert!(
        matches!(
            crate::router::acquire_warm_permit_for_office(
                &office,
                None,
                &conn_id,
                &agent_id,
                None,
                Some(&digest_v1),
            ),
            crate::router::WarmPermitOutcome::StaleWarm
        ),
        "agent execution-set revision bump must invalidate warm session route digest"
    );

    // Verify worktree sandbox fingerprint invalidates warm session when a new worktree is prepared.
    let admitted_v2 = crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
        office: &office,
        vault: &vault,
        runtimes: &registry,
        hq_permits: None,
        employee_id: &agent_id,
        task_id: None,
        is_failover_switch: false,
        excluded_connections: &excluded,
    })
    .unwrap();
    let digest_v2 = admitted_v2.route_digest_suffix.clone();
    if let Some((pid, _)) = admitted_v2.permit {
        let _ = office.release_turn_permit(&pid);
    }
    let mut gs = org_office::git::OfficeGitState::default();
    gs.workspaces.insert(
        org_office::git::OfficeGitState::workspace_key("t-new", "repo-new", &agent_id),
        org_office::git::TaskWorkspaceRecord {
            office_id: info.office_id.clone(),
            employee_id: agent_id.clone(),
            task_id: "t-new".into(),
            task_key: "T-NEW".into(),
            repository_id: "repo-new".into(),
            repository_key: Some("org".into()),
            branch: "task/T-NEW".into(),
            worktree_path: "/tmp/wt-new".into(),
            admin_dir_name: "admin-new".into(),
            base_commit: "abc".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    gs.save(office.runtime_root(), &info.office_id).unwrap();
    assert!(
        matches!(
            crate::router::acquire_warm_permit_for_office(
                &office,
                None,
                &conn_id,
                &agent_id,
                None,
                Some(&digest_v2),
            ),
            crate::router::WarmPermitOutcome::StaleWarm
        ),
        "newly prepared task worktree must invalidate warm session route digest"
    );

    // Corrupt `git-state.json` at BOTH admission and warm permit acquisition MUST fail closed into
    // `StaleWarm` (never matching `"err" == "err"` across turns).
    let git_state_path = office
        .runtime_root()
        .join(&info.office_id)
        .join("git-state.json");
    std::fs::write(&git_state_path, "{corrupt").unwrap();
    let admitted_corrupt =
        crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
            office: &office,
            vault: &vault,
            runtimes: &registry,
            hq_permits: None,
            employee_id: &agent_id,
            task_id: None,
            is_failover_switch: false,
            excluded_connections: &excluded,
        })
        .unwrap();
    let digest_corrupt = admitted_corrupt.route_digest_suffix.clone();
    if let Some((pid, _)) = admitted_corrupt.permit {
        let _ = office.release_turn_permit(&pid);
    }
    assert!(
        matches!(
            crate::router::acquire_warm_permit_for_office(
                &office,
                None,
                &conn_id,
                &agent_id,
                None,
                Some(&digest_corrupt),
            ),
            crate::router::WarmPermitOutcome::StaleWarm
        ),
        "corrupt git-state.json must never match stored err digest on warm permit acquisition"
    );

    // Verify `claude-code` runtime_driver is eligible for host subscription OAuth refresh.
    let mut mgr_cfg = SessionManagerConfig::production(office.durable_root().join("sock"));
    let fake_bin = tempfile::tempdir().unwrap();
    let fake_claude = fake_bin.path().join("claude");
    std::fs::write(&fake_claude, "#!/bin/sh\nexit 0\n").unwrap();
    mgr_cfg.runtimes.register(
        Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()),
        Some(fake_claude.clone()),
    );
    let mgr = SessionManager::new(Arc::clone(&office), mgr_cfg);
    assert_eq!(
        crate::manager::is_host_claude_subscription_refresh_eligible(&mgr, &conn_id),
        Some(fake_claude.canonicalize().unwrap()),
        "claude-code subscription_cli connection must be eligible for host OAuth refresh"
    );

    // Verify classify_turn_failover_reason covers SignedOut, Failed reason, non-claude exclusion, and length cap.
    assert_eq!(
        crate::pump::classify_turn_failover_reason(
            "claude",
            &org_agent_runtime::TurnOutcome::SignedOut,
            &[],
            None,
            false,
            false,
        ),
        Some("auth_expired")
    );
    assert_eq!(
        crate::pump::classify_turn_failover_reason(
            "claude",
            &org_agent_runtime::TurnOutcome::Failed {
                reason: "401 unauthorized".into(),
            },
            &[],
            None,
            false,
            false,
        ),
        Some("auth_expired")
    );
    assert_eq!(
        crate::pump::classify_turn_failover_reason(
            "codex",
            &org_agent_runtime::TurnOutcome::Completed,
            &[],
            Some("401 OAuth access token has expired"),
            false,
            false,
        ),
        None
    );
    let long_prose = format!("401 OAuth access token has expired {}", "x".repeat(1200));
    assert!(!crate::pump::is_claude_auth_expired_reply(&long_prose));
}

#[test]
fn task126_nested_team_hierarchy_and_readonly_preset_enforce_strict_isolation() {
    use org_agent_runtime::{
        AgentIdentity, AgentName, AgentRuntime as _, LaunchContext, RuntimeId, SessionKind,
        SessionSpec, SpecPath,
    };

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let parent_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    // Create child team T2 under parent team T1.
    let child_team_id = new_uuid_v7();
    office
        .submit_org(
            &owner_id,
            org_office::rules::OrgCommand::TeamCreate(org_proto::org::TeamCreateParams {
                command_id: new_uuid_v7(),
                team_id: child_team_id.clone(),
                name: "Sub Team".into(),
                owner_team_id: Some(parent_team_id.clone()),
            }),
        )
        .unwrap()
        .unwrap();

    let hire_in = |team_id: &str, name: &str, handle: &str| -> String {
        let req = Request::new::<HrHire>(
            2,
            &HrHireParams {
                command_id: new_uuid_v7(),
                request_id: new_uuid_v7(),
                employee_id: new_uuid_v7(),
                home_team_id: team_id.to_owned(),
                role_id: None,
                proposal: HireProposalWire {
                    name: name.into(),
                    handle: handle.into(),
                    title: Some("Engineer".into()),
                    focus: None,
                    role_text: None,
                    title_in_team: Some("Engineer".into()),
                    reports_to_employee_id: Some(owner_id.clone()),
                    office_id: None,
                    model_profile_id: None,
                    model_id: None,
                    permission_preset: Some("read_only".into()),
                    tools_config: Vec::new(),
                    system_prompt_markdown: None,
                    body: Some("Work.".into()),
                },
                reason: "Test nested hire".into(),
            },
        )
        .unwrap();
        let resp = office.dispatch_hiring_rpc(Some(&owner_id), &req).unwrap();
        let hired: org_proto::hiring::HireOpResult = match resp.outcome {
            org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
        };
        hired.employee_id.unwrap()
    };
    let parent_agent = hire_in(&parent_team_id, "Parent Agent", "parent-agent");
    let child_agent = hire_in(&child_team_id, "Child Agent", "child-agent");

    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    );
    // Child agent in T2 MUST have ancestor T1's `agents/**` denied.
    let child_denied: Vec<String> = mgr
        .denied_office_globs_for(Some(&child_agent))
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    assert!(
        child_denied
            .iter()
            .any(|s| s.ends_with(&format!("/teams/{parent_team_id}/agents/**"))),
        "child-team agent must have ancestor team agents/** denied: {child_denied:?}"
    );
    // Parent agent in T1 MUST have child subteams `teams/T1/teams/**` denied.
    let parent_denied: Vec<String> = mgr
        .denied_office_globs_for(Some(&parent_agent))
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    assert!(
        parent_denied
            .iter()
            .any(|s| s.ends_with(&format!("/teams/{parent_team_id}/teams/**"))),
        "parent-team agent must have descendant teams/** denied: {parent_denied:?}"
    );

    // Verify ReadOnly preset with apply_worktree_sandbox_rules never adds writable_roots for Codex or Claude.
    let (wt_writable, wt_write_denies) = mgr.worktree_sandbox_grants(&child_agent, false);
    assert!(!wt_writable.is_empty());
    let ws_dir = tempfile::tempdir().unwrap();
    let rt_dir = tempfile::tempdir().unwrap();
    let tmp_dir = tempfile::tempdir().unwrap();
    let ws_spec =
        SpecPath::try_from(ws_dir.path().canonicalize().unwrap().to_str().unwrap()).unwrap();
    let mut ro = crate::preset::build_session_policy(
        PermissionPreset::ReadOnly,
        true,
        &ws_spec,
        None,
        &mgr.denied_office_globs_for(Some(&child_agent)),
        1,
    )
    .unwrap();
    crate::preset::apply_worktree_sandbox_rules(&mut ro, &wt_writable, &wt_write_denies);
    assert!(
        ro.sandbox.writable_roots.is_empty(),
        "ReadOnly preset must never gain worktree writable_roots: {:?}",
        ro.sandbox.writable_roots
    );
    let ctx = LaunchContext {
        runtime_path: std::path::PathBuf::from("/usr/bin/true"),
        runtime_version: Some("1.0.0".into()),
        runtime_dir: SpecPath::try_from(rt_dir.path().canonicalize().unwrap().to_str().unwrap())
            .unwrap(),
        tmpdir: SpecPath::try_from(tmp_dir.path().canonicalize().unwrap().to_str().unwrap())
            .unwrap(),
        office_env_names: vec![],
        new_session_id: Some(
            org_agent_runtime::SessionId::try_from(new_uuid_v7().as_str()).unwrap(),
        ),
        resume: None,
        config_dir: None,
    };
    let make_spec = |rt: &str| SessionSpec {
        runtime: RuntimeId::try_from(rt).unwrap(),
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: AgentName::try_from("child-agent").unwrap(),
            description: "Read-only".into(),
            prompt: "Inspect".into(),
        },
        model: None,
        effort: None,
        workspace: ws_spec.clone(),
        read_only_dirs: vec![],
        instructions: "Inspect".into(),
        permission_mode: ro.permission_mode,
        policy: ro.policy.clone(),
        sandbox: ro.sandbox.clone(),
        mcp_servers: Default::default(),
        tools: ro.allowed_tools.clone(),
    };
    org_agent_runtime_codex::CodexRuntime::new()
        .plan_session(&make_spec("codex"), &ctx)
        .expect("Codex compiles ReadOnly + worktree denies");
    org_agent_runtime_claude::ClaudeRuntime::new()
        .plan_session(&make_spec("claude"), &ctx)
        .expect("Claude compiles ReadOnly + worktree denies");
}

#[test]
fn task126_zero_worktree_corrupt_git_state_and_digest_invalidation() {
    use org_office::git::{OfficeGitState, TaskWorkspaceRecord};

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let hire_agent = |name: &str, handle: &str| -> String {
        let req = Request::new::<HrHire>(
            1,
            &HrHireParams {
                command_id: new_uuid_v7(),
                request_id: new_uuid_v7(),
                employee_id: new_uuid_v7(),
                home_team_id: root_team_id.clone(),
                role_id: None,
                proposal: HireProposalWire {
                    name: name.into(),
                    handle: handle.into(),
                    title: Some("Engineer".into()),
                    focus: None,
                    role_text: None,
                    title_in_team: Some("Engineer".into()),
                    reports_to_employee_id: Some(owner_id.clone()),
                    office_id: None,
                    model_profile_id: None,
                    model_id: None,
                    permission_preset: Some("autonomous_sandboxed".into()),
                    tools_config: Vec::new(),
                    system_prompt_markdown: None,
                    body: Some("Work.".into()),
                },
                reason: "Test hire".into(),
            },
        )
        .unwrap();
        let resp = office.dispatch_hiring_rpc(Some(&owner_id), &req).unwrap();
        match resp.outcome {
            org_proto::Outcome::Result(v) => {
                serde_json::from_value::<org_proto::hiring::HireOpResult>(v)
                    .unwrap()
                    .employee_id
                    .unwrap()
            }
            org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
        }
    };
    let emp_a = hire_agent("Agent A", "agent-a");
    let emp_b = hire_agent("Agent B", "agent-b");

    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    );
    let office_id = office.info().office_id.clone();
    let office_rt = office.runtime_root().join(&office_id);

    // 1. Zero active worktrees: MUST blanket-deny `<office_rt>/repositories/**` and grant zero
    // bare-cache writable roots so newly cloned repos later cannot be read.
    let denied_zero: Vec<String> = mgr
        .denied_office_globs_for(Some(&emp_a))
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    assert!(
        denied_zero
            .iter()
            .any(|s| s.ends_with(&format!("/{office_id}/repositories/**"))),
        "zero-worktree employee must get blanket repositories/** deny: {denied_zero:?}"
    );
    let (wt_zero_writable, _) = mgr.worktree_sandbox_grants(&emp_a, false);
    assert!(
        !wt_zero_writable
            .iter()
            .any(|s| s.as_str().contains("/repositories/")),
        "zero-worktree employee must get zero bare-cache writable grants: {wt_zero_writable:?}"
    );

    // 2. Active worktree on repo R1: verify worktree digest changes whenever a new untargeted repo
    // R2 is cloned OR a peer agent creates a worktree admin dir on R1.
    let repo1 = "01a212a8-982e-71ca-a6de-322dba762d01";
    let repo2 = "01a212a8-982e-71ca-a6de-322dba762d02";
    let r1_cache = office_rt.join("repositories").join(format!("{repo1}.git"));
    let self_wt = office_rt
        .join("teams")
        .join(&root_team_id)
        .join("agents")
        .join(&emp_a)
        .join("tasks/t-1/repositories")
        .join(repo1);
    std::fs::create_dir_all(r1_cache.join("worktrees/admin-a")).unwrap();
    std::fs::create_dir_all(&self_wt).unwrap();
    let mut gs = OfficeGitState::default();
    gs.workspaces.insert(
        OfficeGitState::workspace_key("t-1", repo1, &emp_a),
        TaskWorkspaceRecord {
            office_id: office_id.clone(),
            employee_id: emp_a.clone(),
            task_id: "t-1".into(),
            task_key: "T-1".into(),
            repository_id: repo1.into(),
            repository_key: Some("r1".into()),
            branch: "task/T-1".into(),
            worktree_path: self_wt.display().to_string(),
            admin_dir_name: "admin-a".into(),
            base_commit: "abc".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    gs.save(office.runtime_root(), &office_id).unwrap();
    let d1 = crate::router::compute_worktree_sandbox_digest(&office, &emp_a);

    // Clone untargeted repo R2 on disk -> digest MUST change.
    std::fs::create_dir_all(office_rt.join("repositories").join(format!("{repo2}.git"))).unwrap();
    let d2 = crate::router::compute_worktree_sandbox_digest(&office, &emp_a);
    assert_ne!(
        d1, d2,
        "newly cloned untargeted repository must invalidate warm worktree digest"
    );

    // Peer agent prepares worktree on R1 -> digest MUST change again.
    std::fs::create_dir_all(r1_cache.join("worktrees/admin-peer")).unwrap();
    gs.workspaces.insert(
        OfficeGitState::workspace_key("t-2", repo1, &emp_b),
        TaskWorkspaceRecord {
            office_id: office_id.clone(),
            employee_id: emp_b.clone(),
            task_id: "t-2".into(),
            task_key: "T-2".into(),
            repository_id: repo1.into(),
            repository_key: Some("r1".into()),
            branch: "task/T-2".into(),
            worktree_path: office_rt.join("peer").display().to_string(),
            admin_dir_name: "admin-peer".into(),
            base_commit: "abc".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    gs.save(office.runtime_root(), &office_id).unwrap();
    let d3 = crate::router::compute_worktree_sandbox_digest(&office, &emp_a);
    assert_ne!(
        d2, d3,
        "peer worktree added on targeted repo must invalidate warm worktree digest"
    );

    // 3. Corrupt `git-state.json`: MUST fail closed with blanket `repositories/**` deny,
    // zero bare-cache writable roots, and `"err"` digest.
    std::fs::write(office_rt.join("git-state.json"), "{corrupt-json").unwrap();
    let denied_corrupt: Vec<String> = mgr
        .denied_office_globs_for(Some(&emp_a))
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    assert!(
        denied_corrupt
            .iter()
            .any(|s| s.ends_with(&format!("/{office_id}/repositories/**"))),
        "corrupt git-state.json must fail closed with blanket repositories/** deny: {denied_corrupt:?}"
    );
    let (wt_corrupt_writable, _) = mgr.worktree_sandbox_grants(&emp_a, false);
    assert!(
        !wt_corrupt_writable
            .iter()
            .any(|s| s.as_str().contains("/repositories/")),
        "corrupt git-state.json must grant zero bare-cache writable roots: {wt_corrupt_writable:?}"
    );
    assert_eq!(
        crate::router::compute_worktree_sandbox_digest(&office, &emp_a),
        "err"
    );
}

#[test]
fn task126_recycle_stale_warm_handle_atomicity_and_stale_failover_non_drop() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = Arc::new(SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    ));
    SessionManager::attach_self(&mgr);
    let emp_id = "01a212a8-982e-7000-8000-000000000126";

    let (tx, rx) = std::sync::mpsc::channel::<crate::pump::PumpCommand>();
    let stopped_flag = Arc::new(AtomicBool::new(false));
    let stopped_clone = Arc::clone(&stopped_flag);
    let worker = std::thread::spawn(move || {
        while let Ok(cmd) = rx.recv() {
            if matches!(cmd, crate::pump::PumpCommand::Stop { .. }) {
                stopped_clone.store(true, Ordering::SeqCst);
                break;
            }
        }
    });
    let state = Arc::new(Mutex::new(crate::pump::SessionRuntimeState {
        employee_id: emp_id.into(),
        office_id: office.info().office_id.clone(),
        state: SessionState::Idle,
        run_id: Some("run-old".into()),
        session_id: Some("sess-old".into()),
        runtime_id: Some("claude".into()),
        runtime_session_id: None,
        private_home: "/tmp/home".into(),
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
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
    }));
    let permit = Arc::new(Mutex::new(None));
    {
        let mut reg = mgr.registry.lock().unwrap();
        reg.live.insert(
            emp_id.into(),
            crate::manager::LiveSessionHandle {
                state: Arc::clone(&state),
                tx,
                thread: Some(worker),
                connection_id: Some("conn-1".into()),
                route_digest: "old".into(),
                oauth_expires_at_ms: None,
                workspace: None,
                active_permit: Arc::clone(&permit),
            },
        );
    }

    // 1. Refuse while `reg.starting` already holds employee_id (concurrent start_session).
    {
        mgr.registry.lock().unwrap().starting.insert(emp_id.into());
    }
    assert!(!crate::lifecycle::recycle_stale_warm_handle_in(
        &mgr,
        emp_id,
        "run-old",
        "warm_route_stale"
    ));
    assert!(mgr.registry.lock().unwrap().live.contains_key(emp_id));
    {
        mgr.registry.lock().unwrap().starting.remove(emp_id);
    }

    // 2. Refuse while `reg.draining` is true (shutdown in progress).
    {
        mgr.registry.lock().unwrap().draining = true;
    }
    assert!(!crate::lifecycle::recycle_stale_warm_handle_in(
        &mgr,
        emp_id,
        "run-old",
        "warm_route_stale"
    ));
    assert!(mgr.registry.lock().unwrap().live.contains_key(emp_id));
    {
        mgr.registry.lock().unwrap().draining = false;
    }

    // 3. Refuse while handle is Running or holds an active permit.
    {
        state.lock().unwrap().state = SessionState::Running;
    }
    assert!(!crate::lifecycle::recycle_stale_warm_handle_in(
        &mgr,
        emp_id,
        "run-old",
        "warm_route_stale"
    ));
    {
        state.lock().unwrap().state = SessionState::Idle;
        *permit.lock().unwrap() = Some(("perm-1".into(), 1));
    }
    assert!(!crate::lifecycle::recycle_stale_warm_handle_in(
        &mgr,
        emp_id,
        "run-old",
        "warm_route_stale"
    ));
    {
        *permit.lock().unwrap() = None;
    }

    // 4. Stale-recycle failover when a concurrent start holds `reg.starting` MUST persist
    // `req.objective` into durable `pending_wakes` instead of silently dropping it.
    {
        mgr.registry.lock().unwrap().starting.insert(emp_id.into());
    }
    crate::manager::run_failover_relaunch(
        &mgr,
        crate::pump::FailoverRequest {
            employee_id: emp_id.into(),
            failed_run_id: "run-old".into(),
            failed_connection_id: "conn-1".into(),
            objective: "do-not-drop-stale-turn".into(),
            workspace: None,
            task_ref: None,
            tried_connections: Vec::new(),
            oauth_refresh_attempted: false,
            oauth_token_fp: None,
            reason: "warm_route_stale",
            rate_limit_resets_at: None,
        },
    );
    {
        mgr.registry.lock().unwrap().starting.remove(emp_id);
    }
    let wakes = mgr
        .store
        .read_pending_wakes(emp_id)
        .unwrap()
        .expect("stale recycle contention must persist durable recovery wake");
    assert!(
        wakes
            .items
            .iter()
            .any(|i| i.summary == "do-not-drop-stale-turn"),
        "queued stale objective must be preserved: {:?}",
        wakes.items
    );

    // 5. Idle + permit-free recycle succeeds atomically; stop_all waits until bg worker finishes.
    assert!(crate::lifecycle::recycle_stale_warm_handle_in(
        &mgr,
        emp_id,
        "run-old",
        "warm_route_stale"
    ));
    mgr.stop_all("test_shutdown");
    assert!(
        stopped_flag.load(Ordering::SeqCst),
        "recycled handle must receive Stop and be joined before stop_all returns"
    );
    let reg = mgr.registry.lock().unwrap();
    assert!(reg.live.is_empty());
    assert!(reg.starting.is_empty());
    assert_eq!(reg.bg_spawning, 0);
}

#[test]
fn task126_host_rust_dir_env_rejects_office_roots_and_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let durable_root = dir.path().join("durable");
    let runtime_root = dir.path().join("runtime");
    let external_cargo = dir.path().join("external-cargo");
    let inside_durable = durable_root.join("cargo");
    let inside_runtime = runtime_root.join("cargo");
    let symlink_escape = dir.path().join("symlink-to-durable");
    std::fs::create_dir_all(&external_cargo).unwrap();
    std::fs::create_dir_all(&inside_durable).unwrap();
    std::fs::create_dir_all(&inside_runtime).unwrap();
    std::os::unix::fs::symlink(&inside_durable, &symlink_escape).unwrap();

    let forbidden = vec![durable_root.clone(), runtime_root.clone()];
    let ok = crate::manager::host_rust_dir_env_from(
        Some(external_cargo.as_os_str().to_os_string()),
        None,
        ".cargo",
        &forbidden,
    );
    assert_eq!(
        ok.map(std::path::PathBuf::from),
        Some(external_cargo.canonicalize().unwrap())
    );
    assert!(
        crate::manager::host_rust_dir_env_from(
            Some(inside_durable.as_os_str().to_os_string()),
            None,
            ".cargo",
            &forbidden,
        )
        .is_none(),
        "must reject directory inside durable_root"
    );
    assert!(
        crate::manager::host_rust_dir_env_from(
            Some(inside_runtime.as_os_str().to_os_string()),
            None,
            ".cargo",
            &forbidden,
        )
        .is_none(),
        "must reject directory inside runtime_root"
    );
    assert!(
        crate::manager::host_rust_dir_env_from(
            Some(symlink_escape.as_os_str().to_os_string()),
            None,
            ".cargo",
            &forbidden,
        )
        .is_none(),
        "must reject symlink escaping into durable_root"
    );
}

#[test]
fn task126_org_topology_invalidates_warm_digest_and_expiring_oauth_triggers_stale_permit() {
    use org_office::git::{OfficeGitState, TaskWorkspaceRecord};

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let hire_in = |team_id: &str, name: &str, handle: &str| -> String {
        let req = Request::new::<HrHire>(
            1,
            &HrHireParams {
                command_id: new_uuid_v7(),
                request_id: new_uuid_v7(),
                employee_id: new_uuid_v7(),
                home_team_id: team_id.to_owned(),
                role_id: None,
                proposal: HireProposalWire {
                    name: name.into(),
                    handle: handle.into(),
                    title: Some("Engineer".into()),
                    focus: None,
                    role_text: None,
                    title_in_team: Some("Engineer".into()),
                    reports_to_employee_id: Some(owner_id.clone()),
                    office_id: None,
                    model_profile_id: None,
                    model_id: None,
                    permission_preset: Some("autonomous_sandboxed".into()),
                    tools_config: Vec::new(),
                    system_prompt_markdown: None,
                    body: Some("Work.".into()),
                },
                reason: "Topology test hire".into(),
            },
        )
        .unwrap();
        let resp = office.dispatch_hiring_rpc(Some(&owner_id), &req).unwrap();
        match resp.outcome {
            org_proto::Outcome::Result(v) => {
                serde_json::from_value::<org_proto::hiring::HireOpResult>(v)
                    .unwrap()
                    .employee_id
                    .unwrap()
            }
            org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
        }
    };
    let emp_a = hire_in(&root_team_id, "Warm A", "warm-a");

    // 1. Zero-worktree state: hiring a sibling agent OR creating a new team MUST invalidate digest.
    let d_zero_0 = crate::router::compute_worktree_sandbox_digest(&office, &emp_a);
    let _emp_b = hire_in(&root_team_id, "Warm B", "warm-b");
    let d_zero_1 = crate::router::compute_worktree_sandbox_digest(&office, &emp_a);
    assert_ne!(
        d_zero_0, d_zero_1,
        "hiring sibling agent must invalidate zero-worktree warm digest"
    );
    let sub_team = new_uuid_v7();
    office
        .submit_org(
            &owner_id,
            org_office::rules::OrgCommand::TeamCreate(org_proto::org::TeamCreateParams {
                command_id: new_uuid_v7(),
                team_id: sub_team.clone(),
                name: "New Team".into(),
                owner_team_id: Some(root_team_id.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    let d_zero_2 = crate::router::compute_worktree_sandbox_digest(&office, &emp_a);
    assert_ne!(
        d_zero_1, d_zero_2,
        "creating a team must invalidate zero-worktree warm digest"
    );

    // 2. Active-worktree state with pre-existing R1 and R2: hiring emp_c MUST still invalidate
    // emp_a's digest even when R1/R2 repo list and R1 peer admins do not change.
    let office_id = office.info().office_id.clone();
    let office_rt = office.runtime_root().join(&office_id);
    let repo1 = "01a212a8-982e-71ca-a6de-322dba762e01";
    let repo2 = "01a212a8-982e-71ca-a6de-322dba762e02";
    std::fs::create_dir_all(
        office_rt
            .join("repositories")
            .join(format!("{repo1}.git/worktrees/a1")),
    )
    .unwrap();
    std::fs::create_dir_all(office_rt.join("repositories").join(format!("{repo2}.git"))).unwrap();
    let mut gs = OfficeGitState::default();
    gs.workspaces.insert(
        OfficeGitState::workspace_key("t-1", repo1, &emp_a),
        TaskWorkspaceRecord {
            office_id: office_id.clone(),
            employee_id: emp_a.clone(),
            task_id: "t-1".into(),
            task_key: "T-1".into(),
            repository_id: repo1.into(),
            repository_key: Some("r1".into()),
            branch: "task/T-1".into(),
            worktree_path: office_rt.join("wt-a1").display().to_string(),
            admin_dir_name: "a1".into(),
            base_commit: "abc".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    gs.save(office.runtime_root(), &office_id).unwrap();
    let d_act_0 = crate::router::compute_worktree_sandbox_digest(&office, &emp_a);
    let _emp_c = hire_in(&sub_team, "Warm C", "warm-c");
    let d_act_1 = crate::router::compute_worktree_sandbox_digest(&office, &emp_a);
    assert_ne!(
        d_act_0, d_act_1,
        "hiring sibling agent must invalidate active-worktree warm digest even when repos do not change"
    );

    // 3. `build_pump_hooks` with an expiring OAuth timestamp must return `WARM_ROUTE_STALE` from
    // `on_permit_acquire` so queued in-pump turns recycle cleanly instead of using an expiring token.
    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    );
    let hooks = mgr.build_pump_hooks(&emp_a, "digest".into(), Some(1));
    let acquire = hooks.on_permit_acquire.expect("permit acquire hook");
    let err = acquire("conn-1", None).expect_err("expiring OAuth must fail warm permit acquire");
    assert!(
        err.starts_with("WARM_ROUTE_STALE"),
        "expected WARM_ROUTE_STALE on expiring OAuth, got: {err}"
    );
}
