//! Standalone unit tests for `org-session-presets`.

use org_agent_runtime::{
    ApprovalId, ApprovalKind, ApprovalRequest, Escalation, PathPattern, PermissionMode, SpecPath,
    ToolCapability,
};
use org_office::organization_family::records::PermissionPreset;
use serde_json::json;

use crate::{
    build_grants_summary, build_session_policy, decide_escalation_with_roots, parse_preset,
    EscalationDisposition,
};

#[test]
fn presets_refuse_bypass_and_enforce_sandbox_and_denies() {
    assert!(parse_preset("bypassPermissions").is_err());
    assert_eq!(parse_preset("safe_ask").unwrap(), PermissionPreset::SafeAsk);

    let ws = SpecPath::try_from("/tmp/preset-ws").unwrap();
    let office_glob = PathPattern::try_from("/tmp/office/**").unwrap();
    let cfg = build_session_policy(
        PermissionPreset::WorkspaceEdits,
        true,
        &ws,
        None,
        std::slice::from_ref(&office_glob),
        1,
    )
    .unwrap();
    assert_eq!(cfg.permission_mode, PermissionMode::AcceptEdits);
    assert!(cfg.sandbox.enabled);

    let mut bypass_req = ApprovalRequest::new(
        ApprovalId::new("req-1"),
        ApprovalKind::Command,
        Escalation::SandboxBypass,
        "Bash",
        json!({ "command": "ls" }),
    );
    bypass_req.capability = Some(ToolCapability::Shell);
    assert!(matches!(
        decide_escalation_with_roots(&bypass_req, &cfg.policy, cfg.preset, None, None),
        EscalationDisposition::AutoDeny { .. }
    ));

    let summary = build_grants_summary("emp-1", &cfg, &[]);
    assert_eq!(summary.permission_preset, "workspace_edits");
    assert!(summary.sandbox_enabled);
}
