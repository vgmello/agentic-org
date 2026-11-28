//! Approval bridge: unredacted evaluation, allow-always refusals, writable roots, and deny
//! bypass resistance (`TASK-126`).

use super::*;

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
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
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
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
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

#[test]
fn task126_approval_bridge_enforces_writable_roots_and_strict_git_admin_names() {
    use org_office::git::{OfficeGitState, TaskWorkspaceRecord};

    let dir = tempfile::tempdir().unwrap();
    let canon_dir = dir.path().canonicalize().unwrap();
    let ws = canon_dir.join("workspace");
    let wt = canon_dir.join("worktree");
    let cargo_home = canon_dir.join("host-cargo");
    let bare_cache = canon_dir.join("cache.git");
    let admin_dir = bare_cache.join("worktrees/self-admin");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::create_dir_all(&cargo_home).unwrap();
    std::fs::create_dir_all(&admin_dir).unwrap();
    std::fs::write(wt.join("src/lib.rs"), "pub fn ok() {}\n").unwrap();

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
    let write_denies = vec![org_agent_runtime::PathPattern::try_from(
        admin_dir.join("config.worktree").to_str().unwrap(),
    )
    .unwrap()];
    crate::preset::apply_worktree_sandbox_rules(&mut cfg, &[wt_spec, admin_spec], &write_denies);

    let edit_req = |path: &std::path::Path| {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("req-1"),
            ApprovalKind::FileChange,
            Escalation::None,
            "Write",
            serde_json::json!({ "file_path": path.to_str().unwrap(), "content": "x" }),
        );
        req.capability = Some(org_agent_runtime::ToolCapability::Edit);
        req
    };

    // Allowed: existing and new files inside worktree writable root.
    assert_eq!(
        crate::preset::decide_escalation_with_roots(
            &edit_req(&wt.join("src/lib.rs")),
            &cfg.policy,
            cfg.preset,
            cfg.allowed_tools.as_deref(),
            Some(cfg.sandbox.writable_roots.as_slice()),
        ),
        crate::preset::EscalationDisposition::AllowByPolicy
    );
    assert_eq!(
        crate::preset::decide_escalation_with_roots(
            &edit_req(&wt.join("src/new_dir/new_mod.rs")),
            &cfg.policy,
            cfg.preset,
            cfg.allowed_tools.as_deref(),
            Some(cfg.sandbox.writable_roots.as_slice()),
        ),
        crate::preset::EscalationDisposition::AllowByPolicy
    );

    // Denied: host `$CARGO_HOME/config.toml`, absent bare-cache metadata (`packed-refs`,
    // `refs/replace/x`), and `worktrees/self-admin/config.worktree` (inside writable admin_dir
    // but explicitly write-denied).
    for forbidden in [
        cargo_home.join("config.toml"),
        bare_cache.join("packed-refs"),
        bare_cache.join("refs/replace/deadbeef"),
        admin_dir.join("config.worktree"),
    ] {
        assert!(
            matches!(
                crate::preset::decide_escalation_with_roots(
                    &edit_req(&forbidden),
                    &cfg.policy,
                    cfg.preset,
                    cfg.allowed_tools.as_deref(),
                    Some(cfg.sandbox.writable_roots.as_slice()),
                ),
                crate::preset::EscalationDisposition::AutoDeny { .. }
            ),
            "expected AutoDeny for write target outside writable_roots/denied: {}",
            forbidden.display()
        );
    }

    // Denied: leaf and intermediate dangling symlinks inside writable root pointing outside.
    let leaf_link = wt.join("escape-leaf.toml");
    org_platform::fs::create_symlink(&cargo_home.join("config.toml"), &leaf_link).unwrap();
    let dir_link = wt.join("escape-dir");
    org_platform::fs::create_symlink(&cargo_home.join("missing-subdir"), &dir_link).unwrap();
    for escaped in [leaf_link, dir_link.join("config.toml")] {
        assert!(
            matches!(
                crate::preset::decide_escalation_with_roots(
                    &edit_req(&escaped),
                    &cfg.policy,
                    cfg.preset,
                    cfg.allowed_tools.as_deref(),
                    Some(cfg.sandbox.writable_roots.as_slice()),
                ),
                crate::preset::EscalationDisposition::AutoDeny { .. }
            ),
            "dangling symlink escape must be AutoDenied: {}",
            escaped.display()
        );
    }

    // Verify `.git` as directory emits `.git/hooks` and `.git/hooks/**`, and unsafe glob/whitespace
    // chars in `repository_id` / `admin_dir_name` are rejected from worktree grants.
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let hire_req = Request::new::<HrHire>(
        127,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: new_uuid_v7(),
            home_team_id: root_team_id.clone(),
            role_id: None,
            proposal: HireProposalWire {
                name: "Dir Git Agent".into(),
                handle: "dir-git-agent".into(),
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
                effort: None,
            },
            reason: "Dir git test hire".into(),
        },
    )
    .unwrap();
    let emp_id = match office
        .dispatch_hiring_rpc(Some(&owner_id), &hire_req)
        .unwrap()
        .outcome
    {
        org_proto::Outcome::Result(v) => {
            serde_json::from_value::<org_proto::hiring::HireOpResult>(v)
                .unwrap()
                .employee_id
                .unwrap()
        }
        org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
    };
    let office_id = office.info().office_id.clone();
    let office_rt = office.runtime_root().join(&office_id);
    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    );
    let repo_ok = "01a212a8-982e-71ca-a6de-322dba762f01";
    let emp_root = office_rt
        .join("teams")
        .join(&root_team_id)
        .join("agents")
        .join(&emp_id);
    let wt_with_dir_git = emp_root.join("tasks/t-dir/repositories").join(repo_ok);
    std::fs::create_dir_all(wt_with_dir_git.join(".git")).unwrap();
    std::fs::create_dir_all(
        office_rt
            .join("repositories")
            .join(format!("{repo_ok}.git/worktrees/adm-ok")),
    )
    .unwrap();
    let mut gs = OfficeGitState::default();
    gs.workspaces.insert(
        OfficeGitState::workspace_key("t-dir", repo_ok, &emp_id),
        TaskWorkspaceRecord {
            office_id: office_id.clone(),
            employee_id: emp_id.clone(),
            task_id: "t-dir".into(),
            task_key: "T-DIR".into(),
            repository_id: repo_ok.into(),
            repository_key: Some("ok".into()),
            branch: "task/T-DIR".into(),
            worktree_path: wt_with_dir_git.display().to_string(),
            admin_dir_name: "adm-ok".into(),
            base_commit: "abc".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    for (idx, bad) in [
        "bad]name",
        "bad(name)",
        "bad{name}",
        "bad~name",
        "bad name",
        "bad*name",
        "bad?name",
        "bad[name",
    ]
    .iter()
    .enumerate()
    {
        let bad_wt = emp_root.join(format!("tasks/t-check-{idx}/repositories/repo"));
        std::fs::create_dir_all(&bad_wt).unwrap();
        gs.workspaces.insert(
            OfficeGitState::workspace_key(&format!("t-bad-admin-{idx}"), repo_ok, &emp_id),
            TaskWorkspaceRecord {
                office_id: office_id.clone(),
                employee_id: emp_id.clone(),
                task_id: format!("t-bad-admin-{idx}"),
                task_key: format!("T-BA-{idx}"),
                repository_id: repo_ok.into(),
                repository_key: Some("ok".into()),
                branch: "task/T-BA".into(),
                worktree_path: bad_wt.display().to_string(),
                admin_dir_name: (*bad).into(),
                base_commit: "abc".into(),
                pushed_head: None,
                state: "active".into(),
            },
        );
        gs.workspaces.insert(
            OfficeGitState::workspace_key(&format!("t-bad-repo-{idx}"), bad, &emp_id),
            TaskWorkspaceRecord {
                office_id: office_id.clone(),
                employee_id: emp_id.clone(),
                task_id: format!("t-bad-repo-{idx}"),
                task_key: format!("T-BR-{idx}"),
                repository_id: (*bad).into(),
                repository_key: Some("bad".into()),
                branch: "task/T-BR".into(),
                worktree_path: bad_wt.display().to_string(),
                admin_dir_name: "adm-ok".into(),
                base_commit: "abc".into(),
                pushed_head: None,
                state: "active".into(),
            },
        );
    }
    gs.save(office.runtime_root(), &office_id).unwrap();

    let (writable, denies) = mgr.worktree_sandbox_grants(&emp_id, false);
    assert!(
        denies.iter().any(|p| p.as_str().ends_with("/.git/hooks"))
            && denies
                .iter()
                .any(|p| p.as_str().ends_with("/.git/hooks/**")),
        "directory .git must emit .git/hooks and .git/hooks/** write denies: {denies:?}"
    );
    for s in writable
        .iter()
        .map(|p| p.as_str())
        .chain(denies.iter().map(|p| p.as_str()))
    {
        assert!(
            !s.contains("bad"),
            "unsafe admin_dir_name or repository_id leaked into sandbox rules: {s}"
        );
    }
}

#[test]
fn task126_approval_bridge_blocks_canonical_deny_bypasses_trailing_slash_symlinks_and_opaque_changes(
) {
    let dir = tempfile::tempdir().unwrap();
    let canon_dir = dir.path().canonicalize().unwrap();
    let ws = canon_dir.join("workspace");
    let wt = canon_dir.join("worktree");
    let cargo_home = canon_dir.join("host-cargo");
    let bare_cache = canon_dir.join("cache.git");
    let admin_dir = bare_cache.join("worktrees/self-admin");
    let objects_dir = bare_cache.join("objects");
    let objects_info = objects_dir.join("info");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::create_dir_all(&cargo_home).unwrap();
    std::fs::create_dir_all(&admin_dir).unwrap();
    std::fs::create_dir_all(&objects_info).unwrap();
    std::fs::write(
        wt.join(".git"),
        "gitdir: ../cache.git/worktrees/self-admin\n",
    )
    .unwrap();

    let ws_spec = SpecPath::try_from(ws.to_str().unwrap()).unwrap();
    let wt_spec = SpecPath::try_from(wt.to_str().unwrap()).unwrap();
    let admin_spec = SpecPath::try_from(admin_dir.to_str().unwrap()).unwrap();
    let objects_spec = SpecPath::try_from(objects_dir.to_str().unwrap()).unwrap();

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
        org_agent_runtime::PathPattern::try_from(format!("{}/**", objects_info.display()).as_str())
            .unwrap(),
    ];
    crate::preset::apply_worktree_sandbox_rules(
        &mut cfg,
        &[wt_spec.clone(), admin_spec.clone(), objects_spec],
        &write_denies,
    );

    let edit_raw = |raw: &str| {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("req-bypass"),
            ApprovalKind::FileChange,
            Escalation::None,
            "Write",
            serde_json::json!({ "file_path": raw, "content": "evil" }),
        );
        req.capability = Some(org_agent_runtime::ToolCapability::Edit);
        req
    };

    // Finding 1A: `/./`, `//`, and non-canonical path forms cannot evade fixed write denies.
    for evasive in [
        format!("{}/./config.worktree", admin_dir.display()),
        format!("{}//config.worktree", admin_dir.display()),
        format!("{}/./.git", wt.display()),
        format!("{}/./info/alternates", objects_dir.display()),
    ] {
        assert!(
            matches!(
                crate::preset::decide_escalation_with_roots(
                    &edit_raw(&evasive),
                    &cfg.policy,
                    cfg.preset,
                    cfg.allowed_tools.as_deref(),
                    Some(cfg.sandbox.writable_roots.as_slice()),
                ),
                crate::preset::EscalationDisposition::AutoDeny { .. }
            ),
            "evasive path must be AutoDenied: {evasive}"
        );
    }

    // Finding 1B + 2: in-root symlinks to denied targets and trailing `/` / `/.` on dangling symlinks.
    {
        let adm_link = wt.join("adm");
        org_platform::fs::create_symlink(&admin_dir, &adm_link).unwrap();
        let git_link = wt.join("g");
        org_platform::fs::create_symlink(&wt.join(".git"), &git_link).unwrap();
        let oi_link = wt.join("oi");
        org_platform::fs::create_symlink(&objects_info, &oi_link).unwrap();
        let dangling = wt.join("escape");
        org_platform::fs::create_symlink(&cargo_home.join("config.toml"), &dangling).unwrap();

        for bypass in [
            format!("{}/config.worktree", adm_link.display()),
            git_link.display().to_string(),
            format!("{}/alternates", oi_link.display()),
            format!("{}/", dangling.display()),
            format!("{}/.", dangling.display()),
        ] {
            assert!(
                matches!(
                    crate::preset::decide_escalation_with_roots(
                        &edit_raw(&bypass),
                        &cfg.policy,
                        cfg.preset,
                        cfg.allowed_tools.as_deref(),
                        Some(cfg.sandbox.writable_roots.as_slice()),
                    ),
                    crate::preset::EscalationDisposition::AutoDeny { .. }
                ),
                "symlink/trailing-slash bypass must be AutoDenied: {bypass}"
            );
        }
    }

    // Finding 2b: filesystem root `/` is ignored as a writable root.
    let root_only = vec![SpecPath::try_from("/").unwrap()];
    assert!(matches!(
        crate::preset::decide_escalation_with_roots(
            &edit_raw(&wt.join("ok.rs").display().to_string()),
            &cfg.policy,
            cfg.preset,
            cfg.allowed_tools.as_deref(),
            Some(root_only.as_slice()),
        ),
        crate::preset::EscalationDisposition::AutoDeny { .. }
    ));
}

