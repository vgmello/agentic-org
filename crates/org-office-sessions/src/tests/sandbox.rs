//! Worktree sandbox grants, team isolation, corrupt git state, and Rust dir env (`TASK-126`).

use super::*;

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
                    effort: None,
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
        "/worktrees/self-admin/config.worktree",
        "/.git",
    ] {
        assert!(
            deny_strs.iter().any(|s| s.ends_with(needle)),
            "worktree_sandbox_grants missing write deny `{needle}`: {deny_strs:?}"
        );
    }
    // Optional non-existent bare-cache files (`packed-refs`, `shallow`) and `.git/hooks` on a
    // worktree `.git` file must NOT be emitted into `deny_write` (Linux bwrap turns missing
    // denyWrite files into EACCES stubs; `<cache>` stays outside `writable_roots`).
    for absent in ["/packed-refs", "/shallow", "/.git/hooks"] {
        assert!(
            !deny_strs.iter().any(|s| s.ends_with(absent)),
            "absent optional git path `{absent}` must not be emitted into deny_write: {deny_strs:?}"
        );
    }
    // A dangling symlink at `packed-refs` MUST be detected via `symlink_metadata` and write-denied.
    org_platform::fs::create_symlink(
        &cache_dir.join("nonexistent-target"),
        &cache_dir.join("packed-refs"),
    )
    .unwrap();
    let (_, wt_write_denies_dangling) = mgr.worktree_sandbox_grants(&self_emp_id, false);
    assert!(
        wt_write_denies_dangling
            .iter()
            .any(|p| p.as_str().ends_with("/packed-refs")),
        "dangling symlink at packed-refs must still be write-denied"
    );
    std::fs::remove_file(cache_dir.join("packed-refs")).unwrap();
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
                    effort: None,
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
                    effort: None,
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
