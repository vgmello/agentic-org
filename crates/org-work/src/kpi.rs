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

use std::collections::BTreeSet;

use org_proto::work::{ItemKind, ItemStatus, TimeFigures, WaitReason, WorkEvent, WorkEventKind};
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
        self.apply(&event.change);
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
            rest_ms: t.lead_ms - t.waiting_ms - t.blocked_ms,
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
        self.lead_ms += d;
        if self.started {
            self.cycle_ms += d;
        }
        if self.waiting() {
            self.waiting_ms += d;
        } else if self.kind == ItemKind::Task && self.status == ItemStatus::Blocked {
            self.blocked_ms += d;
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

    fn apply(&mut self, change: &WorkEventKind) {
        match change {
            WorkEventKind::Created if self.rounds == 0 => {
                self.in_round = true;
                self.rounds = 1;
            }
            WorkEventKind::StatusChanged { to, .. } => {
                let to = *to;
                if self.in_round && is_terminal(self.kind, to) {
                    // The round ends; spans still open stop counting here.
                    self.in_round = false;
                    self.started = false;
                    self.spans.clear();
                } else if !self.in_round
                    && self.rounds > 0
                    && is_terminal(self.kind, self.status)
                    && !is_terminal(self.kind, to)
                {
                    self.in_round = true;
                    self.rounds += 1;
                    self.started = false;
                }
                self.status = to;
                if self.in_round && starts_cycle(self.kind, to) && !self.started {
                    self.started = true;
                    self.any_started = true;
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