fn add_test_non_owner(office: &Office, owner_id: &str, team_id: &str) -> String {
    let member_id = new_uuid_v7();
    office
        .submit_org(
            owner_id,
            org_office::rules::OrgCommand::MemberAddPerson(org_proto::org::MemberAddPersonParams {
                command_id: new_uuid_v7(),
                member_id: member_id.clone(),
                name: "Bob NonOwner".into(),
                contact: None,
                handle: Some("bob-nonowner".into()),
                home_team_id: Some(team_id.to_owned()),
            }),
        )
        .unwrap()
        .unwrap();
    member_id
}

fn wait_for_pending_approvals(
    manager: &SessionManager,
    owner_id: &str,
    expected: usize,
) -> Vec<crate::PendingToolApproval> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let list = manager.pending_approvals_for(owner_id).unwrap();
        if list.len() == expected {
            return list;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    manager.pending_approvals_for(owner_id).unwrap()
}

fn build_hook_test_manager(
    office: &Arc<Office>,
    bin_dir: &Path,
    sock_dir: &Path,
) -> SessionManager {
    let fake_claude = write_fake_cli(
        bin_dir,
        "claude",
        r#"#!/bin/sh
read init_line
echo '{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ada-hook"],"current_permission_mode":"default","pid":1236,"version":"2.1.311"}}}'
read user_line
echo '{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000003","permissionMode":"default"}'
echo '{"type":"control_request","request_id":"apr-hook-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"agent-os inbox --body Bearer secret_tok_999;id"}}}'
echo '{"type":"control_request","request_id":"apr-hook-2","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"api_key":"synthetic-key-42"}}}'
echo '{"type":"control_request","request_id":"apr-hook-3","request":{"subtype":"can_use_tool","tool_name":"mystery_tool","input":{"token":"synthetic-stop-tok"}}}'
read d1
read d2
read d3
echo '{"type":"result","subtype":"success","is_error":false,"result":"done"}'
"#,
    );
    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()),
        Some(fake_claude),
    );
    let mut cfg = SessionManagerConfig::production(sock_dir.join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(bin_dir.to_path_buf());
    SessionManager::new(Arc::clone(office), cfg)
}

