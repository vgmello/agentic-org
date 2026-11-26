//! Approval bridge: opaque rename targets, read-only presets, and Pass 3 deny resolution.

use super::*;

#[test]
fn task126_approval_bridge_covers_opaque_rename_targets_and_safe_ask_readonly_presets() {
    let dir = tempfile::tempdir().unwrap();
    let canon_dir = dir.path().canonicalize().unwrap();
    let ws = canon_dir.join("workspace");
    let wt = canon_dir.join("worktree");
    let tmp = canon_dir.join("agent-tmp");
    let cargo_home = canon_dir.join("host-cargo");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::create_dir_all(&cargo_home).unwrap();

    let ws_spec = SpecPath::try_from(ws.to_str().unwrap()).unwrap();
    let wt_spec = SpecPath::try_from(wt.to_str().unwrap()).unwrap();
    let mut cfg = crate::preset::build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &ws_spec,
        None,
        &[],
        1,
    )
    .unwrap();
    crate::preset::apply_worktree_sandbox_rules(&mut cfg, &[wt_spec], &[]);
    let edit_raw = |raw: &str| {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("req-preset"),
            ApprovalKind::FileChange,
            Escalation::None,
            "Write",
            serde_json::json!({ "file_path": raw, "content": "x" }),
        );
        req.capability = Some(org_agent_runtime::ToolCapability::Edit);
        req
    };

    // Finding 3: notebook_path (real Claude `capability: None`), rename move_path / kind.move_path,
    // and mixed malformed / dual-key changes.
    let nb_outside = ApprovalRequest::new(
        ApprovalId::new("nb-1"),
        ApprovalKind::FileChange,
        Escalation::None,
        "NotebookEdit",
        serde_json::json!({ "notebook_path": cargo_home.join("nb.ipynb").to_str().unwrap() }),
    );
    assert!(matches!(
        crate::preset::decide_escalation_with_roots(
            &nb_outside,
            &cfg.policy,
            cfg.preset,
            cfg.allowed_tools.as_deref(),
            Some(cfg.sandbox.writable_roots.as_slice()),
        ),
        crate::preset::EscalationDisposition::AutoDeny { .. }
    ));

    let mut rename_map = ApprovalRequest::new(
        ApprovalId::new("mv-1"),
        ApprovalKind::FileChange,
        Escalation::None,
        "ApplyPatch",
        serde_json::json!({
            "fileChanges": {
                wt.join("src.rs").to_str().unwrap(): {
                    "kind": { "type": "update", "move_path": cargo_home.join("config.toml").to_str().unwrap() }
                }
            }
        }),
    );
    rename_map.capability = Some(org_agent_runtime::ToolCapability::Edit);
    assert!(matches!(
        crate::preset::decide_escalation_with_roots(
            &rename_map,
            &cfg.policy,
            cfg.preset,
            cfg.allowed_tools.as_deref(),
            Some(cfg.sandbox.writable_roots.as_slice()),
        ),
        crate::preset::EscalationDisposition::AutoDeny { .. }
    ));

    let mut mixed_opaque = ApprovalRequest::new(
        ApprovalId::new("op-1"),
        ApprovalKind::FileChange,
        Escalation::None,
        "ApplyPatch",
        serde_json::json!({
            "file_path": wt.join("ok.rs").to_str().unwrap(),
            "changes": [{ "unexpected_field": 123 }]
        }),
    );
    mixed_opaque.capability = Some(org_agent_runtime::ToolCapability::Edit);
    assert!(matches!(
        crate::preset::decide_escalation_with_roots(
            &mixed_opaque,
            &cfg.policy,
            cfg.preset,
            cfg.allowed_tools.as_deref(),
            Some(cfg.sandbox.writable_roots.as_slice()),
        ),
        crate::preset::EscalationDisposition::AutoDeny { .. }
    ));

    // Finding 4: SafeAsk escalates inside root & auto-denies outside; ReadOnly omits tmpdir.
    let safe_cfg = crate::preset::build_session_policy(
        PermissionPreset::SafeAsk,
        true,
        &ws_spec,
        None,
        &[],
        1,
    )
    .unwrap();
    let safe_roots = crate::manager::effective_approval_writable_roots_from(
        PermissionPreset::SafeAsk,
        &safe_cfg.sandbox.writable_roots,
        &tmp,
    );
    assert!(
        safe_roots
            .iter()
            .any(|p| p.as_str() == tmp.to_str().unwrap()),
        "non-ReadOnly preset must include canonical tmpdir in approval writable roots"
    );
    assert!(matches!(
        crate::preset::decide_escalation_with_roots(
            &edit_raw(&ws.join("ok.rs").display().to_string()),
            &safe_cfg.policy,
            safe_cfg.preset,
            safe_cfg.allowed_tools.as_deref(),
            Some(safe_roots.as_slice()),
        ),
        crate::preset::EscalationDisposition::EscalateToOwner { .. }
    ));
    assert!(matches!(
        crate::preset::decide_escalation_with_roots(
            &edit_raw(&cargo_home.join("config.toml").display().to_string()),
            &safe_cfg.policy,
            safe_cfg.preset,
            safe_cfg.allowed_tools.as_deref(),
            Some(safe_roots.as_slice()),
        ),
        crate::preset::EscalationDisposition::AutoDeny { .. }
    ));

    let ro_cfg = crate::preset::build_session_policy(
        PermissionPreset::ReadOnly,
        true,
        &ws_spec,
        None,
        &[],
        1,
    )
    .unwrap();
    let ro_roots = crate::manager::effective_approval_writable_roots_from(
        PermissionPreset::ReadOnly,
        &ro_cfg.sandbox.writable_roots,
        &tmp,
    );
    assert!(
        !ro_roots.iter().any(|p| p.as_str() == tmp.to_str().unwrap()),
        "ReadOnly preset must NOT add tmpdir to approval writable roots"
    );
    assert!(matches!(
        crate::preset::decide_escalation_with_roots(
            &edit_raw(&tmp.join("x").display().to_string()),
            &ro_cfg.policy,
            ro_cfg.preset,
            ro_cfg.allowed_tools.as_deref(),
            Some(ro_roots.as_slice()),
        ),
        crate::preset::EscalationDisposition::AutoDeny { .. }
    ));
}

