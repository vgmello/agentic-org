//! A writer custody that keeps its head in memory and applies exactly the host's rule.

use std::{
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

use super::HELD_BOUND;
use crate::custody::{CustodyError, JournalHead, OfficeIdentity, WriterAuthority};

#[derive(Default)]
struct State {
    stored: Option<(OfficeIdentity, JournalHead)>,
    closed: bool,
    confirmations: usize,
    hold_confirmations: bool,
    held_confirmations: usize,
}

#[derive(Default)]
pub struct FakeCustody {
    state: Mutex<State>,
    changed: Condvar,
}

impl FakeCustody {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Confirmations block until [`FakeCustody::release_confirmations`]: the host is slow.
    pub fn hold_confirmations(&self) {
        self.state().hold_confirmations = true;
    }

    /// Returns once a confirmation is waiting behind the hold. Panics when none has
    /// arrived within [`HELD_BOUND`]: the thread meant to confirm may have died before
    /// reaching the hold, and the test must then fail instead of waiting forever.
    pub fn wait_for_held_confirmation(&self) {
        self.wait_for_held_confirmation_within(HELD_BOUND);
    }

    fn wait_for_held_confirmation_within(&self, bound: Duration) {
        let deadline = Instant::now() + bound;
        let mut state = self.state();
        while state.held_confirmations == 0 {
            assert!(state.hold_confirmations, "no hold is set");
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                // Lift the hold first: a store dropped while this panic unwinds joins its
                // threads, and one that reached the hold meanwhile must not wait forever.
                state.hold_confirmations = false;
                self.changed.notify_all();
                drop(state);
                panic!(
                    "no confirmation reached the hold within {bound:?}: \
                 the thread meant to confirm may have died before it"
                );
            }
            let (next, _) = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner());
            state = next;
        }
    }

    pub fn release_confirmations(&self) {
        self.state().hold_confirmations = false;
        self.changed.notify_all();
    }

    /// The host channel is gone: every call fails.
    pub fn close(&self) {
        self.state().closed = true;
    }

    pub fn reopen(&self) {
        self.state().closed = false;
    }

    /// Sets the confirmed head directly, as if an earlier process had confirmed it.
    pub fn hold(&self, identity: &OfficeIdentity, head: JournalHead) {
        self.state().stored = Some((identity.clone(), head));
    }

    pub fn head(&self) -> Option<JournalHead> {
        self.state().stored.as_ref().map(|(_, head)| head.clone())
    }

    /// How many confirmations were accepted.
    pub fn confirmations(&self) -> usize {
        self.state().confirmations
    }
}

impl WriterAuthority for FakeCustody {
    fn checkpoint(&self, identity: &OfficeIdentity) -> Result<Option<JournalHead>, CustodyError> {
        let state = self.state();
        if state.closed {
            return Err(CustodyError("the host channel is closed".into()));
        }
        match &state.stored {
            Some((stored, _)) if stored != identity => {
                Err(CustodyError("another writer holds this office".into()))
            }
            Some((_, head)) => Ok(Some(head.clone())),
            None => Ok(None),
        }
    }

