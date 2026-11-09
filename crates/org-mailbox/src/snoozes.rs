//! The Mailbox's snooze timers (ADR-0003 §2.2, `L-a`): every active time-based snooze as a
//! `(deadline, member, message)` entry, so a held `msg.wait` can bound its block by the
//! member's next deadline and release what is due.
//!
//! ADR-0003 §2.2 keys the set by `Instant`. These are wall-clock epoch milliseconds from the
//! Mailbox's [`crate::Clock`] instead: the stored deadlines are UTC strings, a snooze is due
//! when the wall clock passes it (as `check_notify` decides), and an `Instant` taken at
//! startup would drift from the wall clock across a sleep or clock change.

use std::collections::{BTreeSet, HashMap};

/// One ordered set per member, so `next_for` and `take_due` touch only that member's timers
/// however many other members have. A timer stays until a wait for its member takes it, a
/// defer, done or expiry replaces or drops it, or [`Self::remove_if_due`] drops it once its
/// snooze is released.
#[derive(Debug, Default)]
pub(crate) struct SnoozeTimers {
    /// member_id -> its timers as `(at_ms, msg_id)`, earliest first. A member with no timers
    /// has no entry.
    by_member: HashMap<String, BTreeSet<(i64, String)>>,
    /// msg_id -> (at_ms, member_id): the entry in `by_member`, so one message has one timer.
    by_id: HashMap<String, (i64, String)>,
}

impl SnoozeTimers {
    /// Sets `id`'s timer, replacing any earlier one.
    pub(crate) fn insert(&mut self, at_ms: i64, member_id: &str, id: &str) {
        self.remove(id);
        self.by_member
            .entry(member_id.to_owned())
            .or_default()
            .insert((at_ms, id.to_owned()));
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
            self.unindex(&member_id, at_ms, id);
        }
    }

    /// Drops `id`'s timer only if it is due at `now_ms` (deadline <= now), and says whether
    /// it did. A timer set since for a later deadline (a re-defer) stays. For `msg.notify`,
    /// which drops the timers of the snoozes its `check_notify` released as due.
    // `msg.notify` (slice 2b) is its first caller.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn remove_if_due(&mut self, id: &str, now_ms: i64) -> bool {
        match self.by_id.get(id) {
            Some((at_ms, _)) if *at_ms <= now_ms => {
                self.remove(id);
                true
            }
            _ => false,
        }
    }

    /// The member's earliest deadline.
    pub(crate) fn next_for(&self, member_id: &str) -> Option<i64> {
        self.by_member
            .get(member_id)
            .and_then(|timers| timers.first())
            .map(|(at_ms, _)| *at_ms)
    }

    /// Removes and returns the member's timers due at `now_ms` (deadline <= now), as
    /// `(at_ms, id)`, earliest first. A caller that fails to release them puts them back with
    /// [`Self::restore`].
    pub(crate) fn take_due(&mut self, member_id: &str, now_ms: i64) -> Vec<(i64, String)> {
        let Some(timers) = self.by_member.get_mut(member_id) else {
            return Vec::new();
        };
        // Everything before (now + 1, "") is due: deadlines <= now, whatever the id.
        let due = match now_ms.checked_add(1) {
            Some(after) => {
                let later = timers.split_off(&(after, String::new()));
                std::mem::replace(timers, later)
            }
            None => std::mem::take(timers),
        };
        if timers.is_empty() {
            self.by_member.remove(member_id);
        }
        for (_, id) in &due {
            self.by_id.remove(id);
        }
        due.into_iter().collect()
    }

    /// Removes `(at_ms, id)` from `member_id`'s set, dropping the set once it's empty.
    fn unindex(&mut self, member_id: &str, at_ms: i64, id: &str) {
        if let Some(timers) = self.by_member.get_mut(member_id) {
            timers.remove(&(at_ms, id.to_owned()));
            if timers.is_empty() {
                self.by_member.remove(member_id);
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        debug_assert_eq!(
            self.by_member.values().map(BTreeSet::len).sum::<usize>(),
            self.by_id.len()
        );
        self.by_id.len()
    }

    /// Members with an index entry: one per member that has a timer, none left empty.
    #[cfg(test)]
    fn indexed_members(&self) -> usize {
        debug_assert!(self.by_member.values().all(|timers| !timers.is_empty()));
        self.by_member.len()
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
        assert_eq!(timers.indexed_members(), 0);
    }

    /// `take_due` and `next_for` see only the member's own timers: another member's earlier
    /// (or due) timers are neither returned nor taken, and a timer due exactly now is due.
    #[test]
    fn timers_are_indexed_per_member() {
        let mut timers = SnoozeTimers::default();
        for n in 0..1_000 {
            timers.insert(n, "remy", &format!("r{n}"));
        }
        timers.insert(500, "ada", "a1");
        timers.insert(i64::MAX, "ada", "a2");
        assert_eq!(timers.indexed_members(), 2);
        assert_eq!(timers.next_for("ada"), Some(500));
        assert!(timers.take_due("ada", 499).is_empty());
        assert_eq!(timers.take_due("ada", 500), [(500, "a1".to_owned())]);
        assert_eq!(timers.len(), 1_001, "remy's timers stay");
        assert_eq!(timers.next_for("remy"), Some(0));
        assert_eq!(
            timers.take_due("ada", i64::MAX),
            [(i64::MAX, "a2".to_owned())]
        );
        assert_eq!(timers.indexed_members(), 1, "ada's empty set is dropped");
        assert!(timers.take_due("pat", i64::MAX).is_empty());
        assert_eq!(timers.indexed_members(), 1);

        // Moving a timer to another member (never by the Mailbox, but the index must hold).
        timers.insert(7, "ada", "r3");
        assert_eq!(timers.next_for("ada"), Some(7));
        assert_eq!(
            timers.take_due("remy", 3),
            [0, 1, 2].map(|n| (n, format!("r{n}")))
        );
        assert_eq!(timers.len(), 997);
    }

    /// `remove_if_due` drops a timer only once it is due; a re-defer to a later deadline
    /// keeps its timer.
    #[test]
    fn remove_if_due_drops_only_a_due_timer() {
        let mut timers = SnoozeTimers::default();
        timers.insert(100, "ada", "m1");
        timers.insert(200, "ada", "m2");
        assert!(!timers.remove_if_due("m1", 99));
        assert!(!timers.remove_if_due("unknown", i64::MAX));
        assert_eq!(timers.len(), 2);
        assert!(timers.remove_if_due("m1", 100));
        assert!(!timers.remove_if_due("m1", 100), "already gone");
        assert_eq!(timers.next_for("ada"), Some(200));

        // Released as due at 250, but re-deferred to 300 before the release is applied.
        timers.insert(300, "ada", "m2");
        assert!(!timers.remove_if_due("m2", 250));
        assert_eq!(timers.next_for("ada"), Some(300));
        assert!(timers.remove_if_due("m2", 300));
        assert_eq!(timers.len(), 0);
        assert_eq!(timers.indexed_members(), 0);
    }
}
