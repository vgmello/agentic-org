//! Linux Claude `bwrap` sibling isolation and fail-closed reaping (`TASK-166`).

use super::*;

#[test]
fn task166_bwrap_deny_dirs_fail_closed_and_precreate_unstarted_siblings_and_git_credentials() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = office.durable_root().join("run-sock");
    std::fs::create_dir_all(&sock_dir).unwrap();
    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(sock_dir.join("office.sock")),
    );
    let employees = office.list_employees().unwrap();
    assert!(!employees.is_empty());
    let me = employees[0].id.as_str().to_owned();
    let unstarted_sibling = "01a212a8-982e-7000-8000-000000000166";
    let office_id = office.info().office_id.clone();
    let office_local = office
        .durable_root()
        .join("local")
        .join("offices")
        .join(&office_id);
    // Register an unstarted sibling agent dir name by hiring or placing an employee dir check:
    // also verify all existing employees (like HR Lead) are pre-created as `0700` dirs.
    let sibling_ids = crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    assert!(
        !sibling_ids.is_empty(),
        "default test office must have at least one sibling employee (e.g. HR Lead)"
    );
    for other in &sibling_ids {
        let agent_dir = office_local.join("agents").join(other);
        assert!(
            agent_dir.is_dir(),
            "unstarted sibling dir must be pre-created"
        );
        assert_eq!(
            std::fs::symlink_metadata(&agent_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let slug = crate::host_auth::agent_tmpdir_slug(&office_id, other);
        let tmp_dir = sock_dir.join("t").join(slug);
        assert!(tmp_dir.is_dir(), "sibling tmpdir must be pre-created");
    }
    assert!(office.durable_root().join("authority").is_dir());
    assert!(office.folder().join("git-credentials").is_dir());
    // Cross-team sibling directories along the team hierarchy are pre-created (`TASK-166`).
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let child_team_a = new_uuid_v7();
    let child_team_b = new_uuid_v7();
    for (tid, name) in [(&child_team_a, "Team A"), (&child_team_b, "Team B")] {
        office
            .submit_org(
                &me,
                org_office::rules::OrgCommand::TeamCreate(org_proto::org::TeamCreateParams {
                    command_id: new_uuid_v7(),
                    team_id: tid.clone(),
                    name: name.into(),
                    owner_team_id: Some(root_team_id.clone()),
                }),
            )
            .unwrap()
            .unwrap();
    }
    let (emp_a, _) = hire_agent_with_runtime(
        &office,
        &me,
        &child_team_a,
        "anthropic",
        "claude",
        "team-a-eng",
        "claude-sonnet-4-6",
    );
    crate::manager::ensure_bwrap_deny_dirs(&mgr, &emp_a).unwrap();
    let office_rt = office.runtime_root().join(&office_id);
    assert!(
        office_rt
            .join("teams")
            .join(&root_team_id)
            .join("teams")
            .join(&child_team_b)
            .is_dir(),
        "sibling team dir must be pre-created for bwrap masking"
    );
    assert!(
        office_rt
            .join("teams")
            .join(&root_team_id)
            .join("agents")
            .is_dir(),
        "ancestor team agents dir must be pre-created for bwrap masking"
    );
    assert!(
        office_rt
            .join("teams")
            .join(&root_team_id)
            .join("teams")
            .join(&child_team_a)
            .join("teams")
            .is_dir(),
        "leaf child subteam dir must be pre-created for bwrap masking"
    );
    let denies = mgr.denied_office_globs_for(Some(&me));
    let git_creds_canon = office
        .folder()
        .join("git-credentials")
        .canonicalize()
        .unwrap();
    assert!(
        denies
            .iter()
            .any(|p| p.as_str().starts_with(git_creds_canon.to_str().unwrap())),
        "denied globs must cover host-local git-credentials dir"
    );

    // Fail-closed: if a required deny directory is replaced by a regular file, `ensure_bwrap_deny_dirs`
    // MUST return `Err` instead of silently swallowing it with `let _ = ...`.
    let tampered = office_local.join("agents").join(unstarted_sibling);
    std::fs::write(&tampered, b"not-a-dir").unwrap();
    let err = crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap_err();
    assert_eq!(err.code, org_proto::ErrorCode::StorageError);
}

#[test]
fn task166_enforce_bwrap_sibling_isolation_blocks_working_and_stops_idle_for_post_launch_sibling() {
    use std::sync::atomic::Ordering;

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = Arc::new(SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    ));
    let emp_a = "01a212a8-982e-7000-8000-000000000a01";
    let emp_b = "01a212a8-982e-7000-8000-000000000b02";
    let emp_c = "01a212a8-982e-7000-8000-000000000c03";

    // 1. Pre-launch sibling `emp_b` IS masked by `emp_a`, while post-launch `emp_c` is NOT.
    // While `emp_a` is actively `Running`:
    //   - `emp_b` passes cleanly without disturbing `emp_a`.
    //   - `emp_c` MUST fail closed with `Conflict` before any binding file is written.
    let masked: std::collections::BTreeSet<String> = [emp_b.to_owned()].into_iter().collect();
    let (handle_a, stopped_a) = spawn_stub_bwrap_handle(
        &office.info().office_id,
        emp_a,
        SessionState::Running,
        masked,
    );
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(emp_a.into(), handle_a);

    crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_b).unwrap();
    assert!(!stopped_a.load(Ordering::SeqCst));

    let conflict = crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap_err();
    assert_eq!(conflict.code, org_proto::ErrorCode::Conflict);
    assert!(!stopped_a.load(Ordering::SeqCst));

    // 2. While another sibling is concurrently in `reg.starting`, fail closed with `Conflict`.
    {
        mgr.registry.lock().unwrap().starting.insert(emp_a.into());
    }
    let starting_conflict =
        crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap_err();
    assert_eq!(starting_conflict.code, org_proto::ErrorCode::Conflict);
    {
        mgr.registry.lock().unwrap().starting.remove(emp_a);
    }

    // 3. Once `emp_a` becomes `Idle`, starting post-launch sibling `emp_c` MUST stop & reap
    // `emp_a`'s stale bwrap session, revoke its binding, release `reg.starting`, and record state.
    {
        let reg = mgr.registry.lock().unwrap();
        reg.live[emp_a].state.lock().unwrap().state = SessionState::Idle;
    }
    crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap();
    assert!(stopped_a.load(Ordering::SeqCst));
    assert!(!mgr.registry.lock().unwrap().live.contains_key(emp_a));
    assert!(!mgr.registry.lock().unwrap().starting.contains(emp_a));
}

