//! Pass 6 fail-closed hardening: clearing a stall lock never wipes the persisted
//! `recovery_required` flag while a live handle (such as a `Stopping` pump) remains, and an
//! unrecovered pre-pump handle is never registered once admissions close.

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
