//! Restart recovery, resume caps, stall locks, and concurrent active-turn writers
//! (`LIVE-9..13`).

use super::*;

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