fn assert_approval_lifecycle_transitions(
    manager: &SessionManager,
    owner_id: &str,
    emp_id: &str,
    hook_hits: &std::sync::atomic::AtomicUsize,
) {
    use std::sync::atomic::Ordering;

    let before_approve = hook_hits.load(Ordering::SeqCst);
    manager
        .decide_approval(owner_id, emp_id, "apr-hook-1", "allow_once", None)
        .expect("approve succeeded");
    assert!(hook_hits.load(Ordering::SeqCst) > before_approve);
    assert_eq!(manager.pending_approvals_for(owner_id).unwrap().len(), 2);

    let before_deny = hook_hits.load(Ordering::SeqCst);
    manager
        .decide_approval(
            owner_id,
            emp_id,
            "apr-hook-2",
            "deny",
            Some("not needed".into()),
        )
        .expect("deny succeeded");
    assert!(hook_hits.load(Ordering::SeqCst) > before_deny);
    assert_eq!(
        manager.pending_approvals_for(owner_id).unwrap()[0].approval_id,
        "apr-hook-3"
    );

    let before_stop = hook_hits.load(Ordering::SeqCst);
    manager
        .stop_session(emp_id, "owner stopped")
        .expect("stop succeeded");
    assert!(hook_hits.load(Ordering::SeqCst) > before_stop);
    assert!(manager.pending_approvals_for(owner_id).unwrap().is_empty());
}