#[test]
fn task166_panicking_bwrap_reap_persists_without_existing_file_and_blocks_stop_and_start() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = Arc::new(SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    ));
    let emp_b = "01a212a8-982e-7000-8000-000000000b02";
    let emp_c = "01a212a8-982e-7000-8000-000000000c03";
    let masked: std::collections::BTreeSet<String> = [emp_b.to_owned()].into_iter().collect();

    // Intentionally do NOT pre-seed `active-turn.v1.json`: panicking reap must upsert it (`B4-2`).
    let owner_id = office.owner_member_id().to_owned();
    let hr_agent_id = office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == org_office::organization_family::records::EmployeeKind::Agent)
        .unwrap()
        .id
        .as_str()
        .to_owned();
    assert!(mgr.store.read_active_turn(&hr_agent_id).unwrap().is_none());

    let panic_handle = spawn_panicking_bwrap_handle(&office.info().office_id, &hr_agent_id, masked);
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), panic_handle);

    let reap_err = crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap_err();
    assert_eq!(reap_err.code, org_proto::ErrorCode::Conflict);
    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));
    assert!(
        mgr.store
            .read_active_turn(&hr_agent_id)
            .unwrap()
            .unwrap()
            .recovery_required,
        "panicking bwrap reap must upsert recovery_required=true even when active-turn.v1.json was absent"
    );

    // `stop_session` on a restored RecoveryRequired handle MUST fail closed and keep it in `reg.live` (`B4-1`).
    let stop_err = mgr.stop_session(&hr_agent_id, "bypass_stop").unwrap_err();
    assert_eq!(stop_err.code, org_proto::ErrorCode::Conflict);
    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));

    // `start_session` on `hr_agent_id` itself MUST fail closed.
    let start_hr_err = mgr
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: hr_agent_id.clone(),
                instruction: Some("bypass?".into()),
                workspace: None,
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
            },
        )
        .unwrap_err();
    assert_eq!(start_hr_err.code, org_proto::ErrorCode::Conflict);
    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));

    // Retry on `emp_c` must STILL fail closed until owner clears recovery on `hr_agent_id`.
    let retry_err = crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap_err();
    assert_eq!(retry_err.code, org_proto::ErrorCode::Conflict);
    mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap();
    assert!(!mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));
    crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap();
}

