//! Time figures (§6): hand-built event sequences with known answers, figures from the engine's
//! own events, and property tests over random sequences.

use org_proto::org::Position;
use org_proto::work::*;

use super::support::*;
use crate::{format_ms, time_figures, TimeTally, WorkCall};

use ItemStatus::*;

fn ev(at: u64, change: WorkEventKind) -> WorkEvent {
    WorkEvent {
        target_id: "t".into(),
        version: 0,
        at: format_ms(at),
        actor_id: None,
        auto: false,
        cause: None,
        org_position: Position::default(),
        change,
    }
}

fn status(at: u64, from: ItemStatus, to: ItemStatus) -> WorkEvent {
    ev(
        at,
        WorkEventKind::StatusChanged {
            from,
            to,
            resolution: (to == Done).then_some(Resolution::Completed),
            reason: None,
            anyway: None,
        },
    )
}

fn wait_on(at: u64, start: bool, comment_id: &str) -> WorkEvent {
    let reason = WaitReason::Question {
        comment_id: comment_id.into(),
    };
    ev(
        at,
        if start {
            WorkEventKind::WaitingStarted {
                on_member_id: OWNER.into(),
                reason,
            }
        } else {
            WorkEventKind::WaitingEnded {
                on_member_id: OWNER.into(),
                reason,
            }
        },
    )
}

fn wait(at: u64, start: bool) -> WorkEvent {
    wait_on(at, start, "q1")
}

fn task(events: &[WorkEvent], as_of: u64) -> TimeFigures {
    time_figures(ItemKind::Task, "ada", OWNER, events, as_of)
}

#[test]
fn a_task_blocked_for_two_hours_inside_a_ten_hour_lead() {
    let events = [
        ev(T0, WorkEventKind::Created),
        status(T0 + HOUR, Todo, InProgress),
        status(T0 + 3 * HOUR, InProgress, Blocked),
        status(T0 + 5 * HOUR, Blocked, InProgress),
        status(T0 + 10 * HOUR, InProgress, Done),
    ];
    let t = task(&events, T0 + 20 * HOUR);
    assert_eq!(t.lead_ms, 10 * HOUR);
    assert_eq!(t.blocked_ms, 2 * HOUR);
    assert_eq!(t.waiting_on_owner_ms, 0);
    assert_eq!(t.rest_ms, 8 * HOUR);
    assert_eq!(t.cycle_ms, Some(9 * HOUR));
    assert_eq!((t.open, t.rounds), (false, 1));
    // Untracked working time is never shown as idle.
    assert_eq!((t.working_ms, t.idle_ms), (None, None));
    assert_eq!(t.as_of, format_ms(T0 + 20 * HOUR));
}

#[test]
fn an_open_item_runs_to_as_of_and_has_no_cycle_until_started() {
    let events = [ev(T0, WorkEventKind::Created)];
    let t = task(&events, T0 + 3 * HOUR);
    assert_eq!(
        (t.lead_ms, t.rest_ms, t.cycle_ms, t.open, t.rounds),
        (3 * HOUR, 3 * HOUR, None, true, 1)
    );
}

#[test]
fn questions_to_the_owner_count_as_waiting_once_and_win_over_blocked() {
    let events = [
        ev(T0, WorkEventKind::Created),
        status(T0 + HOUR, Todo, Blocked),
        wait_on(T0 + 2 * HOUR, true, "q1"),
        // A second span overlapping the first counts once.
        wait_on(T0 + 3 * HOUR, true, "q2"),
        wait_on(T0 + 3 * HOUR + 30 * MINUTE, false, "q1"),
        wait_on(T0 + 4 * HOUR, false, "q2"),
        status(T0 + 5 * HOUR, Blocked, Done),
    ];
    let t = task(&events, T0 + 5 * HOUR);
    assert_eq!(t.lead_ms, 5 * HOUR);
    assert_eq!(t.waiting_on_owner_ms, 2 * HOUR);
    assert_eq!(t.blocked_ms, 2 * HOUR);
    assert_eq!(t.rest_ms, HOUR);
}

