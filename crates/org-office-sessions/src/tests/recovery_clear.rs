//! Fail-closed recovery clear: clearing a stall lock never wipes the persisted
//! `recovery_required` flag or `pending-start.v1.json` while a live handle (such as a `Stopping`
//! pump) remains, never holds the start slot against that pump's own failover, never drops a
//! failover turn that loses its relaunch to a concurrent clear, and an unrecovered pre-pump handle
//! is never registered once admissions close.

use super::*;

fn manager_with_hr_agent() -> (Arc<Office>, Arc<SessionManager>, String, String) {
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
    (office, mgr, owner_id, hr_agent_id)
}

#[test]
fn pass6_clear_stall_lock_keeps_recovery_required_while_a_stopping_handle_stays_live() {
    use std::sync::atomic::Ordering;

    let (office, mgr, owner_id, hr_agent_id) = manager_with_hr_agent();
    mgr.store
        .persist_recovery_required(&hr_agent_id, Some("run-a"), Some("sess-a"), None)
        .unwrap();
    mgr.store
        .update_active_turn(&hr_agent_id, |rec| rec.stall_locked = true)
        .unwrap();
    mgr.store
        .write_pending_start(&crate::recovery::PendingStartRecord {
            version: 1,
            employee_id: hr_agent_id.clone(),
            run_id: "run-a".into(),
            session_id: "sess-a".into(),
            mark: "mark-a".into(),
            started_at: crate::credentials::rfc3339_now(),
        })
        .unwrap();
    let pending_start_path = mgr.store.pending_start_path(&hr_agent_id).unwrap();
    // A pump still tearing down (it may be mid-failover): its outcome is unknown, so the clear
    // neither joins it nor wipes the persisted `recovery_required` flag.
    let (handle, stopped) = spawn_stub_bwrap_handle(
        &office.info().office_id,
        &hr_agent_id,
        SessionState::Stopping,
        std::collections::BTreeSet::new(),
    );
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), handle);

    let view = mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap();
    assert!(
        view.recovery_required,
        "the kept persisted flag is reported to the caller"
    );

    assert!(
        mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id),
        "a Stopping handle is left to finish its own teardown"
    );
    assert!(
        !stopped.load(Ordering::SeqCst),
        "the clear never stops or joins it"
    );
    let persisted = mgr.store.read_active_turn(&hr_agent_id).unwrap().unwrap();
    assert!(!persisted.stall_locked, "the stall lock itself is cleared");
    assert!(
        persisted.recovery_required,
        "recovery_required is kept while a live handle remains (fail closed)"
    );
    assert!(!mgr.registry.lock().unwrap().starting.contains(&hr_agent_id));
    assert!(
        pending_start_path.is_file(),
        "pending-start.v1.json launch evidence survives while a live handle remains"
    );
    assert_eq!(
        mgr.store
            .read_pending_start(&hr_agent_id)
            .unwrap()
            .map(|rec| rec.run_id),
        Some("run-a".to_owned())
    );

    // Once the teardown has finished and the handle is gone, a clear removes the flag.
    let gone = mgr
        .registry
        .lock()
        .unwrap()
        .live
        .remove(&hr_agent_id)
        .unwrap();
    let _ = gone.tx.send(crate::pump::PumpCommand::Stop {
        reason: "teardown_finished".into(),
    });
    if let Some(thread) = gone.thread {
        thread.join().unwrap();
    }
    mgr.clear_stall_lock(&owner_id, &hr_agent_id).unwrap();
    assert!(
        !mgr.store
            .read_active_turn(&hr_agent_id)
            .unwrap()
            .unwrap()
            .recovery_required
    );
    assert!(
        mgr.store
            .read_pending_start(&hr_agent_id)
            .unwrap()
            .is_none(),
        "with no live handle left, the clear removes the pending-start evidence too"
    );
}

/// A stub pump whose thread exits only when `gate` fires, so a reap that joins it blocks
/// deterministically until the test opens the gate.
fn gated_stub_handle(
    office_id: &str,
    emp_id: &str,
    state: SessionState,
) -> (
    crate::manager::LiveSessionHandle,
    std::sync::mpsc::Sender<()>,
) {
    let (mut handle, _) =
        spawn_stub_bwrap_handle(office_id, emp_id, state, std::collections::BTreeSet::new());
    let _ = handle.tx.send(crate::pump::PumpCommand::Stop {
        reason: "swap_in_gated_thread".into(),
    });
    if let Some(thread) = handle.thread.take() {
        thread.join().unwrap();
    }
    let (gate, gate_rx) = std::sync::mpsc::channel::<()>();
    handle.thread = Some(std::thread::spawn(move || {
        let _ = gate_rx.recv();
    }));
    (handle, gate)
}

