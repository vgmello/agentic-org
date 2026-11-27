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
        repository_instructions: String::new(),
        permission_mode: preset.permission_mode,
        policy: preset.policy,
        sandbox: preset.sandbox,
        mcp_servers: Default::default(),
        tools: None,
        operator_shell_prefixes: Vec::new(),
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
        repository_instructions: String::new(),
        permission_mode: ro.permission_mode,
        policy: ro.policy.clone(),
        sandbox: ro.sandbox.clone(),
        mcp_servers: Default::default(),
        tools: ro.allowed_tools.clone(),
        operator_shell_prefixes: Vec::new(),
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

fn hire_autonomous_agent(
    office: &Office,
    root_team_id: &str,
    owner_id: &str,
    name: &str,
    handle: &str,
) -> String {
    let req = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: new_uuid_v7(),
            home_team_id: root_team_id.to_owned(),
            role_id: None,
            proposal: HireProposalWire {
                name: name.into(),
                handle: handle.into(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner_id.to_owned()),
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
    let resp = office.dispatch_hiring_rpc(Some(owner_id), &req).unwrap();
    match resp.outcome {
        org_proto::Outcome::Result(v) => {
            serde_json::from_value::<org_proto::hiring::HireOpResult>(v)
                .unwrap()
                .employee_id
                .unwrap()
        }
        org_proto::Outcome::Error(e) => panic!("hire failed: {e:?}"),
    }
}

fn capture_cargo_env(
    mgr: &SessionManager,
    home: &std::path::Path,
    workspace: &std::path::Path,
    tmpdir: &std::path::Path,
    emp_id: &str,
    sandbox_enabled: bool,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let bind = tmpdir.join("b");
    mgr.captured_env(&crate::manager::SessionEnvInput {
        home,
        agent_base: home.parent(),
        workspace,
        tmpdir,
        employee_id: emp_id,
        employee_name: "Cargo Agent",
        employee_handle: "cargo-agent",
        session_id: "sess-1",
        token: "tok-1",
        binding_file: &bind,
        sandbox_enabled,
        operator: false,
    })
}

#[test]
fn task135_per_agent_cargo_isolation_readonly_toolchain_and_office_vault_deny() {
    use org_agent_runtime::SpecPath;

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let emp_a = hire_autonomous_agent(&office, &root_team_id, &owner_id, "Cargo A", "cargo-a");
    let emp_b = hire_autonomous_agent(&office, &root_team_id, &owner_id, "Cargo B", "cargo-b");

    let home_a = office.ensure_private_home(&emp_a).unwrap();
    let home_b = office.ensure_private_home(&emp_b).unwrap();
    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    );
    let (_rt_a, tmp_a) = mgr.prepare_run_dirs(&emp_a).unwrap();
    let (_rt_b, tmp_b) = mgr.prepare_run_dirs(&emp_b).unwrap();

    // Worktree workspace is separate from agent home.
    let ws_a = tempfile::tempdir().unwrap();
    let ws_a_canon = ws_a.path().canonicalize().unwrap();
    let ws_a_spec = SpecPath::try_from(ws_a_canon.to_str().unwrap()).unwrap();
    let cfg_a = mgr
        .build_employee_preset_cfg(
            &emp_a,
            PermissionPreset::AutonomousSandboxed,
            true,
            &ws_a_spec,
            true,
        )
        .unwrap();
    let writable_a: Vec<&str> = cfg_a
        .sandbox
        .writable_roots
        .iter()
        .map(SpecPath::as_str)
        .collect();
    let deny_read_a: Vec<&str> = cfg_a.sandbox.deny_read.iter().map(|p| p.as_str()).collect();

    let expected_cargo_a = home_a.join(".cargo").canonicalize().unwrap();
    let expected_target_a = home_a
        .parent()
        .unwrap()
        .join("cache/cargo-target")
        .canonicalize()
        .unwrap();
    assert!(writable_a.contains(&expected_cargo_a.to_str().unwrap()));
    assert!(writable_a.contains(&expected_target_a.to_str().unwrap()));
    assert!(!writable_a.iter().any(|p| p.contains(&emp_b)));
    if let Some(host_home) = std::env::var_os("HOME") {
        let host_home = std::path::PathBuf::from(host_home);
        for host_ro in [host_home.join(".cargo"), host_home.join(".rustup")] {
            let s = host_ro.to_string_lossy();
            assert!(!writable_a.iter().any(|p| *p == s.as_ref()));
        }
    }
    assert!(deny_read_a
        .iter()
        .any(|p| p.ends_with(&format!("/agents/{emp_b}/**"))));
    let git_creds_glob = format!(
        "{}/**",
        office
            .folder()
            .join("git-credentials")
            .canonicalize()
            .unwrap_or_else(|_| office.folder().join("git-credentials"))
            .display()
    );
    assert!(deny_read_a.iter().any(|p| *p == git_creds_glob));

    for ro_preset in [
        PermissionPreset::ReadOnly,
        PermissionPreset::OperatorSandboxed,
    ] {
        let ro_cfg = mgr
            .build_employee_preset_cfg(&emp_a, ro_preset, true, &ws_a_spec, false)
            .unwrap();
        assert!(ro_cfg.sandbox.writable_roots.is_empty());
    }

    let get = |env: &[(std::ffi::OsString, std::ffi::OsString)], key: &str| {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    let env_a = capture_cargo_env(&mgr, &home_a, &ws_a_canon, &tmp_a, &emp_a, true);
    let env_b = capture_cargo_env(&mgr, &home_b, &ws_a_canon, &tmp_b, &emp_b, true);
    let env_unsandboxed = capture_cargo_env(&mgr, &home_a, &ws_a_canon, &tmp_a, &emp_a, false);
    assert_eq!(
        get(&env_a, "CARGO_HOME").as_deref(),
        Some(expected_cargo_a.as_os_str())
    );
    assert_eq!(
        get(&env_a, "CARGO_TARGET_DIR").as_deref(),
        Some(expected_target_a.as_os_str())
    );
    assert_eq!(
        get(&env_a, "CARGO_NET_OFFLINE").as_deref(),
        Some(std::ffi::OsStr::new("true"))
    );
    assert_ne!(get(&env_a, "CARGO_HOME"), get(&env_b, "CARGO_HOME"));
    assert_ne!(
        get(&env_a, "CARGO_TARGET_DIR"),
        get(&env_b, "CARGO_TARGET_DIR")
    );
    assert!(get(&env_unsandboxed, "CARGO_NET_OFFLINE").is_none());
}

#[test]
fn task135_concurrent_offline_cargo_check_with_readonly_host_cache() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let host_cargo = root.join("ro-host-cargo");
    std::fs::create_dir_all(host_cargo.join("registry/index")).unwrap();
    std::fs::create_dir_all(host_cargo.join("registry/cache")).unwrap();
    std::fs::create_dir_all(host_cargo.join("git/db")).unwrap();

    // Lock the synthetic host Cargo cache read-only (`0555`) to prove neither agent writes to it.
    for ro in [
        host_cargo.join("registry/index"),
        host_cargo.join("registry/cache"),
        host_cargo.join("registry"),
        host_cargo.join("git/db"),
        host_cargo.join("git"),
        host_cargo.clone(),
    ] {
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    // Build a self-contained offline Cargo workspace with a local path dependency (zero network).
    let crate_dir = root.join("sample-app");
    let dep_dir = root.join("sample-dep");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::create_dir_all(dep_dir.join("src")).unwrap();
    std::fs::write(
        dep_dir.join("Cargo.toml"),
        "[package]\nname = \"sample-dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        dep_dir.join("src/lib.rs"),
        "pub fn answer() -> u32 { 42 }\n",
    )
    .unwrap();
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"sample-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsample-dep = {{ path = {:?} }}\n",
            dep_dir
        ),
    )
    .unwrap();
    std::fs::write(
        crate_dir.join("src/lib.rs"),
        "#[test]\nfn works() { assert_eq!(sample_dep::answer(), 42); }\n",
    )
    .unwrap();

    // Prepare two isolated agents pointing at the SAME read-only `host_cargo`.
    let prepare_agent = |name: &str| {
        let base = root.join(name);
        let home = base.join("home");
        org_platform::fs::create_owner_only_dir_all(&home).unwrap();
        org_platform::fs::set_owner_only_dir(&base).unwrap();
        org_platform::fs::set_owner_only_dir(&home).unwrap();
        let mut env = vec![(
            std::ffi::OsString::from("HOME"),
            home.as_os_str().to_os_string(),
        )];
        crate::manager::append_rust_toolchain_env(
            &mut env,
            crate::manager::RustToolchainEnvInput {
                agent_home: Some(&home),
                agent_base: Some(&base),
                rustup_explicit: std::env::var_os("RUSTUP_HOME"),
                cargo_explicit: Some(host_cargo.as_os_str().to_os_string()),
                host_home: std::env::var_os("HOME"),
                forbidden: &[home.clone(), crate_dir.clone()],
                sandbox_enabled: true,
            },
        );
        (base, home, env)
    };
    let (base_a, home_a, env_a) = prepare_agent("agent-a");
    let (base_b, home_b, env_b) = prepare_agent("agent-b");

    let run_cargo = |env: Vec<(std::ffi::OsString, std::ffi::OsString)>| {
        let crate_dir = crate_dir.clone();
        std::thread::spawn(move || {
            let mut cmd = std::process::Command::new("cargo");
            cmd.args(["test", "--quiet"])
                .current_dir(&crate_dir)
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default());
            for (k, v) in env {
                cmd.env(k, v);
            }
            cmd.output().expect("run cargo test")
        })
    };
    let t_a = run_cargo(env_a);
    let t_b = run_cargo(env_b);
    let out_a = t_a.join().unwrap();
    let out_b = t_b.join().unwrap();

    // Restore host_cargo perms so tempdir cleans up reliably even if an assertion fails.
    for rw in [
        host_cargo.clone(),
        host_cargo.join("registry"),
        host_cargo.join("registry/index"),
        host_cargo.join("registry/cache"),
        host_cargo.join("git"),
        host_cargo.join("git/db"),
    ] {
        let _ = std::fs::set_permissions(&rw, std::fs::Permissions::from_mode(0o700));
    }

    assert!(
        out_a.status.success(),
        "agent A offline cargo test failed: {}",
        String::from_utf8_lossy(&out_a.stderr)
    );
    assert!(
        out_b.status.success(),
        "agent B offline cargo test failed: {}",
        String::from_utf8_lossy(&out_b.stderr)
    );
    // Each agent compiled into its own isolated `CARGO_TARGET_DIR` (never sharing `sample-app/target`).
    assert!(!crate_dir.join("target").exists());
    assert!(base_a.join("cache/cargo-target/debug").is_dir());
    assert!(base_b.join("cache/cargo-target/debug").is_dir());
    // Neither agent wrote any lock/cache file inside the read-only host Cargo dir.
    assert!(!host_cargo.join(".package-cache").exists());
    assert!(!host_cargo.join(".global-cache").exists());
    assert!(home_a.join(".cargo").is_dir());
    assert!(home_b.join(".cargo").is_dir());
}
