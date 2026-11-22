//! The Mailbox's snooze timers (ADR-0003 §2.2, `L-a`, `TASK-81`): every active time-based snooze
//! as a `(deadline, member, message)` entry keyed per `(member_id, message_id)` so multiple
//! recipients of the same channel or group-DM message maintain independent snooze timers.
//!
//! Wall-clock epoch milliseconds from the Mailbox's [`crate::Clock`] are used because stored
//! deadlines are UTC strings and a snooze is due when the wall clock passes it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Ordered snooze timer set across all members, indexed per `(member_id, message_id)`.
#[derive(Debug, Default)]
pub(crate) struct SnoozeTimers {
    by_time: BTreeSet<(i64, String, String)>,
    /// `(member_id, msg_id)` -> `at_ms`: each recipient has at most one active timer per message.
    by_key: HashMap<(String, String), i64>,
}

impl SnoozeTimers {
    /// Sets `(member_id, id)`'s timer, replacing any earlier one for that recipient.
    pub(crate) fn insert(&mut self, at_ms: i64, member_id: &str, id: &str) {
        self.remove(member_id, id);
        self.by_time
            .insert((at_ms, member_id.to_owned(), id.to_owned()));
        self.by_key
            .insert((member_id.to_owned(), id.to_owned()), at_ms);
    }

    /// Puts back a timer taken with [`Self::take_due`] or [`Self::take_all_due`] unless
    /// `(member_id, id)` already has a newer timer.
    pub(crate) fn restore(&mut self, at_ms: i64, member_id: &str, id: &str) {
        if !self
            .by_key
            .contains_key(&(member_id.to_owned(), id.to_owned()))
        {
            self.insert(at_ms, member_id, id);
        }
    }

    /// Drops `(member_id, id)`'s timer, if it has one.
    pub(crate) fn remove(&mut self, member_id: &str, id: &str) {
        if let Some(at_ms) = self.by_key.remove(&(member_id.to_owned(), id.to_owned())) {
            self.by_time
                .remove(&(at_ms, member_id.to_owned(), id.to_owned()));
        }
    }

    /// Earliest active deadline across all members, if any.
    pub(crate) fn next_deadline(&self) -> Option<i64> {
        self.by_time.first().map(|(at_ms, _, _)| *at_ms)
    }

    /// The member's earliest deadline.
    pub(crate) fn next_for(&self, member_id: &str) -> Option<i64> {
        self.by_time
            .iter()
            .find(|(_, member, _)| member == member_id)
            .map(|(at_ms, _, _)| *at_ms)
    }

    /// Removes and returns all due timers across all members (`deadline <= now_ms`), grouped by
    /// `member_id` for batch resurfacing.
    pub(crate) fn take_all_due(&mut self, now_ms: i64) -> Vec<(String, Vec<(i64, String)>)> {
        let due: Vec<(i64, String, String)> = self
            .by_time
            .iter()
            .take_while(|(at_ms, _, _)| *at_ms <= now_ms)
            .cloned()
            .collect();
        let mut grouped: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
        for (at_ms, member, id) in due {
            self.remove(&member, &id);
            grouped.entry(member).or_default().push((at_ms, id));
        }
        grouped.into_iter().collect()
    }

    /// Removes and returns the member's timers due at `now_ms` (`deadline <= now`), as
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
            self.remove(member_id, id);
        }
        due
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        debug_assert_eq!(self.by_time.len(), self.by_key.len());
        self.by_key.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_timer_per_member_and_message_ordered_by_deadline() {
        let mut timers = SnoozeTimers::default();
        timers.insert(300, "ada", "m1");
        timers.insert(100, "remy", "m2");
        timers.insert(200, "ada", "m3");
        // Multi-recipient isolation: remy snoozing m1 must not touch ada's m1 timer!
        timers.insert(500, "remy", "m1");
        assert_eq!(timers.len(), 4);
        assert_eq!(timers.next_deadline(), Some(100));
        assert_eq!(timers.next_for("ada"), Some(200));
        assert_eq!(timers.next_for("remy"), Some(100));
        assert_eq!(timers.next_for("pat"), None);

        // Re-snoozing replaces only that member's timer.
        timers.insert(50, "ada", "m1");
        assert_eq!(timers.len(), 4);
        assert_eq!(timers.next_for("ada"), Some(50));
        assert_eq!(timers.next_for("remy"), Some(100));

        assert!(timers.take_due("ada", 49).is_empty());
        assert_eq!(timers.take_due("ada", 100), [(50, "m1".to_owned())]);
        assert_eq!(timers.next_for("ada"), Some(200));
        assert_eq!(timers.len(), 3);
        assert_eq!(
            timers.take_due("remy", i64::MAX),
            [(100, "m2".to_owned()), (500, "m1".to_owned())]
        );
        assert_eq!(timers.next_for("remy"), None);
        timers.insert(400, "ada", "m4");
        assert_eq!(
            timers.take_all_due(500),
            vec![(
                "ada".to_owned(),
                vec![(200, "m3".to_owned()), (400, "m4".to_owned())]
            )]
        );
        timers.restore(200, "ada", "m3");
        timers.insert(900, "ada", "m4");
        timers.restore(400, "ada", "m4");
        assert_eq!(timers.next_for("ada"), Some(200));
        assert_eq!(timers.take_due("ada", 899), [(200, "m3".to_owned())]);
        timers.restore(200, "ada", "m3");
        timers.remove("ada", "m4");
        timers.remove("ada", "m3");
        timers.remove("ada", "m3");
        timers.remove("ada", "unknown");
        assert_eq!(timers.len(), 0);
    }
}