#[test]
fn task102_pending_approvals_for_enforces_owner_auth_redacts_and_fires_hook_on_lifecycle() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let non_owner_id = add_test_non_owner(&office, &owner_id, &root_team_id);
    let (emp_id, _conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "anthropic",
        "claude",
        "ada-hook",
        "claude-sonnet-4-6",
    );

    let bin_dir = tempfile::tempdir().unwrap();
    let sock_dir = tempfile::tempdir().unwrap();
    let manager = build_hook_test_manager(&office, bin_dir.path(), sock_dir.path());
    let hook_hits = Arc::new(AtomicUsize::new(0));
    let hook_counter = Arc::clone(&hook_hits);
    manager.set_approval_change_hook(Arc::new(move || {
        hook_counter.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(manager.pending_approvals_for(&owner_id).unwrap().is_empty());

    manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: emp_id.clone(),
                instruction: Some("Trigger approvals".into()),
                workspace: None,
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
            },
        )
        .expect("session started");

    let list = wait_for_pending_approvals(&manager, &owner_id, 3);
    assert_eq!(list.len(), 3, "all three pending approvals must surface");
    assert!(hook_hits.load(Ordering::SeqCst) >= 3);
    for entry in &list {
        let rendered = format!("{entry:?}");
        for secret in ["secret_tok_999", "synthetic-key-42", "synthetic-stop-tok"] {
            assert!(!rendered.contains(secret), "{rendered}");
        }
    }
    for unauthorized in [&non_owner_id, &emp_id] {
        let err = manager
            .pending_approvals_for(unauthorized)
            .expect_err("non-owner caller must be refused");
        assert_eq!(err.code, org_proto::ErrorCode::NotAuthorized);
    }

    assert_approval_lifecycle_transitions(&manager, &owner_id, &emp_id, &hook_hits);
}
