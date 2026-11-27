//! `TASK-87`: the office-local task counter's numbering (`ADR-0019`, `doc-80`), on both
//! engines.
//!
//! - an unclean stop, then an opening with no create and a clean stop: the reservation is
//!   spent before the clean-stop marker, so the next opening never issues a number the
//!   crashed run may have answered (`ADR-0019` Decision 7);
//! - concurrent creates after an unclean stop jump past the reservation once;
//! - a counter that is missing beside tasks of the office's code, or behind them, refuses a
//!   create rather than reconstruct or reissue a number (`doc-80`).

use std::sync::Arc;

use org_proto::tasks::TasksCreateParams;
use org_proto::ErrorCode;
use org_store::{Ack, Change};
use serde_json::json;

use super::task_storage::{params, uid, Bench, TEAM_B};
use super::*;
use crate::store::open::task_ops::TaskCreateOp;
use crate::tasks::features::TASK_NUMBER_BLOCK;
use crate::tasks::records::{self, OfficeTaskCounterRecord};

/// A change straight to the store, as damage outside the office (a lost file, a restored
/// backup) leaves its files.
struct Raw(Change);

impl StoreOp for Raw {
    type Out = Result<Reply, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let change = self.0;
        store.submit(None, true, move |_| Ok(change.clone()))
    }
}

fn write_counter(b: &Bench, counter: Option<OfficeTaskCounterRecord>) {
    let office = &b.here.office_id;
    let path = records::counter_path(office);
    let change = match counter {
        Some(counter) => {
            Change::new(Ack::Durable).put(path, records::encode_counter(office, &counter))
        }
        None => Change::new(Ack::Durable).remove(path),
    };
    let reply = b.store().run(Raw(change)).expect("written");
    assert_eq!(reply.outcome, Outcome::Completed);
}

#[test]
fn a_clean_stop_after_an_unclean_one_spends_the_reservation_before_its_marker() {
    for engine in ENGINES {
        let mut b = Bench::new(engine);
        let first = b.create(Some(TEAM_B), None);
        // Inside the block: `committed`, so a power cut may take it back after its answer.
        let second = b.create(Some(TEAM_B), None);
        assert_eq!((first.number, second.number), (1, 2), "{engine:?}");
        assert_eq!(b.ack(&uid(b.next - 1)), Some(Ack::Committed));

        // A kill, then a run that creates nothing and stops cleanly.
        b.reopen_after_crash();
        b.reopen_clean();
        let counter = b.counter();
        assert_eq!(
            counter[0].last_number, TASK_NUMBER_BLOCK,
            "{engine:?}: the clean stop spent the reservation"
        );
        assert_eq!(counter[0].reserved_through, TASK_NUMBER_BLOCK);
        let next = b.create(Some(TEAM_B), None);
        assert_eq!(
            next.number,
            TASK_NUMBER_BLOCK + 1,
            "{engine:?}: never T-<CODE>2 again"
        );
        assert_eq!(b.ack(&uid(b.next - 1)), Some(Ack::Durable));

        // Two unclean stops in a row, then a clean one: still past the reservation.
        b.reopen_after_crash();
        b.reopen_after_crash();
        b.reopen_clean();
        let next = b.create(Some(TEAM_B), None);
        assert_eq!(next.number, 2 * TASK_NUMBER_BLOCK + 1, "{engine:?}");

        // A clean stop after a clean opening leaves the counter as it stands.
        b.reopen_clean();
        assert_eq!(b.counter()[0].last_number, 2 * TASK_NUMBER_BLOCK + 1);
        let dump = b.store().dump();
        b.rebuild();
        assert_eq!(b.store().dump(), dump, "{engine:?}: rebuild parity");
    }
}

#[test]
fn concurrent_creates_after_an_unclean_stop_jump_past_the_reservation_once() {
    const CREATES: u64 = 6;
    for engine in ENGINES {
        let mut b = Bench::new(engine);
        b.create(Some(TEAM_B), None);
        b.reopen_after_crash();
        let store = Arc::new(b.store.take().expect("open"));
        let here = b.here.clone();
        let actor = b.actor();
        let threads: Vec<_> = (0..CREATES)
            .map(|n| {
                let (store, here, actor) = (Arc::clone(&store), here.clone(), actor.clone());
                std::thread::spawn(move || {
                    let numbering = store.task_numbering();
                    let out = store.run(TaskCreateOp {
                        actor,
                        here,
                        params: params::<TasksCreateParams>(json!({
                            "command_id": uid(0x100 + 2 * n).as_str(),
                            "task_id": uid(0x101 + 2 * n).as_str(),
                            "team_id": id(TEAM_B).as_str(),
                            "title": format!("Concurrent {n}"),
                            "body_markdown": "Body.\n",
                        })),
                        numbering,
                        prepared_files: Default::default(),
                    });
                    out.expect("stored").expect("created").task.number
                })
            })
            .collect();
        let mut numbers: Vec<u64> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        numbers.sort_unstable();
        let expected: Vec<u64> = (1..=CREATES).map(|n| TASK_NUMBER_BLOCK + n).collect();
        assert_eq!(numbers, expected, "{engine:?}: one jump, no block skipped");
        b.store = Some(Arc::into_inner(store).expect("the threads are done"));
        let counter = b.counter();
        assert_eq!(counter[0].last_number, TASK_NUMBER_BLOCK + CREATES);
        assert_eq!(counter[0].reserved_through, 2 * TASK_NUMBER_BLOCK);
    }
}

#[test]
fn a_missing_counter_beside_tasks_of_the_offices_code_refuses_to_create() {
    for engine in ENGINES {
        let mut b = Bench::new(engine);
        b.create(Some(TEAM_B), None);
        b.create(Some(TEAM_B), None);
        // The office-local counter is lost; the tasks of its code are not.
        write_counter(&b, None);
        assert!(b.counter().is_empty(), "{engine:?}");
        assert_eq!(
            b.try_create(Some(TEAM_B), None).unwrap_err(),
            ErrorCode::Unavailable,
            "{engine:?}: no number is reconstructed from the tasks"
        );
        b.rebuild();
        assert_eq!(
            b.try_create(Some(TEAM_B), None).unwrap_err(),
            ErrorCode::Unavailable,
            "{engine:?}: after a rebuild too"
        );
    }
}

#[test]
fn a_counter_behind_tasks_of_the_offices_code_refuses_to_create() {
    for engine in ENGINES {
        let mut b = Bench::new(engine);
        for _ in 0..3 {
            b.create(Some(TEAM_B), None);
        }
        let code = b.counter()[0].office_code.clone();
        let counter = |last_number, reserved_through| OfficeTaskCounterRecord {
            office_code: code.clone(),
            last_number,
            reserved_through,
        };
        // A counter restored from an older backup: its last number and its reservation are
        // behind the tasks, then only its last number is.
        for (last, reserved) in [(1, 2), (2, TASK_NUMBER_BLOCK)] {
            write_counter(&b, Some(counter(last, reserved)));
            assert_eq!(
                b.try_create(Some(TEAM_B), None).unwrap_err(),
                ErrorCode::Unavailable,
                "{engine:?}: counter {last}/{reserved} would reissue a number"
            );
        }
        // Caught up with the tasks, it numbers on.
        write_counter(&b, Some(counter(3, TASK_NUMBER_BLOCK)));
        assert_eq!(b.create(Some(TEAM_B), None).number, 4, "{engine:?}");
    }
}
