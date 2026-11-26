//! Permission presets, escalation policy, and multi-runtime capability mapping (`ENROLL-7`).

use super::*;

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