fn failover_request(
    emp_id: &str,
    reason: &'static str,
    stop_epoch: u64,
) -> crate::pump::FailoverRequest {
    crate::pump::FailoverRequest {
        employee_id: emp_id.into(),
        failed_run_id: "run-a".into(),
        failed_connection_id: format!("conn-{reason}"),
        objective: format!("keep-{reason}-turn"),
        workspace: None,
        task_ref: None,
        tried_connections: vec![format!("conn-{reason}")],
        oauth_refresh_attempted: false,
        oauth_token_fp: None,
        reason,
        rate_limit_resets_at: None,
        stop_epoch,
    }
}

fn queued_recovery_summaries(mgr: &SessionManager, emp_id: &str) -> Vec<String> {
    mgr.store
        .read_pending_wakes(emp_id)
        .unwrap()
        .map(|queue| {
            queue
                .items
                .into_iter()
                .filter(|item| item.kind == crate::wake::WakeKind::Recovery)
                .map(|item| item.summary)
                .collect()
        })
        .unwrap_or_default()
}

fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn pass1_clear_stall_lock_racing_a_failover_pump_never_drops_rate_limited_or_auth_expired_turns() {
    let (office, mgr, owner_id, hr_agent_id) = manager_with_hr_agent();
    // Keep any requeued wake durable instead of launching a real runtime from the clear's wake
    // pass (`process_due_wakes_for` holds every dispatch while a budget hold is active).
    mgr.set_budget_hold(Some("test: keep requeued failover wakes durable".into()));
    // The failed pump already ran `finalize_child` (state `Stopped`) and handed its turn to an
    // `org-pool-failover` worker that has not yet claimed the start slot (Window B).
    let (handle, gate) = gated_stub_handle(
        &office.info().office_id,
        &hr_agent_id,
        SessionState::Stopped,
    );
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), handle);
    let epoch = mgr.registry.lock().unwrap().stop_epoch(&hr_agent_id);

    let clear_mgr = Arc::clone(&mgr);
    let clear_owner = owner_id.clone();
    let clear_emp = hr_agent_id.clone();
    let clearing = std::thread::spawn(move || clear_mgr.clear_stall_lock(&clear_owner, &clear_emp));
    // The clear removed the `Stopped` handle and holds the start slot while its reap joins the
    // (gated) pump thread.
    wait_for("the clear to hold the start slot mid-reap", || {
        let reg = mgr.registry.lock().unwrap();
        reg.starting.contains(&hr_agent_id) && !reg.live.contains_key(&hr_agent_id)
    });

    for reason in ["rate_limited", "auth_expired"] {
        crate::manager::run_failover_relaunch(&mgr, failover_request(&hr_agent_id, reason, epoch));
        assert!(
            queued_recovery_summaries(&mgr, &hr_agent_id).contains(&format!("keep-{reason}-turn")),
            "{reason}: a failover that loses its relaunch to the clear is requeued durably"
        );
    }
    assert!(
        mgr.registry.lock().unwrap().starting.contains(&hr_agent_id),
        "the failover never took the slot the clear still holds"
    );

    gate.send(()).unwrap();
    clearing.join().unwrap().unwrap();
    let reg = mgr.registry.lock().unwrap();
    assert!(!reg.starting.contains(&hr_agent_id));
    assert!(!reg.live.contains_key(&hr_agent_id));
    drop(reg);
    let queued = queued_recovery_summaries(&mgr, &hr_agent_id);
    for reason in ["rate_limited", "auth_expired"] {
        assert!(
            queued.contains(&format!("keep-{reason}-turn")),
            "{reason}: the requeued turn is still pending after the clear: {queued:?}"
        );
    }
}