#[test]
fn task166_start_and_recycle_fail_closed_on_panicking_stale_bwrap_and_stop_all_drains_cleanly() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = Arc::new(SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    ));
    let owner_id = office.owner_member_id().to_owned();
    let hr_agent_id = office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == org_office::organization_family::records::EmployeeKind::Agent)
        .unwrap()
        .id
        .as_str()
        .to_owned();
    let emp_c = "01a212a8-982e-7000-8000-000000000c03";
    // Create sibling directory on disk so `hr_agent_id`'s empty mask is stale.
    std::fs::create_dir_all(
        office
            .durable_root()
            .join("local")
            .join("offices")
            .join(&office.info().office_id)
            .join("agents")
            .join(emp_c),
    )
    .unwrap();
    let empty_mask = std::collections::BTreeSet::new();

    // 1. `start_session` hitting an Idle stale bwrap handle whose pump panics MUST persist
    // `recovery_required = true`, restore handle into `reg.live`, return `Conflict`, and keep
    // `emp_c` blocked until `clear_stall_lock` (`B4-3`).
    let panic_start =
        spawn_panicking_bwrap_handle(&office.info().office_id, &hr_agent_id, empty_mask.clone());
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), panic_start);
    let start_err = mgr
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: hr_agent_id.clone(),
                instruction: Some("trigger stale recycle".into()),
                workspace: None,
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
            },
        )
        .unwrap_err();
    assert_eq!(start_err.code, org_proto::ErrorCode::Conflict);
    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));
    assert!(
        mgr.store
            .read_active_turn(&hr_agent_id)
            .unwrap()
            .unwrap()
            .recovery_required
    );
    let sib_err = crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap_err();
    assert_eq!(sib_err.code, org_proto::ErrorCode::Conflict);
    mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap();

    // 2. `stop_session` hitting a panicking bwrap handle MUST fail closed and keep `emp_c` blocked,
    // while `stop_all` / `shutdown` MUST still drain cleanly to empty `reg.live` (`B4-1`).
    let panic_stop =
        spawn_panicking_bwrap_handle(&office.info().office_id, &hr_agent_id, empty_mask);
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), panic_stop);
    let stop_err = mgr.stop_session(&hr_agent_id, "user_stop").unwrap_err();
    assert_eq!(stop_err.code, org_proto::ErrorCode::Conflict);
    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));
    assert!(
        crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).is_err(),
        "sibling isolation must remain blocked after failed stop_session reap"
    );

    mgr.stop_all("office_restart");
    assert!(
        mgr.registry.lock().unwrap().live.is_empty(),
        "stop_all must converge even with RecoveryRequired handles present"
    );
    assert!(
        mgr.store
            .read_active_turn(&hr_agent_id)
            .unwrap()
            .unwrap()
            .recovery_required,
        "stop_all drain must preserve durable recovery_required=true"
    );
}

