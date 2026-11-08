//! Time figures (§6, rev 2): derived only from an item's immutable events. A pure function of
//! the events and `as_of`; any cache (`work_kpi`) must equal it, so [`TimeTally`] is the fold
//! itself, serializable, and gives the same answer however the events are fed to it.
//!
//! - **Rounds** open at `Created` and at a reopen (a `StatusChanged` out of a terminal status)
//!   and close at the next move into a terminal status. **Lead time** sums the rounds; an open
//!   round runs to `as_of`.
//! - **Cycle time** sums, over rounds that started, round end (or `as_of`) minus the round's
//!   first start (`in_progress`; a bug's `fixed` by hand; a request's `scoping`). A move back to
//!   `todo` keeps the start.
//! - **The split**, by precedence: waiting on the owner (at least one open waiting span, so
//!   overlapping spans count once; a bug in `fixed` reported by the
//!   owner; a request in `scoped`), then blocked (a task in `blocked`), then
//!   **the rest**. `waiting_on_owner + blocked + rest = lead`, always. Working time is untracked
//!   until sessions report it, and untracked time is never folded into idle: `working_ms` and
//!   `idle_ms` stay absent and the rest holds both.

use std::collections::{BTreeMap, BTreeSet};

use org_proto::work::{
    ItemKind, ItemStatus, Resolution, TimeFigures, VerifyOutcome, WaitReason, WorkEvent,
    WorkEventKind,
};
use serde::{Deserialize, Serialize};

use crate::states::{is_terminal, starts_cycle};
use crate::time::{format_ms, parse_ms};

/// The time figures of an item from its events, oldest first, as of `as_of_ms`. `created_by`
/// and `owner_id` say whether a bug's verification wait is the owner's.
pub fn time_figures(
    kind: ItemKind,
    created_by: &str,
    owner_id: &str,
    events: &[WorkEvent],
    as_of_ms: u64,
) -> TimeFigures {
    let mut tally = TimeTally::new(kind, created_by, owner_id);
    for e in events {
        tally.push(e);
    }
    tally.figures(as_of_ms)
}

/// The running fold behind [`time_figures`]: feed it an item's events in order, in any number
/// of batches (it may be stored between them), and read the figures at any time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeTally {
    kind: ItemKind,
    reported_by_owner: bool,
    status: ItemStatus,
    in_round: bool,
    started: bool,
    any_started: bool,
    rounds: u32,
    #[serde(default)]
    deleted: bool,
    /// The last close was a duplicate's: a reopen then goes back to `new`, not `triaged`.
    #[serde(default)]
    closed_as_duplicate: bool,
    /// The status before each automatic status change, by version, so an Undo of one puts the
    /// status back.
    #[serde(default)]
    auto_from: BTreeMap<u64, ItemStatus>,
    last_at_ms: Option<u64>,
    /// The open waiting spans, keyed by `(on_member_id, reason)`.
    spans: BTreeSet<String>,
    lead_ms: u64,
    cycle_ms: u64,
    waiting_ms: u64,
    blocked_ms: u64,
}

impl TimeTally {
    pub fn new(kind: ItemKind, created_by: &str, owner_id: &str) -> Self {
        Self {
            kind,
            reported_by_owner: created_by == owner_id,
            status: initial_status(kind),
            in_round: false,
            started: false,
            any_started: false,
            rounds: 0,
            deleted: false,
            closed_as_duplicate: false,
            auto_from: BTreeMap::new(),
            last_at_ms: None,
            spans: BTreeSet::new(),
            lead_ms: 0,
            cycle_ms: 0,
            waiting_ms: 0,
            blocked_ms: 0,
        }
    }

    /// Adds the next event. An event dated before the previous one counts as at the same time
    /// (the office never writes one; spans are never negative). An unreadable time does too.
    pub fn push(&mut self, event: &WorkEvent) {
        let last = self.last_at_ms;
        let at = parse_ms(&event.at)
            .unwrap_or(last.unwrap_or(0))
            .max(last.unwrap_or(0));
        self.advance_to(at);
        self.last_at_ms = Some(at);
        self.apply(event);
    }

    /// The figures as of `as_of_ms` (an open round runs to it).
    pub fn figures(&self, as_of_ms: u64) -> TimeFigures {
        let mut t = self.clone();
        let as_of = as_of_ms.max(self.last_at_ms.unwrap_or(0));
        t.advance_to(as_of);
        TimeFigures {
            lead_ms: t.lead_ms,
            cycle_ms: t.any_started.then_some(t.cycle_ms),
            waiting_on_owner_ms: t.waiting_ms,
            blocked_ms: t.blocked_ms,
            // Never below zero, even from a corrupt stored tally.
            rest_ms: t
                .lead_ms
                .saturating_sub(t.waiting_ms)
                .saturating_sub(t.blocked_ms),
            working_ms: None,
            idle_ms: None,
            open: t.in_round,
            rounds: t.rounds,
            as_of: format_ms(as_of),
        }
    }

