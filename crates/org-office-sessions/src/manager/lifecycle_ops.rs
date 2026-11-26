//! Stop, shutdown, and stall-lock clearing with fail-closed live-handle reaping.

use super::*;

impl SessionManager {
    /// Stops any active session for `employee_id`, reaps background tasks, and revokes its
    /// run binding immediately (called on pause, offboard, or explicit stop; `SUB-5`, `B2 Q9`).
    /// Refuses evicting handles already in `RecoveryRequired` and fails closed on unverified teardown.
    pub fn stop_session(&self, employee_id: &str, reason: &str) -> Result<SessionView, ApiError> {
        stop_session_impl(self, employee_id, reason)
    }

    /// Stops every live session cleanly while waiting for any in-flight starts and background
    /// workers to settle, then flushes all deferred session usage into office accounting.
    pub fn stop_all(&self, reason: &str) {
        stop_all_inner(self, reason, false);
    }

    /// Permanently closes admissions, stops every live session, joins in-flight starts and
    /// background workers, and flushes all deferred session usage before the office shuts down.
    pub fn shutdown(&self, reason: &str) {
        stop_all_inner(self, reason, true);
    }

    /// Owner clears a consecutive-stall or manual-recovery lock (`LIVE-12`).
    pub fn clear_stall_lock(
        &self,
        actor_member_id: &str,
        employee_id: &str,
    ) -> Result<SessionView, ApiError> {
        self.clear_stall_lock_inner(actor_member_id, false, employee_id)
    }

    pub(super) fn clear_stall_lock_inner(
        &self,
        actor_member_id: &str,
        hq_attested_owner: bool,
        employee_id: &str,
    ) -> Result<SessionView, ApiError> {
        clear_stall_lock_impl(self, actor_member_id, hq_attested_owner, employee_id)
    }
}

pub(super) fn take_clearable_live_handle(
    mgr: &SessionManager,
    employee_id: &str,
) -> Result<Option<LiveSessionHandle>, ApiError> {
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if reg.starting.contains(employee_id) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session start or teardown is already in progress",
        ));
    }
    let should_remove = reg.live.get(employee_id).is_some_and(|handle| {
        let mut guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        // A `Stopping` handle is a pump still tearing down (possibly mid-failover): it stays live
        // and the persisted `recovery_required` flag is kept below (Pass 6).
        let removable = guard.recovery_required
            || matches!(
                guard.state,
                SessionState::StallLocked
                    | SessionState::RecoveryRequired
                    | SessionState::Stopped
                    | SessionState::Suspended
            );
        guard.watchdog.clear_stall_lock(Instant::now());
        if removable && handle.thread.is_some() {
            // Prepared in the same critical section as the decision, so a failure the pump
            // records after this point is still seen by the reap (fail closed).
            guard.recovery_required = false;
            if guard.state == SessionState::RecoveryRequired {
                guard.state = SessionState::Stopping;
            }
        }
        removable
    });
    reg.starting.insert(employee_id.to_owned());
    let old = if should_remove {
        reg.live.remove(employee_id)
    } else {
        None
    };
    Ok(old)
}

/// Clears the persisted stall lock, and the persisted `recovery_required` flag only when
/// `clear_recovery` (no live handle remains whose teardown outcome is still unknown, Pass 6).
pub(super) fn clear_persisted_recovery_state(
    mgr: &SessionManager,
    employee_id: &str,
    clear_recovery: bool,
) {
    let _ = mgr.store.clear_pending_start(employee_id);
    let _ = mgr.store.update_active_turn(employee_id, |persisted| {
        persisted.consecutive_stalls = 0;
        persisted.stall_locked = false;
        if clear_recovery {
            persisted.recovery_required = false;
        }
        persisted.updated_at = rfc3339_now();
    });
    mgr.last_states
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(employee_id);
}

pub(super) fn clear_stall_lock_impl(
    mgr: &SessionManager,
    actor_member_id: &str,
    hq_attested_owner: bool,
    employee_id: &str,
) -> Result<SessionView, ApiError> {
    mgr.authorize_owner_only_inner(actor_member_id, hq_attested_owner)?;
    let _ = mgr.require_agent(employee_id)?;
    let dead_handle = take_clearable_live_handle(mgr, employee_id)?;
    let slot_guard = StartSlotGuard::new(&mgr.registry, employee_id.to_owned());
    if let Some(old) = dead_handle {
        reap_cleared_handle(mgr, employee_id, old)?;
    }
    // The start slot is held, so nothing can install a new handle in between.
    let live_handle_remains = mgr
        .registry
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .live
        .contains_key(employee_id);
    clear_persisted_recovery_state(mgr, employee_id, !live_handle_remains);
    drop(slot_guard);
    let _ = mgr.process_due_wakes_for(employee_id);
    let live_view = {
        let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.live.get(employee_id).map(|handle| {
            handle
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .to_view()
        })
    };
    if let Some(mut view) = live_view {
        overlay_durable_wake_state(&mgr.store, employee_id, &mut view);
        // A kept persisted `recovery_required` flag (a live handle still tearing down) is
        // reported, so the caller sees why the next start will refuse.
        if mgr
            .store
            .read_active_turn(employee_id)
            .ok()
            .flatten()
            .is_some_and(|rec| rec.recovery_required)
        {
            view.recovery_required = true;
        }
        return Ok(view);
    }
    mgr.idle_view_for(employee_id)
}