#[test]
fn task166_recycle_stale_warm_handle_fails_closed_and_updates_existing_active_turn() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = Arc::new(SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    ));
    let owner_id = office.owner_member_id().to_owned();
    let hr_agent_id = office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == org_office::organization_family::records::EmployeeKind::Agent)
        .unwrap()
        .id
        .as_str()
        .to_owned();
    let emp_c = "01a212a8-982e-7000-8000-000000000c03";
    std::fs::create_dir_all(
        office
            .durable_root()
            .join("local")
            .join("offices")
            .join(&office.info().office_id)
            .join("agents")
            .join(emp_c),
    )
    .unwrap();

    // Pre-seed `active-turn.v1.json` to prove `persist_recovery_required` preserves existing
    // metadata while flipping `active = false, recovery_required = true` (`B4-2`).
    mgr.store
        .write_active_turn(&ActiveTurnRecord {
            version: 1,
            employee_id: hr_agent_id.clone(),
            run_id: "run-existing".into(),
            session_id: "sess-existing".into(),
            runtime_session_id: Some("rt-existing".into()),
            mark: "m-existing".into(),
            pid: None,
            pgid: None,
            started_identity: None,
            boot_identity: None,
            objective: "work".into(),
            attachment_ids: Vec::new(),
            accepted_commands: Vec::new(),
            resume_attempts: 0,
            consecutive_stalls: 0,
            stall_locked: false,
            recovery_required: false,
            active: true,
            updated_at: "2026-11-26T00:00:00Z".into(),
        })
        .unwrap();

    let panic_recycle = spawn_panicking_bwrap_handle(
        &office.info().office_id,
        &hr_agent_id,
        std::collections::BTreeSet::new(),
    );
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), panic_recycle);

    assert!(crate::lifecycle::recycle_stale_warm_handle_in(
        &mgr,
        &hr_agent_id,
        "run-a",
        "sandbox_topology_changed",
    ));
    let bg_handles = std::mem::take(&mut mgr.registry.lock().unwrap().bg_workers);
    for h in bg_handles {
        let _ = h.join();
    }

    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));
    assert!(!mgr.registry.lock().unwrap().starting.contains(&hr_agent_id));
    let persisted = mgr.store.read_active_turn(&hr_agent_id).unwrap().unwrap();
    assert!(!persisted.active);
    assert!(persisted.recovery_required);
    assert_eq!(persisted.run_id, "run-existing");
    assert_eq!(persisted.mark, "m-existing");

    let sib_err = crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap_err();
    assert_eq!(sib_err.code, org_proto::ErrorCode::Conflict);
    mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap();
    crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap();
}