    fn confirm(
        &self,
        identity: &OfficeIdentity,
        before: Option<&JournalHead>,
        after: &JournalHead,
    ) -> Result<(), CustodyError> {
        let mut state = self.state();
        if state.hold_confirmations {
            state.held_confirmations += 1;
            self.changed.notify_all();
            while state.hold_confirmations {
                state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            state.held_confirmations -= 1;
        }
        if state.closed {
            return Err(CustodyError("the host channel is closed".into()));
        }
        let refuse = |why: &str| Err(CustodyError(why.into()));
        match &state.stored {
            Some((stored, _)) if stored != identity => return refuse("another incarnation"),
            Some((_, head)) => {
                if before != Some(head) {
                    return refuse("`before` is not the stored head");
                }
                // Identical is the whole head, high-waters included.
                if after.ordinal <= head.ordinal && after != head {
                    return refuse("the ordinal does not advance");
                }
                if !after.high_waters.none_behind(&head.high_waters) {
                    return refuse("a stream high-water went back");
                }
            }
            None if before.is_some() => return refuse("`before` is not the stored head"),
            None => {}
        }
        state.stored = Some((identity.clone(), after.clone()));
        state.confirmations += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{change::Sha256, sequences::HighWaters, testkit::identity};

    fn head(ordinal: u64, work: u64) -> JournalHead {
        JournalHead {
            ordinal,
            sha256: Sha256::of(&ordinal.to_be_bytes()),
            high_waters: HighWaters {
                work,
                ..HighWaters::default()
            },
        }
    }

    /// A custody that holds ordinal 5 with work high-water 9.
    fn holding() -> (FakeCustody, JournalHead) {
        let custody = FakeCustody::new();
        custody.confirm(&identity(), None, &head(5, 9)).unwrap();
        (custody, head(5, 9))
    }

    fn refused(custody: &FakeCustody, result: Result<(), CustodyError>, stored: &JournalHead) {
        assert!(result.is_err());
        assert_eq!(custody.head().as_ref(), Some(stored));
        assert_eq!(custody.confirmations(), 1);
    }

    #[test]
    fn a_confirmation_from_another_incarnation_is_refused() {
        let (custody, stored) = holding();
        let mut other = identity();
        other.journal_incarnation = crate::testkit::id(0xB4);
        let result = custody.confirm(&other, Some(&stored), &head(6, 9));
        refused(&custody, result, &stored);
        assert!(custody.checkpoint(&other).is_err());
    }

    #[test]
    fn a_confirmation_whose_before_is_not_the_stored_head_is_refused() {
        let (custody, stored) = holding();
        for before in [None, Some(head(4, 9)), Some(head(5, 8))] {
            let result = custody.confirm(&identity(), before.as_ref(), &head(6, 9));
            refused(&custody, result, &stored);
        }
        let fresh = FakeCustody::new();
        let result = fresh.confirm(&identity(), Some(&stored), &head(6, 9));
        assert!(result.is_err() && fresh.head().is_none());
    }

    #[test]
    fn a_confirmation_whose_ordinal_is_behind_is_refused_and_only_the_identical_head_repeats() {
        let (custody, stored) = holding();
        let result = custody.confirm(&identity(), Some(&stored), &head(4, 9));
        refused(&custody, result, &stored);
        // The same ordinal with another hash, or with other high-waters, is not the same head.
        let mut forked = head(5, 9);
        forked.sha256 = Sha256::of(b"another entry");
        let result = custody.confirm(&identity(), Some(&stored), &forked);
        refused(&custody, result, &stored);
        let result = custody.confirm(&identity(), Some(&stored), &head(5, 10));
        refused(&custody, result, &stored);
        custody
            .confirm(&identity(), Some(&stored), &stored)
            .unwrap();
        assert_eq!(custody.confirmations(), 2);
    }

    #[test]
    fn a_confirmation_that_takes_a_high_water_down_is_refused() {
        let (custody, stored) = holding();
        let result = custody.confirm(&identity(), Some(&stored), &head(6, 8));
        refused(&custody, result, &stored);
        custody
            .confirm(&identity(), Some(&stored), &head(6, 9))
            .unwrap();
        assert_eq!(custody.head(), Some(head(6, 9)));
    }

    /// A hold that no confirmation reaches (its thread died first) fails the wait, not
    /// the run.
    #[test]
    #[should_panic(expected = "no confirmation reached the hold within")]
    fn waiting_for_a_held_confirmation_that_never_arrives_fails_at_its_bound() {
        let custody = FakeCustody::new();
        custody.hold_confirmations();
        custody.wait_for_held_confirmation_within(Duration::from_millis(50));
    }

    /// The failed wait lifts the hold, so a store dropped while the panic unwinds never
    /// joins a thread that reached the hold in the meantime.
    #[test]
    fn a_failed_wait_lifts_the_hold() {
        let custody = FakeCustody::new();
        custody.hold_confirmations();
        let waited = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            custody.wait_for_held_confirmation_within(Duration::from_millis(50))
        }));
        assert!(waited.is_err());
        assert!(!custody.state().hold_confirmations);
    }
}