pub(super) fn reap_cleared_handle(
    mgr: &SessionManager,
    employee_id: &str,
    old: LiveSessionHandle,
) -> Result<(), ApiError> {
    if old.thread.is_some() {
        crate::bwrap_isolation::reap_live_handle(
            mgr,
            employee_id,
            old,
            None,
            crate::bwrap_isolation::ReapMode::FailClosedRestore {
                conflict_message: format!(
                    "could not verify teardown of session `{employee_id}` during recovery clear; resolve recovery first"
                ),
            },
        )?;
        return Ok(());
    }
    mgr.bindings.revoke(employee_id, None);
    if let Some((pid, _)) = old
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        let _ = mgr.office.mark_turn_permit_release_pending(&pid);
    }
    Ok(())
}

pub(super) fn stop_session_impl(
    mgr: &SessionManager,
    employee_id: &str,
    reason: &str,
) -> Result<SessionView, ApiError> {
    let removed = {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(handle) = reg.live.get(employee_id) {
            let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Err(err) = ensure_live_handle_not_recovery_required(&st) {
                drop(st);
                drop(reg);
                mgr.bindings.revoke(employee_id, None);
                return Err(err);
            }
            if reg.starting.contains(employee_id) {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session start or teardown is already in progress",
                ));
            }
            drop(st);
            let handle = reg.live.remove(employee_id);
            if handle.is_some() {
                reg.starting.insert(employee_id.to_owned());
            }
            handle
        } else {
            None
        }
    };
    let Some(handle) = removed else {
        mgr.bindings.revoke(employee_id, None);
        return mgr.idle_view_for(employee_id);
    };
    let slot_guard = StartSlotGuard::new(&mgr.registry, employee_id.to_owned());
    let reaped = crate::bwrap_isolation::reap_live_handle(
        mgr,
        employee_id,
        handle,
        Some(reason),
        crate::bwrap_isolation::ReapMode::FailClosedRestore {
            conflict_message: format!(
                "could not verify teardown of session `{employee_id}`; resolve recovery first"
            ),
        },
    )?;
    drop(slot_guard);
    if reaped.permit_released && !is_admissions_closed(mgr) {
        mgr.process_due_wakes();
    }
    Ok(reaped.view)
}

pub(super) fn reap_replaced_handle(
    mgr: &SessionManager,
    emp_id: &str,
    old: Option<LiveSessionHandle>,
) -> Result<(), ApiError> {
    let Some(handle) = old else {
        return Ok(());
    };
    crate::bwrap_isolation::reap_live_handle(
        mgr,
        emp_id,
        handle,
        None,
        crate::bwrap_isolation::ReapMode::FailClosedRestore {
            conflict_message: format!(
                "could not verify teardown of prior session `{emp_id}`; resolve recovery first"
            ),
        },
    )?;
    Ok(())
}

pub(super) fn stop_session_for_shutdown(mgr: &SessionManager, employee_id: &str, reason: &str) {
    let removed = {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.starting.contains(employee_id) {
            return;
        }
        let handle = reg.live.remove(employee_id);
        if handle.is_some() {
            reg.starting.insert(employee_id.to_owned());
        }
        handle
    };
    if let Some(handle) = removed {
        let _slot_guard = StartSlotGuard::new(&mgr.registry, employee_id.to_owned());
        let _ = crate::bwrap_isolation::reap_live_handle(
            mgr,
            employee_id,
            handle,
            Some(reason),
            crate::bwrap_isolation::ReapMode::ShutdownDrain,
        );
    }
}

pub(super) fn stop_all_inner(mgr: &SessionManager, reason: &str, permanent: bool) {
    {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.draining = true;
        if permanent {
            reg.shutting_down = true;
        }
    }
    loop {
        let ids: Vec<String> = {
            let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.keys().cloned().collect()
        };
        for id in ids {
            stop_session_for_shutdown(mgr, &id, reason);
        }
        let bg_handles: Vec<JoinHandle<()>> = {
            let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
            std::mem::take(&mut reg.bg_workers)
        };
        for handle in bg_handles {
            let _ = handle.join();
        }
        let settled = {
            let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.is_empty()
                && reg.starting.is_empty()
                && reg.bg_spawning == 0
                && reg.bg_workers.is_empty()
        };
        if settled {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let _ = mgr.flush_all_pending_usage();
    if !permanent {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.draining = false;
    }
}

pub(super) fn check_registry_startable(
    reg: &SessionRegistry,
    emp_id: &str,
) -> Result<(), ApiError> {
    if reg.draining || reg.shutting_down {
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "office session manager is shutting down",
        ));
    }
    if reg.starting.contains(emp_id) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session start is already in progress",
        ));
    }
    Ok(())
}