#[test]
fn task166_clear_stall_lock_rejects_starting_and_fails_closed_on_panicking_pump() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = Arc::new(SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    ));
    let owner_id = office.owner_member_id().to_owned();
    let hr_agent_id = office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == org_office::organization_family::records::EmployeeKind::Agent)
        .unwrap()
        .id
        .as_str()
        .to_owned();
    let emp_c = "01a212a8-982e-7000-8000-000000000c03";

    // 1. Concurrent `clear_stall_lock` while `reg.starting` holds `hr_agent_id` MUST reject with
    // `Conflict` without mutating persisted `recovery_required = true` (`B5-1`).
    mgr.store
        .persist_recovery_required(&hr_agent_id, Some("run-1"), Some("sess-1"), None)
        .unwrap();
    mgr.registry
        .lock()
        .unwrap()
        .starting
        .insert(hr_agent_id.clone());
    let starting_err = mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap_err();
    assert_eq!(starting_err.code, org_proto::ErrorCode::Conflict);
    assert!(
        mgr.store
            .read_active_turn(&hr_agent_id)
            .unwrap()
            .unwrap()
            .recovery_required,
        "rejected clear_stall_lock must not clear persisted recovery_required"
    );
    mgr.registry.lock().unwrap().starting.remove(&hr_agent_id);

    // 2. `clear_stall_lock` against a `StallLocked` handle whose pump thread panics MUST fail
    // closed: persist `recovery_required = true`, restore handle into `reg.live`, return `Conflict`,
    // and keep `emp_c` sibling isolation blocked until a subsequent clear (`B5-1`).
    let (tx, _rx) = std::sync::mpsc::channel::<crate::pump::PumpCommand>();
    let (mut panic_handle, _) = spawn_stub_bwrap_handle(
        &office.info().office_id,
        &hr_agent_id,
        SessionState::StallLocked,
        std::collections::BTreeSet::new(),
    );
    let _ = panic_handle.tx.send(crate::pump::PumpCommand::Stop {
        reason: "test_cleanup".into(),
    });
    if let Some(t) = panic_handle.thread.take() {
        let _ = t.join();
    }
    panic_handle.tx = tx;
    panic_handle.thread = Some(std::thread::spawn(|| {
        panic!("simulated StallLocked pump teardown panic");
    }));
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), panic_handle);

    let clear_err = mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap_err();
    assert_eq!(clear_err.code, org_proto::ErrorCode::Conflict);
    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));
    assert!(!mgr.registry.lock().unwrap().starting.contains(&hr_agent_id));
    assert!(
        mgr.store
            .read_active_turn(&hr_agent_id)
            .unwrap()
            .unwrap()
            .recovery_required
    );
    let sib_err = crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap_err();
    assert_eq!(sib_err.code, org_proto::ErrorCode::Conflict);

    // Second `clear_stall_lock` (now that `thread` is `None`) clears recovery cleanly.
    mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap();
    assert!(!mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));
    assert!(
        !mgr.store
            .read_active_turn(&hr_agent_id)
            .unwrap()
            .unwrap()
            .recovery_required
    );
    crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap();
}

#[test]
fn task166_unrecovered_pre_pump_abort_persists_and_blocks_sibling_isolation() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = Arc::new(SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    ));
    let owner_id = office.owner_member_id().to_owned();
    let hr_agent_id = office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.kind == org_office::organization_family::records::EmployeeKind::Agent)
        .unwrap()
        .id
        .as_str()
        .to_owned();
    let emp_c = "01a212a8-982e-7000-8000-000000000c03";
    let (handle, _) = spawn_stub_bwrap_handle(
        &office.info().office_id,
        &hr_agent_id,
        SessionState::Starting,
        std::collections::BTreeSet::new(),
    );
    let _ = handle.tx.send(crate::pump::PumpCommand::Stop {
        reason: "test_cleanup".into(),
    });
    if let Some(t) = handle.thread {
        let _ = t.join();
    }
    crate::manager::install_unrecovered_pre_pump_handle(
        &mgr,
        &hr_agent_id,
        crate::manager::PrePumpHandleMeta {
            tx: handle.tx,
            state: handle.state,
            connection_id: None,
            route_digest: String::new(),
            oauth_expires_at_ms: None,
            workspace: None,
            active_permit: handle.active_permit,
            supports_mid_turn: false,
            sandbox_plan: crate::manager::SandboxPlan::linux_bwrap(
                std::collections::BTreeSet::new(),
            ),
            partition: None,
            active_model: None,
            active_effort: None,
            pinned_override: Default::default(),
        },
    );

    assert!(mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id));
    assert!(
        mgr.store
            .read_active_turn(&hr_agent_id)
            .unwrap()
            .unwrap()
            .recovery_required,
        "pre-pump unverified teardown must upsert durable recovery_required=true"
    );
    assert_eq!(
        mgr.stop_session(&hr_agent_id, "bypass").unwrap_err().code,
        org_proto::ErrorCode::Conflict
    );
    assert_eq!(
        crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c)
            .unwrap_err()
            .code,
        org_proto::ErrorCode::Conflict
    );
    mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap();
    crate::manager::enforce_bwrap_sibling_isolation(&mgr, emp_c).unwrap();
}