#[test]
fn pass1_clear_stall_lock_reserves_no_start_slot_for_a_stopping_handle() {
    use crate::manager::{take_clearable_live_handle, ClearableHandle};

    let (office, mgr, _owner_id, hr_agent_id) = manager_with_hr_agent();
    let office_id = office.info().office_id.clone();

    // Window A: a `Stopping` pump (possibly about to fail over) stays live and its failover can
    // still claim the slot, because the clear reserved none.
    let (stopping, stopping_gate) =
        gated_stub_handle(&office_id, &hr_agent_id, SessionState::Stopping);
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), stopping);
    assert!(matches!(
        take_clearable_live_handle(&mgr, &hr_agent_id).unwrap(),
        ClearableHandle::Kept
    ));
    {
        let reg = mgr.registry.lock().unwrap();
        assert!(
            !reg.starting.contains(&hr_agent_id),
            "no slot for a kept handle"
        );
        assert!(reg.live.contains_key(&hr_agent_id));
    }
    let kept = mgr
        .registry
        .lock()
        .unwrap()
        .live
        .remove(&hr_agent_id)
        .unwrap();
    stopping_gate.send(()).unwrap();
    kept.thread.unwrap().join().unwrap();

    // A dead (`Stopped`) handle is removed with the slot reserved for its reap.
    let (stopped, stopped_gate) =
        gated_stub_handle(&office_id, &hr_agent_id, SessionState::Stopped);
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(hr_agent_id.clone(), stopped);
    let ClearableHandle::Removed(removed) = take_clearable_live_handle(&mgr, &hr_agent_id).unwrap()
    else {
        panic!("a Stopped handle is removed for reaping");
    };
    assert!(mgr.registry.lock().unwrap().starting.remove(&hr_agent_id));
    stopped_gate.send(()).unwrap();
    removed.thread.unwrap().join().unwrap();

    // No handle at all: the slot is reserved so no start races the persisted-state clear.
    assert!(matches!(
        take_clearable_live_handle(&mgr, &hr_agent_id).unwrap(),
        ClearableHandle::Absent
    ));
    assert!(mgr.registry.lock().unwrap().starting.remove(&hr_agent_id));
}

#[test]
fn pass1_failover_after_explicit_stop_or_during_shutdown_is_abandoned_not_requeued() {
    let (_office, mgr, _owner_id, hr_agent_id) = manager_with_hr_agent();
    mgr.set_budget_hold(Some("test: never launch".into()));
    let epoch = mgr.registry.lock().unwrap().stop_epoch(&hr_agent_id);

    // Positive control: contention alone requeues the turn.
    mgr.registry
        .lock()
        .unwrap()
        .starting
        .insert(hr_agent_id.clone());
    crate::manager::run_failover_relaunch(
        &mgr,
        failover_request(&hr_agent_id, "quota_exhausted", epoch),
    );
    assert!(queued_recovery_summaries(&mgr, &hr_agent_id)
        .contains(&"keep-quota_exhausted-turn".to_owned()));
    mgr.registry.lock().unwrap().starting.remove(&hr_agent_id);

    // An explicit stop accepted after the run launched (here with the handle already gone, as
    // when a concurrent clear reaped it) abandons the failover even while the slot is contended.
    mgr.stop_session(&hr_agent_id, "owner_stop").unwrap();
    assert_eq!(
        mgr.registry.lock().unwrap().stop_epoch(&hr_agent_id),
        epoch + 1
    );
    mgr.registry
        .lock()
        .unwrap()
        .starting
        .insert(hr_agent_id.clone());
    crate::manager::run_failover_relaunch(
        &mgr,
        failover_request(&hr_agent_id, "model_unavailable", epoch),
    );
    assert!(!queued_recovery_summaries(&mgr, &hr_agent_id)
        .contains(&"keep-model_unavailable-turn".to_owned()));
    mgr.registry.lock().unwrap().starting.remove(&hr_agent_id);

    // A drain (stop_all / shutdown) abandons it too.
    mgr.registry.lock().unwrap().draining = true;
    crate::manager::run_failover_relaunch(
        &mgr,
        failover_request(&hr_agent_id, "rate_limited", epoch + 1),
    );
    mgr.registry.lock().unwrap().draining = false;
    assert!(!queued_recovery_summaries(&mgr, &hr_agent_id)
        .contains(&"keep-rate_limited-turn".to_owned()));
}

#[test]
fn pass6_unrecovered_pre_pump_handle_is_persisted_but_never_registered_while_draining() {
    let (office, mgr, _owner_id, hr_agent_id) = manager_with_hr_agent();
    for closing in ["draining", "shutting_down"] {
        {
            let mut reg = mgr.registry.lock().unwrap();
            reg.draining = closing == "draining";
            reg.shutting_down = closing == "shutting_down";
        }
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
                bwrap_masked_siblings: Some(std::collections::BTreeSet::new()),
            },
        );
        assert!(
            !mgr.registry.lock().unwrap().live.contains_key(&hr_agent_id),
            "{closing}: no handle is registered once admissions close"
        );
        assert!(
            mgr.store
                .read_active_turn(&hr_agent_id)
                .unwrap()
                .unwrap()
                .recovery_required,
            "{closing}: recovery_required is still persisted (fail closed)"
        );
    }
}

