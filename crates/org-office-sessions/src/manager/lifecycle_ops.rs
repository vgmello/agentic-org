//! Stop, shutdown, and stall-lock clearing with fail-closed live-handle reaping.

use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_proto::sessions::{SessionState, SessionView};
use org_proto::{ApiError, ErrorCode};

use crate::credentials::rfc3339_now;

use super::registry_guards::ensure_live_handle_not_recovery_required;
use super::views::overlay_durable_wake_state;
use super::{is_admissions_closed, LiveSessionHandle, SessionManager, StartSlotGuard};

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

/// What a recovery clear found for the agent's live handle.
pub(crate) enum ClearableHandle {
    /// A dead or locked handle (`StallLocked`, `RecoveryRequired`, `Stopped`, `Suspended`, or
    /// flagged `recovery_required`) was removed; the start slot is reserved for its reap.
    Removed(Box<LiveSessionHandle>),
    /// A handle that is still running or tearing down (such as a `Stopping` pump that may be
    /// mid-failover) stays live. No start slot is reserved, so its own failover relaunch is never
    /// refused for contention, and its teardown outcome is unknown, so durable recovery evidence
    /// is kept.
    Kept,
    /// No live handle exists; the start slot is reserved so no start can begin (and persist fresh
    /// recovery evidence) while the persisted lock is cleared.
    Absent,
}

/// Classifies the live handle for a recovery clear, clearing its in-memory stall lock. Reserves
/// the start slot (in the same `registry` critical section) only for [`ClearableHandle::Removed`]
/// and [`ClearableHandle::Absent`]; the caller owns releasing it.
pub(crate) fn take_clearable_live_handle(
    mgr: &SessionManager,
    employee_id: &str,
) -> Result<ClearableHandle, ApiError> {
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if reg.starting.contains(employee_id) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session start or teardown is already in progress",
        ));
    }
    let Some(handle) = reg.live.get(employee_id) else {
        reg.starting.insert(employee_id.to_owned());
        return Ok(ClearableHandle::Absent);
    };
    let should_remove = {
        let mut guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
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
    };
    if !should_remove {
        return Ok(ClearableHandle::Kept);
    }
    reg.starting.insert(employee_id.to_owned());
    Ok(reg
        .live
        .remove(employee_id)
        .map_or(ClearableHandle::Absent, |handle| {
            ClearableHandle::Removed(Box::new(handle))
        }))
}

/// Clears the persisted stall lock, and only when `clear_recovery` (no live handle remains whose
/// teardown outcome is still unknown, Pass 6) also the persisted `recovery_required` flag and the
/// `pending-start.v1.json` launch evidence startup recovery would need.
pub(super) fn clear_persisted_recovery_state(
    mgr: &SessionManager,
    employee_id: &str,
    clear_recovery: bool,
) {
    if clear_recovery {
        let _ = mgr.store.clear_pending_start(employee_id);
    }
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
    match take_clearable_live_handle(mgr, employee_id)? {
        ClearableHandle::Kept => clear_persisted_recovery_state(mgr, employee_id, false),
        found => {
            let slot_guard = StartSlotGuard::new(&mgr.registry, employee_id.to_owned());
            if let ClearableHandle::Removed(old) = found {
                reap_cleared_handle(mgr, employee_id, *old)?;
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
        }
    }
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
        super::reap::reap_live_handle(
            mgr,
            employee_id,
            old,
            None,
            super::reap::ReapMode::FailClosedRestore {
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
        let start_in_flight = reg.starting.contains(employee_id);
        if start_in_flight {
            // The stop still wins over an in-flight start, warm send or failover relaunch: the
            // epoch moves before refusing, so that launch aborts at registration (and drops its
            // failover turn) instead of leaving a live session behind (`TASK-190`).
            reg.record_explicit_stop(employee_id);
        }
        if let Some(handle) = reg.live.get(employee_id) {
            let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Err(err) = ensure_live_handle_not_recovery_required(&st) {
                drop(st);
                drop(reg);
                mgr.bindings.revoke(employee_id, None);
                return Err(err);
            }
            if start_in_flight {
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
            // Accepted: a pool failover of any run installed before now is abandoned, even one
            // whose handle a concurrent recovery clear already reaped.
            reg.record_explicit_stop(employee_id);
            handle
        } else {
            if start_in_flight {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session start or teardown is already in progress",
                ));
            }
            reg.record_explicit_stop(employee_id);
            None
        }
    };
    let Some(handle) = removed else {
        mgr.bindings.revoke(employee_id, None);
        return mgr.idle_view_for(employee_id);
    };
    let slot_guard = StartSlotGuard::new(&mgr.registry, employee_id.to_owned());
    let reaped = super::reap::reap_live_handle(
        mgr,
        employee_id,
        handle,
        Some(reason),
        super::reap::ReapMode::FailClosedRestore {
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

impl SessionManager {
    /// Clears `employee_id`'s stored conversation state for `partition` and stops the live
    /// session ONLY when that live handle is bound to `partition`, checked atomically under
    /// `self.registry` (zero cross-caller TOCTOU for Ask Tess `tess.session_reset`). Never
    /// disturbs another caller's running or starting session or `tess:wake`.
    pub fn reset_partition_session(
        &self,
        employee_id: &str,
        partition: &crate::partition::PartitionKey,
        reason: &str,
    ) -> Result<bool, ApiError> {
        self.partitions.clear_partition(employee_id, partition);
        let removed = {
            let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            let matches_partition = reg
                .live
                .get(employee_id)
                .is_some_and(|h| h.partition.as_ref() == Some(partition));
            if !matches_partition || reg.starting.contains(employee_id) {
                None
            } else {
                let handle = reg.live.remove(employee_id);
                if handle.is_some() {
                    reg.starting.insert(employee_id.to_owned());
                    reg.record_explicit_stop(employee_id);
                }
                handle
            }
        };
        let Some(handle) = removed else {
            return Ok(false);
        };
        let slot_guard = StartSlotGuard::new(&self.registry, employee_id.to_owned());
        let reaped = super::reap::reap_live_handle(
            self,
            employee_id,
            handle,
            Some(reason),
            super::reap::ReapMode::FailClosedRestore {
                conflict_message: format!(
                    "could not verify teardown of session `{employee_id}`; resolve recovery first"
                ),
            },
        )?;
        drop(slot_guard);
        if reaped.permit_released && !is_admissions_closed(self) {
            self.process_due_wakes();
        }
        Ok(true)
    }
}

pub(super) fn reap_replaced_handle(
    mgr: &SessionManager,
    emp_id: &str,
    old: Option<LiveSessionHandle>,
) -> Result<(), ApiError> {
    let Some(handle) = old else {
        return Ok(());
    };
    super::reap::reap_live_handle(
        mgr,
        emp_id,
        handle,
        None,
        super::reap::ReapMode::FailClosedRestore {
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
        let _ = super::reap::reap_live_handle(
            mgr,
            employee_id,
            handle,
            Some(reason),
            super::reap::ReapMode::ShutdownDrain,
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