#[test]
fn task126_pass3_notebook_edit_unsandboxed_deny_resolver_macos_case_and_dual_key_changes() {
    let dir = tempfile::tempdir().unwrap();
    let canon_dir = dir.path().canonicalize().unwrap();
    let ws = canon_dir.join("workspace");
    let wt = canon_dir.join("worktree");
    let admin_dir = canon_dir.join("cache.git/worktrees/self-admin");
    let cargo_home = canon_dir.join("host-cargo");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::create_dir_all(&admin_dir).unwrap();
    std::fs::create_dir_all(&cargo_home).unwrap();
    std::fs::write(
        wt.join(".git"),
        "gitdir: ../cache.git/worktrees/self-admin\n",
    )
    .unwrap();

    let ws_spec = SpecPath::try_from(ws.to_str().unwrap()).unwrap();
    let wt_spec = SpecPath::try_from(wt.to_str().unwrap()).unwrap();
    let admin_spec = SpecPath::try_from(admin_dir.to_str().unwrap()).unwrap();
    let mut cfg = crate::preset::build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &ws_spec,
        None,
        &[],
        1,
    )
    .unwrap();
    let write_denies = vec![
        org_agent_runtime::PathPattern::try_from(
            admin_dir.join("config.worktree").to_str().unwrap(),
        )
        .unwrap(),
        org_agent_runtime::PathPattern::try_from(wt.join(".git").to_str().unwrap()).unwrap(),
    ];
    crate::preset::apply_worktree_sandbox_rules(&mut cfg, &[wt_spec, admin_spec], &write_denies);
    // Simulate an owner previously granting `AllowAlways` on `NotebookEdit`.
    cfg.policy.allow.push(org_agent_runtime::Rule {
        tool: org_agent_runtime::RuleTool::try_from("NotebookEdit").unwrap(),
        scope: org_agent_runtime::RuleScope::Any,
    });

    // Finding 1: NotebookEdit (`capability: None`, `kind: FileChange`) must still hit `cap:edit` denies.
    {
        let adm_link = wt.join("adm");
        org_platform::fs::create_symlink(&admin_dir, &adm_link).unwrap();
        let dangling_cw = wt.join("cw");
        org_platform::fs::create_symlink(&admin_dir.join("config.worktree"), &dangling_cw).unwrap();

        for target in [adm_link.join("config.worktree"), wt.join(".git")] {
            let nb_req = ApprovalRequest::new(
                ApprovalId::new("nb-deny"),
                ApprovalKind::FileChange,
                Escalation::None,
                "NotebookEdit",
                serde_json::json!({ "notebook_path": target.to_str().unwrap() }),
            );
            assert!(
                matches!(
                    crate::preset::decide_escalation_with_roots(
                        &nb_req,
                        &cfg.policy,
                        cfg.preset,
                        None,
                        Some(cfg.sandbox.writable_roots.as_slice()),
                    ),
                    crate::preset::EscalationDisposition::AutoDeny { ref reason }
                        if reason.contains("denied by office security policy")
                ),
                "NotebookEdit (capability: None) must AutoDeny via policy deny on target: {}",
                target.display()
            );
        }

        // Finding 2: Unsandboxed (`writable_roots: None`) dangling symlink & symlink+`..` traversal.
        for unsandboxed_bypass in [
            dangling_cw.display().to_string(),
            format!("{}/sub/../config.worktree", adm_link.display()),
        ] {
            let mut req = ApprovalRequest::new(
                ApprovalId::new("unsbx"),
                ApprovalKind::FileChange,
                Escalation::None,
                "Write",
                serde_json::json!({ "file_path": unsandboxed_bypass, "content": "x" }),
            );
            req.capability = Some(org_agent_runtime::ToolCapability::Edit);
            assert!(
                matches!(
                    crate::preset::decide_escalation_with_roots(
                        &req,
                        &cfg.policy,
                        cfg.preset,
                        cfg.allowed_tools.as_deref(),
                        None,
                    ),
                    crate::preset::EscalationDisposition::AutoDeny { .. }
                ),
                "unsandboxed deny bypass must AutoDeny: {unsandboxed_bypass}"
            );
        }
    }

    // Finding 3: macOS APFS case-folded absent denied target inside writable admin dir.
    if org_platform::fs::paths_case_insensitive_by_default() {
        let mut upper = ApprovalRequest::new(
            ApprovalId::new("apfs-case"),
            ApprovalKind::FileChange,
            Escalation::None,
            "Write",
            serde_json::json!({
                "file_path": admin_dir.join("CONFIG.WORKTREE").to_str().unwrap(),
                "content": "x"
            }),
        );
        upper.capability = Some(org_agent_runtime::ToolCapability::Edit);
        assert!(matches!(
            crate::preset::decide_escalation_with_roots(
                &upper,
                &cfg.policy,
                cfg.preset,
                cfg.allowed_tools.as_deref(),
                Some(cfg.sandbox.writable_roots.as_slice()),
            ),
            crate::preset::EscalationDisposition::AutoDeny { .. }
        ));
    }

    // Finding 4: Dual-key array `changes` item and `notebookPath` inside object `changes`.
    for payload in [
        serde_json::json!({
            "changes": [{
                "path": wt.join("ok.rs").to_str().unwrap(),
                "file_path": cargo_home.join("config.toml").to_str().unwrap()
            }]
        }),
        serde_json::json!({
            "changes": [{
                "path": wt.join("ok.rs").to_str().unwrap(),
                "file_path": 123
            }]
        }),
        serde_json::json!({
            "fileChanges": {
                wt.join("ok.rs").to_str().unwrap(): {
                    "notebookPath": cargo_home.join("nb.ipynb").to_str().unwrap()
                }
            }
        }),
    ] {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("chg-dual"),
            ApprovalKind::FileChange,
            Escalation::None,
            "ApplyPatch",
            payload,
        );
        req.capability = Some(org_agent_runtime::ToolCapability::Edit);
        assert!(matches!(
            crate::preset::decide_escalation_with_roots(
                &req,
                &cfg.policy,
                cfg.preset,
                cfg.allowed_tools.as_deref(),
                Some(cfg.sandbox.writable_roots.as_slice()),
            ),
            crate::preset::EscalationDisposition::AutoDeny { .. }
        ));
    }
}