#[test]
fn a_reopened_task_has_two_rounds_and_a_cycle_per_round() {
    let events = [
        ev(T0, WorkEventKind::Created),
        status(T0 + HOUR, Todo, InProgress),
        // Back to To do keeps the round's start.
        status(T0 + 2 * HOUR, InProgress, Todo),
        status(T0 + 3 * HOUR, Todo, Done),
        // Done for 10 hours: outside every round.
        status(T0 + 13 * HOUR, Done, Todo),
        status(T0 + 15 * HOUR, Todo, InProgress),
        status(T0 + 16 * HOUR, InProgress, Done),
    ];
    let t = task(&events, T0 + 30 * HOUR);
    assert_eq!(t.rounds, 2);
    assert_eq!(t.lead_ms, 3 * HOUR + 3 * HOUR);
    assert_eq!(t.cycle_ms, Some(2 * HOUR + HOUR));
    assert!(!t.open);
}

#[test]
fn a_span_open_at_a_rounds_end_stops_there() {
    let events = [
        ev(T0, WorkEventKind::Created),
        wait(T0 + HOUR, true),
        status(T0 + 2 * HOUR, Todo, Done),
        status(T0 + 5 * HOUR, Done, Todo),
    ];
    let t = task(&events, T0 + 6 * HOUR);
    assert_eq!(t.waiting_on_owner_ms, HOUR);
    assert_eq!(t.lead_ms, 3 * HOUR);
    assert_eq!(t.rounds, 2);
}

#[test]
fn bugs_waiting_for_the_owners_verification_and_requests_waiting_for_approval() {
    let bug = [
        ev(T0, WorkEventKind::Created),
        status(T0 + HOUR, New, Triaged),
        status(T0 + 2 * HOUR, Triaged, Fixed),
        status(T0 + 4 * HOUR, Fixed, Closed),
    ];
    let by_owner = time_figures(ItemKind::Bug, OWNER, OWNER, &bug, T0 + 9 * HOUR);
    assert_eq!(
        (by_owner.waiting_on_owner_ms, by_owner.lead_ms),
        (2 * HOUR, 4 * HOUR)
    );
    assert_eq!(
        by_owner.cycle_ms,
        Some(2 * HOUR),
        "fixed by hand starts the cycle"
    );
    let by_agent = time_figures(ItemKind::Bug, "ada", OWNER, &bug, T0 + 9 * HOUR);
    assert_eq!(by_agent.waiting_on_owner_ms, 0);

    let request = [
        ev(T0, WorkEventKind::Created),
        status(T0 + HOUR, New, Scoping),
        status(T0 + 2 * HOUR, Scoping, Scoped),
        status(T0 + 5 * HOUR, Scoped, Approved),
    ];
    let r = time_figures(ItemKind::Request, "ada", OWNER, &request, T0 + 9 * HOUR);
    assert_eq!(
        (r.waiting_on_owner_ms, r.cycle_ms, r.lead_ms),
        (3 * HOUR, Some(4 * HOUR), 5 * HOUR)
    );
}

#[test]
fn the_engines_events_give_the_figures() {
    let w = World::new();
    let id = w.task();
    w.clock.advance(HOUR);
    w.set(&id, InProgress);
    w.clock.advance(HOUR);
    let mut q = w.comment(&id, "Which API?");
    q.comment_kind = Some(CommentKind::Question);
    q.asks = Some(OWNER.into());
    let q_id = q.comment_id.clone();
    w.ok(&agent(), WorkCall::Comment(q));
    w.clock.advance(2 * HOUR);
    let mut a = w.comment(&id, "Stripe");
    a.comment_kind = Some(CommentKind::Answer);
    a.answers = Some(q_id);
    w.ok(&app(), WorkCall::Comment(a));
    w.clock.advance(HOUR);
    w.set(&id, Blocked);
    w.clock.advance(HOUR);
    w.set(&id, Done);
    let t = time_figures(ItemKind::Task, OWNER, OWNER, &w.events(&id), w.now() + DAY);
    assert_eq!(t.lead_ms, 6 * HOUR);
    assert_eq!(t.cycle_ms, Some(5 * HOUR));
    assert_eq!(t.waiting_on_owner_ms, 2 * HOUR);
    assert_eq!(t.blocked_ms, HOUR);
    assert_eq!(t.rest_ms, 3 * HOUR);
}

