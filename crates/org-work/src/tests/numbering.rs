//! Per-project `T-n` numbers (§2.6).

use org_proto::ErrorCode;

use super::support::*;
use crate::WorkCall;

#[test]
fn each_project_has_its_own_task_sequence_and_numbers_are_never_reused() {
    let w = World::new();
    let a1 = w.task();
    let a2 = w.task();
    let mut p = w.create_params(OTHER_PROJECT);
    let b1 = p.item_id.clone();
    p.title = "Payments".into();
    w.ok(&app(), WorkCall::TaskCreate(p));
    assert_eq!(w.item(&a1).number, Some(1));
    assert_eq!(w.item(&a2).number, Some(2));
    assert_eq!(w.item(&b1).number, Some(1));
    w.ok(&app(), w.delete(&a2));
    assert_eq!(w.item(&w.task()).number, Some(3), "2 stays used");
}

#[test]
fn a_refused_create_uses_no_number() {
    let w = World::new();
    let mut p = w.create_params(PROJECT);
    p.title = String::new();
    assert_eq!(
        w.code(&app(), WorkCall::TaskCreate(p)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(w.item(&w.task()).number, Some(1));
}

#[test]
fn a_thousand_creates_get_a_thousand_consecutive_numbers() {
    let w = World::new();
    let mut numbers: Vec<u64> = (0..1000)
        .map(|_| {
            let p = w.create_params(PROJECT);
            w.ok(&app(), WorkCall::TaskCreate(p)).items[0]
                .number
                .unwrap()
        })
        .collect();
    numbers.sort_unstable();
    assert_eq!(numbers, (1..=1000).collect::<Vec<_>>());
}
