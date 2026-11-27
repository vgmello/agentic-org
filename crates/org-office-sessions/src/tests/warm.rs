//! Warm-session permits, stale-route recycling, and topology digests (`TASK-126`).

use super::*;

#[test]
fn task126_warm_permit_turns_stale_on_execution_config_revision_change_and_401_reply_filter() {
    use org_proto::hiring::{AgentExecutionSet, AgentExecutionSetParams};

    // The 401 reply filter is runtime-owned (`AgentRuntime::reply_signals_auth_expired`).
    let claude = org_agent_runtime_claude::ClaudeRuntime::new();
    let codex = org_agent_runtime_codex::CodexRuntime::new();
    assert!(claude.reply_signals_auth_expired(
        r#"Failed to authenticate. API Error: 401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired."}}"#
    ));
    assert!(claude.reply_signals_auth_expired("401 OAuth access token has expired"));
    assert!(!claude.reply_signals_auth_expired(
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
                effort: None,
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
            effort: None,
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
            &claude,
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
            &claude,
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
            &codex,
            &org_agent_runtime::TurnOutcome::Completed,
            &[],
            Some("401 OAuth access token has expired"),
            false,
            false,
        ),
        None
    );
    let long_prose = format!("401 OAuth access token has expired {}", "x".repeat(1200));
    assert!(!claude.reply_signals_auth_expired(&long_prose));
    assert_eq!(
        crate::pump::classify_turn_failover_reason(
            &claude,
            &org_agent_runtime::TurnOutcome::Completed,
            &[],
            Some("401 OAuth access token has expired"),
            false,
            false,
        ),
        Some("auth_expired"),
        "a Claude reply-only 401 with no side effects fails over"
    );
    assert_eq!(
        crate::pump::classify_turn_failover_reason(
            &claude,
            &org_agent_runtime::TurnOutcome::Completed,
            &[],
            Some("401 OAuth access token has expired"),
            true,
            false,
        ),
        None,
        "a reply-only 401 after side effects never fails over"
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
        writable_roots: Vec::new(),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
        queued_turns: 0,
        activity: Default::default(),
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
                supports_mid_turn: false,
                bwrap_masked_siblings: None,
                partition: None,
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
            stop_epoch: 0,
            partition: None,
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
                    effort: None,
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
    let hooks = mgr.build_pump_hooks(&emp_a, "digest".into(), Some(1), None, false);
    let acquire = hooks.on_permit_acquire.expect("permit acquire hook");
    let err = acquire("conn-1", None).expect_err("expiring OAuth must fail warm permit acquire");
    assert!(
        err.starts_with("WARM_ROUTE_STALE"),
        "expected WARM_ROUTE_STALE on expiring OAuth, got: {err}"
    );
}