#[test]
fn a_reopen_reopens_the_span_of_an_unanswered_question() {
    let w = World::new();
    let id = w.task();
    let mut q = w.comment(&id, "Which API?");
    q.comment_kind = Some(CommentKind::Question);
    q.asks = Some(OWNER.into());
    w.ok(&agent(), WorkCall::Comment(q));
    w.clock.advance(HOUR);
    w.set(&id, Done);
    w.clock.advance(HOUR);
    w.set(&id, Todo);
    assert!(matches!(
        w.last_event(&id).change,
        WorkEventKind::WaitingStarted { .. }
    ));
    w.clock.advance(HOUR);
    let t = time_figures(ItemKind::Task, OWNER, OWNER, &w.events(&id), w.now());
    assert_eq!(
        (t.lead_ms, t.waiting_on_owner_ms, t.rounds),
        (2 * HOUR, 2 * HOUR, 2)
    );
}

/// xorshift64*: a small deterministic generator for property tests.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A random but well-formed task history: moves from the table, questions, gaps.
fn random_history(rng: &mut Rng) -> Vec<WorkEvent> {
    let mut at = T0;
    let mut s = Todo;
    let mut events = vec![ev(at, WorkEventKind::Created)];
    for _ in 0..rng.below(40) {
        at += rng.below(5 * HOUR);
        // Now and then an event dated earlier than the previous one (counts as the same time).
        let when = if rng.below(10) == 0 { at - MINUTE } else { at };
        match rng.below(4) {
            0 => {
                let q = format!("q{}", rng.below(3));
                events.push(wait_on(when, rng.below(2) == 0, &q));
            }
            _ => {
                let options: Vec<ItemStatus> = [Todo, InProgress, Blocked, Done]
                    .into_iter()
                    .filter(|&to| crate::task_move_allowed(s, to))
                    .collect();
                let to = options[rng.below(options.len() as u64) as usize];
                events.push(status(when, s, to));
                s = to;
            }
        }
    }
    events
}

#[test]
fn property_the_parts_always_sum_to_the_lead_time() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..500 {
        let events = random_history(&mut rng);
        let as_of = T0 + rng.below(200 * HOUR);
        let t = task(&events, as_of);
        assert_eq!(t.waiting_on_owner_ms + t.blocked_ms + t.rest_ms, t.lead_ms);
        assert!(t.cycle_ms.unwrap_or(0) <= t.lead_ms);
        assert_eq!((t.working_ms, t.idle_ms), (None, None));
    }
}

#[test]
fn property_figures_are_the_same_however_the_events_are_batched() {
    let mut rng = Rng(42);
    for _ in 0..300 {
        let events = random_history(&mut rng);
        let as_of = T0 + rng.below(200 * HOUR);
        let whole = task(&events, as_of);
        // Feed the events in random batches, storing the tally (as a cache would) between them.
        let mut tally = TimeTally::new(ItemKind::Task, "ada", OWNER);
        let mut i = 0;
        while i < events.len() {
            let n = 1 + rng.below(5) as usize;
            for e in &events[i..(i + n).min(events.len())] {
                tally.push(e);
            }
            i += n;
            let stored = serde_json::to_string(&tally).unwrap();
            tally = serde_json::from_str(&stored).unwrap();
        }
        assert_eq!(tally.figures(as_of), whole);
    }
}

#[test]
fn a_deleted_item_stops_counting() {
    let events = [
        ev(T0, WorkEventKind::Created),
        wait(T0 + HOUR, true),
        ev(T0 + 2 * HOUR, WorkEventKind::Deleted),
        // Anything after the delete changes nothing.
        status(T0 + 3 * HOUR, Todo, InProgress),
    ];
    let t = task(&events, T0 + 10 * HOUR);
    assert_eq!(
        (t.lead_ms, t.waiting_on_owner_ms, t.open, t.cycle_ms),
        (2 * HOUR, HOUR, false, None)
    );
}

#[test]
fn a_corrupt_stored_tally_never_panics() {
    let mut tally = serde_json::to_value(TimeTally::new(ItemKind::Task, "ada", OWNER)).unwrap();
    tally["lead_ms"] = 5.into();
    tally["waiting_ms"] = 9.into();
    tally["blocked_ms"] = u64::MAX.into();
    let tally: TimeTally = serde_json::from_value(tally).unwrap();
    assert_eq!(tally.figures(T0).rest_ms, 0);
}

#[test]
fn a_span_started_outside_a_round_counts_nothing() {
    let events = [
        ev(T0, WorkEventKind::Created),
        status(T0 + HOUR, Todo, Done),
        wait(T0 + 2 * HOUR, true),
        status(T0 + 3 * HOUR, Done, Todo),
    ];
    let t = task(&events, T0 + 5 * HOUR);
    assert_eq!((t.lead_ms, t.waiting_on_owner_ms), (3 * HOUR, 0));
}
