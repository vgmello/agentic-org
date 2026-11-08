//! `change_id` idempotency (§2.3), the age checks, app-only calls, sizes and re-runs.

use org_proto::work::*;
use org_proto::ErrorCode;

use super::support::*;
use crate::testkit::FailPoint;
use crate::{ulid_from_parts, TxnOutput, WorkCall};

#[test]
fn a_retry_gets_the_stored_answer_even_after_the_item_changed() {
    let w = World::new();
    let id = w.task();
    let p = w.status(&id, ItemStatus::InProgress);
    let first = w.ok(&agent(), WorkCall::TaskSetStatus(p.clone()));
    w.set(&id, ItemStatus::Blocked);
    let n = w.events(&id).len();
    match w.call(&agent(), WorkCall::TaskSetStatus(p)) {
        TxnOutput::Replayed(Ok(again)) => assert_eq!(again, first),
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(w.events(&id).len(), n, "a replay records nothing");
}

#[test]
fn the_same_change_id_with_other_params_or_another_member_is_refused() {
    let w = World::new();
    let id = w.task();
    let p = w.status(&id, ItemStatus::InProgress);
    w.ok(&agent(), WorkCall::TaskSetStatus(p.clone()));
    let mut other = p.clone();
    other.status = ItemStatus::Blocked;
    other.blocked_reason = Some("x".into());
    let e = w.err(&agent(), WorkCall::TaskSetStatus(other));
    assert_eq!(e.code, ErrorCode::InvalidRequest);
    assert!(e.message.contains("reused"));
    assert_eq!(
        w.code(&Caller::agent("riley"), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
}

use crate::Caller;

#[test]
fn a_refusal_is_answered_from_the_store_after_the_item_changed() {
    let w = World::new();
    let id = w.task();
    let mut stale = w.status(&id, ItemStatus::InProgress);
    stale.expected_version = 1;
    w.set(&id, ItemStatus::Blocked);
    let e = w.err(&agent(), WorkCall::TaskSetStatus(stale.clone()));
    assert_eq!(e.code, ErrorCode::Conflict);
    // Even once the move would be allowed on its own, the retry gets the same refusal.
    w.set(&id, ItemStatus::Todo);
    match w.call(&agent(), WorkCall::TaskSetStatus(stale)) {
        TxnOutput::Replayed(Err(again)) => assert_eq!(again, e),
        other => panic!("expected a replayed refusal, got {other:?}"),
    }
}

#[test]
fn age_checks_come_after_the_lookup() {
    let w = World::new();
    let id = w.task();
    // A change id made 29.5 days ago: new ones are expired, a stored one is still answered.
    let old_ms = w.now();
    let mut stored = w.status(&id, ItemStatus::InProgress);
    stored.change_id = ulid_from_parts(old_ms, 77);
    w.ok(&agent(), WorkCall::TaskSetStatus(stored.clone()));
    w.clock.advance(29 * DAY + 12 * HOUR);
    assert!(matches!(
        w.call(&agent(), WorkCall::TaskSetStatus(stored)),
        TxnOutput::Replayed(Ok(_))
    ));
    let mut fresh = w.status(&id, ItemStatus::Todo);
    fresh.change_id = ulid_from_parts(old_ms, 78);
    assert_eq!(
        w.code(&agent(), WorkCall::TaskSetStatus(fresh.clone())),
        ErrorCode::Expired
    );
    assert!(
        !w.store.snapshot().results.contains_key(&fresh.change_id),
        "not stored"
    );

    // 6 minutes ahead is clock skew, and isn't stored either: once the clock catches up the
    // same call succeeds. 4 minutes ahead is fine.
    let mut ahead = w.status(&id, ItemStatus::Todo);
    ahead.change_id = ulid_from_parts(w.now() + 6 * MINUTE, 1);
    assert_eq!(
        w.code(&agent(), WorkCall::TaskSetStatus(ahead.clone())),
        ErrorCode::ClockSkew
    );
    w.clock.advance(2 * MINUTE);
    w.ok(&agent(), WorkCall::TaskSetStatus(ahead));
    let mut near = w.status(&id, ItemStatus::InProgress);
    near.change_id = ulid_from_parts(w.now() + 4 * MINUTE, 2);
    w.ok(&agent(), WorkCall::TaskSetStatus(near));
}

#[test]
fn a_change_id_that_isnt_a_ulid_is_invalid() {
    let w = World::new();
    let mut p = w.create_params(PROJECT);
    p.change_id = "not-a-ulid".into();
    assert_eq!(
        w.code(&app(), WorkCall::TaskCreate(p)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn a_storage_error_rolls_back_and_is_not_stored() {
    for point in [
        FailPoint::PutItem,
        FailPoint::RecordChange,
        FailPoint::PutChangeResult,
    ] {
        let w = World::new();
        let p = w.create_params(PROJECT);
        let before = w.store.snapshot();
        w.store.fail_next(point);
        assert!(w.svc.call(app(), WorkCall::TaskCreate(p.clone())).is_err());
        assert_eq!(w.store.snapshot(), before, "{point:?}: nothing kept");
        // The retry succeeds, with the number the failed attempt didn't keep.
        let a = w.ok(&app(), WorkCall::TaskCreate(p));
        assert_eq!(a.items[0].number, Some(1));
    }
}

#[test]
fn a_re_run_by_the_writer_gives_the_same_answer_once() {
    let w = World::new();
    let (id, step) = w.task_with_step(true);
    let (task, call) = w.make_task(&step);
    w.store.rerun_next();
    let before = w.store.attempts();
    let a = w.ok(&agent(), call);
    assert_eq!(w.store.attempts(), before + 2, "the body ran twice");
    assert_eq!(a.items.len(), 2);
    assert_eq!(w.item(&task).number, Some(2), "one number, not two");
    assert_eq!(
        w.store.snapshot().feed.len(),
        2,
        "one feed row per committed call"
    );
    assert_eq!(w.events(&id).len(), 2);
}

#[test]
fn delete_over_an_agent_connection_is_not_authorized_before_any_other_check() {
    let w = World::new();
    let id = w.task();
    let call = WorkCall::Delete(WorkDeleteParams {
        change_id: "not-even-a-ulid".into(),
        item_id: w.id(),
        expected_version: 99,
    });
    assert_eq!(w.code(&agent(), call), ErrorCode::NotAuthorized);
    assert_eq!(w.code(&agent(), w.delete(&id)), ErrorCode::NotAuthorized);
    assert!(!w.item(&id).deleted);
    w.ok(&app(), w.delete(&id));
    assert!(w.item(&id).deleted);
}

#[test]
fn too_large_comes_after_not_found_and_before_everything_else_and_is_not_stored() {
    let w = World::new();
    let huge = "x".repeat(WORK_PARAMS_MAX_BYTES + 1);
    let mut p = w.create_params(PROJECT);
    p.description = Some(huge.clone());
    let change_id = p.change_id.clone();
    assert_eq!(w.code(&app(), WorkCall::TaskCreate(p)), ErrorCode::TooLarge);
    assert!(!w.store.snapshot().results.contains_key(&change_id));
    let mut p = w.create_params("no-such-project");
    p.description = Some(huge);
    assert_eq!(w.code(&app(), WorkCall::TaskCreate(p)), ErrorCode::NotFound);
}
