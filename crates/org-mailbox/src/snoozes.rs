//! The Mailbox's snooze timers (ADR-0003 §2.2, `L-a`): every active time-based snooze as a
//! `(deadline, member, message)` entry, so a held `msg.wait` can bound its block by the
//! member's next deadline and release what is due.
//!
//! ADR-0003 §2.2 keys the set by `Instant`. These are wall-clock epoch milliseconds from the
//! Mailbox's [`crate::Clock`] instead: the stored deadlines are UTC strings, a snooze is due
//! when the wall clock passes it (as `check_notify` decides), and an `Instant` taken at
//! startup would drift from the wall clock across a sleep or clock change.

use std::collections::{BTreeSet, HashMap};

/// One ordered set for every member: `next_for` and `take_due` scan it, and a timer stays
/// until a wait for its member takes it (or a defer, done or expiry replaces or drops it). A
/// member who never waits keeps due timers here; a per-member index (or cleanup when
/// `msg.notify` releases snoozes) is the follow-up if the set grows large.
#[derive(Debug, Default)]
pub(crate) struct SnoozeTimers {
    by_time: BTreeSet<(i64, String, String)>,
    /// msg_id -> (at_ms, member_id): the entry in `by_time`, so one message has one timer.
    by_id: HashMap<String, (i64, String)>,
}

impl SnoozeTimers {
    /// Sets `id`'s timer, replacing any earlier one.
    pub(crate) fn insert(&mut self, at_ms: i64, member_id: &str, id: &str) {
        self.remove(id);
        self.by_time
            .insert((at_ms, member_id.to_owned(), id.to_owned()));
        self.by_id
            .insert(id.to_owned(), (at_ms, member_id.to_owned()));
    }

    /// Puts back a timer taken with [`Self::take_due`] unless `id` has one again (a newer
    /// defer's timer wins).
    pub(crate) fn restore(&mut self, at_ms: i64, member_id: &str, id: &str) {
        if !self.by_id.contains_key(id) {
            self.insert(at_ms, member_id, id);
        }
    }

    /// Drops `id`'s timer, if it has one.
    pub(crate) fn remove(&mut self, id: &str) {
        if let Some((at_ms, member_id)) = self.by_id.remove(id) {
            self.by_time.remove(&(at_ms, member_id, id.to_owned()));
        }
    }

    /// The member's earliest deadline.
    pub(crate) fn next_for(&self, member_id: &str) -> Option<i64> {
        self.by_time
            .iter()
            .find(|(_, member, _)| member == member_id)
            .map(|(at_ms, _, _)| *at_ms)
    }

    /// Removes and returns the member's timers due at `now_ms` (deadline <= now), as
    /// `(at_ms, id)`, earliest first. A caller that fails to release them puts them back with
    /// [`Self::restore`].
    pub(crate) fn take_due(&mut self, member_id: &str, now_ms: i64) -> Vec<(i64, String)> {
        let due: Vec<(i64, String)> = self
            .by_time
            .iter()
            .take_while(|(at_ms, _, _)| *at_ms <= now_ms)
            .filter(|(_, member, _)| member == member_id)
            .map(|(at_ms, _, id)| (*at_ms, id.clone()))
            .collect();
        for (_, id) in &due {
            self.remove(id);
        }
        due
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        debug_assert_eq!(self.by_time.len(), self.by_id.len());
        self.by_id.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_timer_per_message_ordered_by_deadline_per_member() {
        let mut timers = SnoozeTimers::default();
        timers.insert(300, "ada", "m1");
        timers.insert(100, "remy", "m2");
        timers.insert(200, "ada", "m3");
        assert_eq!(timers.next_for("ada"), Some(200));
        assert_eq!(timers.next_for("remy"), Some(100));
        assert_eq!(timers.next_for("pat"), None);

        // Re-snoozing replaces the earlier timer.
        timers.insert(50, "ada", "m1");
        assert_eq!(timers.len(), 3);
        assert_eq!(timers.next_for("ada"), Some(50));

        assert!(timers.take_due("ada", 49).is_empty());
        assert_eq!(timers.take_due("ada", 100), [(50, "m1".to_owned())]);
        assert_eq!(timers.next_for("ada"), Some(200));
        assert_eq!(timers.len(), 2);
        assert_eq!(timers.take_due("remy", i64::MAX), [(100, "m2".to_owned())]);
        assert_eq!(timers.next_for("remy"), None);
        timers.insert(400, "ada", "m4");
        assert_eq!(
            timers.take_due("ada", 500),
            [(200, "m3".to_owned()), (400, "m4".to_owned())]
        );
        timers.restore(200, "ada", "m3");
        timers.insert(900, "ada", "m4");
        timers.restore(400, "ada", "m4");
        assert_eq!(timers.next_for("ada"), Some(200));
        assert_eq!(timers.take_due("ada", 899), [(200, "m3".to_owned())]);
        timers.restore(200, "ada", "m3");
        timers.remove("m4");

        timers.remove("m3");
        timers.remove("m3");
        timers.remove("unknown");
        assert_eq!(timers.len(), 0);
        assert_eq!(timers.next_for("ada"), None);
    }
}