    /// Counts the time from the last event to `at` into the current bucket.
    fn advance_to(&mut self, at: u64) {
        let Some(last) = self.last_at_ms else { return };
        if !self.in_round || at <= last {
            self.last_at_ms = Some(at.max(last));
            return;
        }
        let d = at - last;
        self.lead_ms = self.lead_ms.saturating_add(d);
        if self.started {
            self.cycle_ms = self.cycle_ms.saturating_add(d);
        }
        if self.waiting() {
            self.waiting_ms = self.waiting_ms.saturating_add(d);
        } else if self.kind == ItemKind::Task && self.status == ItemStatus::Blocked {
            self.blocked_ms = self.blocked_ms.saturating_add(d);
        }
        self.last_at_ms = Some(at);
    }

    fn waiting(&self) -> bool {
        !self.spans.is_empty()
            || (self.kind == ItemKind::Bug
                && self.status == ItemStatus::Fixed
                && self.reported_by_owner)
            || (self.kind == ItemKind::Request && self.status == ItemStatus::Scoped)
    }

    /// Ends the round: spans still open stop counting here.
    fn close_round(&mut self) {
        self.in_round = false;
        self.started = false;
        self.spans.clear();
    }

    /// Starts the next round (a reopen).
    fn open_round(&mut self) {
        self.in_round = true;
        self.rounds += 1;
        self.started = false;
    }

    /// Takes `status`; entering the cycle's start status starts the round's cycle time.
    fn enter(&mut self, status: ItemStatus) {
        self.status = status;
        if self.in_round && starts_cycle(self.kind, status) && !self.started {
            self.started = true;
            self.any_started = true;
        }
    }

    fn reopenable(&self) -> bool {
        !self.in_round && self.rounds > 0 && is_terminal(self.kind, self.status)
    }

    fn apply(&mut self, event: &WorkEvent) {
        match &event.change {
            WorkEventKind::Created if self.rounds == 0 => {
                self.in_round = true;
                self.rounds = 1;
            }
            // A deleted item stops counting (§2.8), and nothing reopens it.
            WorkEventKind::Deleted => {
                self.in_round = false;
                self.started = false;
                self.deleted = true;
                self.spans.clear();
            }
            _ if self.deleted => {}
            WorkEventKind::StatusChanged {
                from,
                to,
                resolution,
                ..
            } => {
                let to = *to;
                if event.auto {
                    self.auto_from.insert(event.version, *from);
                }
                if self.in_round && is_terminal(self.kind, to) {
                    self.close_round();
                    self.closed_as_duplicate = *resolution == Some(Resolution::Duplicate);
                } else if self.reopenable() && !is_terminal(self.kind, to) {
                    self.open_round();
                }
                self.enter(to);
            }
            // A bug triaged, verified, or reopened (§1.10): none of them is a `StatusChanged`.
            WorkEventKind::Triaged { .. } if self.in_round => self.enter(ItemStatus::Triaged),
            WorkEventKind::Verified { outcome, .. } if self.in_round => match outcome {
                VerifyOutcome::Fixed => {
                    self.close_round();
                    self.closed_as_duplicate = false;
                    self.status = ItemStatus::Closed;
                }
                _ => self.enter(ItemStatus::Triaged),
            },
            WorkEventKind::Reopened { .. } if self.reopenable() => {
                self.open_round();
                let to = if self.closed_as_duplicate {
                    ItemStatus::New
                } else {
                    ItemStatus::Triaged
                };
                self.enter(to);
            }
            // The Undo of an automatic status change puts the earlier status back. (A cycle it
            // had started stays started.)
            WorkEventKind::Undone { event_version } if self.in_round => {
                if let Some(from) = self.auto_from.get(event_version) {
                    self.status = *from;
                }
            }
            WorkEventKind::WaitingStarted {
                on_member_id,
                reason,
            } if self.in_round => {
                self.spans.insert(span_key(on_member_id, reason));
            }
            WorkEventKind::WaitingEnded {
                on_member_id,
                reason,
            } => {
                self.spans.remove(&span_key(on_member_id, reason));
            }
            _ => {}
        }
    }
}

/// A span's key: `(on_member_id, reason)` as JSON.
fn span_key(on: &str, reason: &WaitReason) -> String {
    serde_json::to_string(&(on, reason)).expect("a wait reason always encodes")
}

fn initial_status(kind: ItemKind) -> ItemStatus {
    match kind {
        ItemKind::Task => ItemStatus::Todo,
        _ => ItemStatus::New,
    }
}
