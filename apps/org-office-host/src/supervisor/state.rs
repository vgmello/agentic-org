//! An office's status record (what `office.list` and `office.status_changed` show), shared by
//! its supervision thread, which changes it, and the supervisor, which reads it. Every change is
//! announced to the [`StatusObserver`], after the change.

use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::SystemTime;

use org_proto::{OfficeStatus, OfficeStatusReason};

use crate::sync::lock;

/// One office's status, with why it's `offline` or `unavailable` and when it restarts next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OfficeState {
    pub(crate) status: OfficeStatus,
    /// Set only with `offline` and `unavailable`.
    pub(crate) reason: Option<OfficeStatusReason>,
    /// Set only while `offline` with a restart pending.
    pub(crate) next_restart_at: Option<SystemTime>,
}

impl OfficeState {
    pub(crate) fn starting() -> Self {
        Self::plain(OfficeStatus::Starting)
    }

    pub(crate) fn active() -> Self {
        Self::plain(OfficeStatus::Active)
    }

    /// Exited for `reason`; restarts at `next_restart_at`.
    pub(crate) fn offline(reason: OfficeStatusReason, next_restart_at: SystemTime) -> Self {
        Self {
            status: OfficeStatus::Offline,
            reason: Some(reason),
            next_restart_at: Some(next_restart_at),
        }
    }

    /// Won't be started again on its own, because of `reason`.
    pub(crate) fn unavailable(reason: OfficeStatusReason) -> Self {
        Self {
            status: OfficeStatus::Unavailable,
            reason: Some(reason),
            next_restart_at: None,
        }
    }

    fn plain(status: OfficeStatus) -> Self {
        Self {
            status,
            reason: None,
            next_restart_at: None,
        }
    }
}

/// What [`SharedState::request_restart`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartRequest {
    Sent,
    /// It's `starting` or `active` (or another status that isn't down).
    Running,
    /// `unavailable` for a reason a restart can't fix.
    CantFix(OfficeStatusReason),
}

/// Told about every change of an office's state, after the change. The host's offices implement
/// it to announce `office.status_changed`; the supervisor owns this abstraction so it never
/// depends on them.
pub(crate) trait StatusObserver: Send + Sync {
    fn status_changed(&self, office_id: &str);
}

/// Where changes are announced: set once, after the supervisor is built.
pub(crate) type Observer = Arc<OnceLock<Weak<dyn StatusObserver>>>;

/// An office's state, as its supervision thread and the supervisor share it.
pub(crate) struct SharedState {
    office_id: String,
    state: Mutex<OfficeState>,
    observer: Observer,
}

impl SharedState {
    pub(crate) fn new(office_id: &str, initial: OfficeState, observer: Observer) -> Self {
        Self {
            office_id: office_id.to_owned(),
            state: Mutex::new(initial),
            observer,
        }
    }

    pub(crate) fn get(&self) -> OfficeState {
        lock(&self.state).clone()
    }

    /// Changes the state; if it differs, announces it afterwards (with no lock held).
    pub(crate) fn set(&self, new: OfficeState) {
        let changed = {
            let mut state = lock(&self.state);
            let changed = *state != new;
            *state = new;
            changed
        };
        if changed {
            self.announce();
        }
    }

    /// `office.restart`: if the office is `offline`, or `unavailable` for a reason a restart can
    /// fix, runs `send` (which asks its supervision thread to restart it). The decision and the
    /// send happen under the state's lock, and the thread marks the office `starting` (under the
    /// same lock) before it takes queued restart requests: so a request is either taken before
    /// that run or refused as `Running`, never left to reset the counts of a later run.
    pub(crate) fn request_restart(&self, send: impl FnOnce()) -> RestartRequest {
        let state = lock(&self.state);
        match (state.status, state.reason) {
            (
                OfficeStatus::Unavailable,
                Some(
                    reason @ (OfficeStatusReason::UnsupportedVersion
                    | OfficeStatusReason::InvalidRequest),
                ),
            ) => RestartRequest::CantFix(reason),
            (OfficeStatus::Offline | OfficeStatus::Unavailable, _) => {
                send();
                RestartRequest::Sent
            }
            _ => RestartRequest::Running,
        }
    }

    /// Announces the current state (e.g. once the office is in the catalog).
    pub(crate) fn announce(&self) {
        if let Some(observer) = self.observer.get().and_then(Weak::upgrade) {
            observer.status_changed(&self.office_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    #[test]
    fn a_restart_is_requested_only_while_the_office_is_down() {
        let state = SharedState::new("A", OfficeState::starting(), Arc::new(OnceLock::new()));
        let sent = Cell::new(0);
        let send = || sent.set(sent.get() + 1);
        assert_eq!(state.request_restart(send), RestartRequest::Running);
        state.set(OfficeState::active());
        assert_eq!(state.request_restart(send), RestartRequest::Running);
        state.set(OfficeState::offline(
            OfficeStatusReason::Crashed,
            SystemTime::now() + Duration::from_secs(1),
        ));
        assert_eq!(state.request_restart(send), RestartRequest::Sent);
        state.set(OfficeState::unavailable(OfficeStatusReason::KeyMissing));
        assert_eq!(state.request_restart(send), RestartRequest::Sent);
        state.set(OfficeState::unavailable(
            OfficeStatusReason::RestartsExhausted,
        ));
        assert_eq!(state.request_restart(send), RestartRequest::Sent);
        for reason in [
            OfficeStatusReason::UnsupportedVersion,
            OfficeStatusReason::InvalidRequest,
        ] {
            state.set(OfficeState::unavailable(reason));
            assert_eq!(state.request_restart(send), RestartRequest::CantFix(reason));
        }
        assert_eq!(sent.get(), 3);
    }
}