#[test]
fn pass1_contended_failover_of_an_offboarded_agent_is_not_requeued() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = Arc::new(SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    ));
    mgr.set_budget_hold(Some("test: never launch".into()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (agent_id, _conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "google",
        "agy",
        "retiree",
        "gemini-2.5-pro",
    );
    let epoch = mgr.registry.lock().unwrap().stop_epoch(&agent_id);
    mgr.registry
        .lock()
        .unwrap()
        .starting
        .insert(agent_id.clone());

    // Positive control: while the agent is active, a contended failover is requeued.
    crate::manager::run_failover_relaunch(&mgr, failover_request(&agent_id, "rate_limited", epoch));
    assert!(
        queued_recovery_summaries(&mgr, &agent_id).contains(&"keep-rate_limited-turn".to_owned())
    );

    // Offboarded at the office without this manager's `stop_session` (no stop epoch bump): the
    // record alone shows the agent will never run here again, so nothing is queued for it.
    let offboard = org_proto::Request::new::<org_proto::hiring::AgentOffboard>(
        3,
        &org_proto::hiring::AgentOffboardParams {
            command_id: new_uuid_v7(),
            member_id: agent_id.clone(),
            expected_version: 1,
        },
    )
    .unwrap();
    let resp = office
        .dispatch_hiring_rpc(Some(&owner_id), &offboard)
        .unwrap();
    assert!(
        matches!(resp.outcome, org_proto::Outcome::Result(_)),
        "offboard failed: {:?}",
        resp.outcome
    );
    crate::manager::run_failover_relaunch(&mgr, failover_request(&agent_id, "auth_expired", epoch));
    assert!(
        !queued_recovery_summaries(&mgr, &agent_id).contains(&"keep-auth_expired-turn".to_owned())
    );
    mgr.registry.lock().unwrap().starting.remove(&agent_id);
}

#[test]
fn pass2_stop_session_conflicting_with_an_in_flight_start_moves_the_epoch_without_revoking_binding()
{
    let (_office, mgr, _owner_id, hr_agent_id) = manager_with_hr_agent();
    mgr.bindings
        .issue(&hr_agent_id, "run-in-flight", "sess-in-flight", 0)
        .unwrap();
    let tokens_before = mgr.bindings.active_tokens();
    assert_eq!(tokens_before.len(), 1);
    let epoch_before = {
        let mut reg = mgr.registry.lock().unwrap();
        assert!(!reg.live.contains_key(&hr_agent_id));
        reg.starting.insert(hr_agent_id.clone());
        reg.stop_epoch(&hr_agent_id)
    };

    let err = mgr
        .stop_session(&hr_agent_id, "owner_stop")
        .expect_err("stop_session must conflict while reg.starting is held with no live handle");
    assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    assert_eq!(
        mgr.registry.lock().unwrap().stop_epoch(&hr_agent_id),
        epoch_before + 1,
        "a stop refused by an in-flight start still moves stop_epoch, so that start aborts at \
         registration instead of leaving a live session (TASK-190)"
    );
    assert_eq!(
        mgr.bindings.active_tokens(),
        tokens_before,
        "conflicted stop_session must not revoke the in-flight launch binding"
    );

    mgr.registry.lock().unwrap().starting.remove(&hr_agent_id);
    mgr.stop_session(&hr_agent_id, "owner_stop")
        .expect("stop_session succeeds once start slot releases");
    assert_eq!(
        mgr.registry.lock().unwrap().stop_epoch(&hr_agent_id),
        epoch_before + 2
    );
    assert!(
        mgr.bindings.active_tokens().is_empty(),
        "accepted stop_session revokes the binding"
    );
}

#[test]
fn pass2_requeue_contended_failover_abandons_after_explicit_stop_epoch_bump() {
    let (_office, mgr, _owner_id, hr_agent_id) = manager_with_hr_agent();
    mgr.set_budget_hold(Some("test: never launch".into()));
    let epoch = mgr.registry.lock().unwrap().stop_epoch(&hr_agent_id);
    mgr.stop_session(&hr_agent_id, "owner_stop").unwrap();

    crate::manager::requeue_contended_failover(
        &mgr,
        &failover_request(&hr_agent_id, "rate_limited", epoch),
    );
    assert!(
        queued_recovery_summaries(&mgr, &hr_agent_id).is_empty(),
        "explicit stop between contention and requeue must abandon the failover wake"
    );
}
