//! Runtime registry and real supervised session launches, multi-turn, and respawn.

use super::*;

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
        ("gemini", "gemini"),
        ("gemini-cli", "gemini"),
        ("kimi", "kimi"),
        ("moonshot", "kimi"),
        ("pi", "pi"),
        ("pi-acp", "pi"),
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
            repository_instructions: String::new(),
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
            operator_shell_prefixes: Vec::new(),
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
        assert_eq!(plan.stdio, org_agent_runtime::Stdio::Piped);
        if expected_id != "pi" {
            assert!(
                !plan.args.is_empty(),
                "{expected_id} should pass ACP/CLI args"
            );
        }
    }
    assert!(matches!(
        reg.resolve("nonexistent-runtime"),
        Err(crate::RuntimeRegistryError::UnknownRuntime(_))
    ));
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
                effort: None,
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
                effort: None,
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
