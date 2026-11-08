//! The task's state machine (§1.10): every (from, to) pair, allowed and refused.

use org_proto::work::*;
use org_proto::ErrorCode;

use super::support::*;
use crate::WorkCall;

use ItemStatus::*;

const STATUSES: [ItemStatus; 4] = [Todo, InProgress, Blocked, Done];

/// The design's table, written out (not derived from the code under test).
fn allowed(from: ItemStatus, to: ItemStatus) -> bool {
    matches!(
        (from, to),
        (Todo, InProgress | Blocked | Done)
            | (InProgress, Todo | Blocked | Done)
            | (Blocked, InProgress | Todo | Done)
            | (Done, Todo | InProgress)
    )
}

/// A task in `status`, reached through allowed moves.
fn task_in(w: &World, status: ItemStatus) -> String {
    let id = w.task();
    match status {
        Todo => {}
        Blocked | InProgress | Done => {
            w.set(&id, status);
        }
        _ => unreachable!(),
    }
    assert_eq!(w.item(&id).status, status);
    id
}

#[test]
fn every_move_is_allowed_or_refused_as_the_table_says() {
    for from in STATUSES {
        for to in STATUSES {
            let w = World::new();
            let id = task_in(&w, from);
            let before = w.item(&id);
            let events_before = w.events(&id).len();
            let p = w.status(&id, to);
            let out = w.call(&agent(), WorkCall::TaskSetStatus(p));
            if allowed(from, to) {
                let accepted = out.answer().expect("allowed move");
                assert_eq!(accepted.items[0].status, to, "{from:?} -> {to:?}");
                let events = w.events(&id);
                assert_eq!(
                    events.len(),
                    events_before + 1,
                    "{from:?} -> {to:?}: one event"
                );
                let e = events.last().unwrap();
                assert_eq!(e.actor_id.as_deref(), Some("ada"));
                assert!(!e.auto);
                assert!(matches!(
                    e.change,
                    WorkEventKind::StatusChanged { from: f, to: t, .. } if f == from && t == to
                ));
            } else {
                let e = out.answer().expect_err("refused move");
                assert_eq!(e.code, ErrorCode::InvalidRequest, "{from:?} -> {to:?}");
                assert_eq!(w.item(&id), before, "{from:?} -> {to:?} changed nothing");
                assert_eq!(w.events(&id).len(), events_before);
            }
        }
    }
}

#[test]
fn statuses_of_other_kinds_and_unknown_are_refused() {
    let w = World::new();
    let id = w.task();
    for s in [New, Triaged, Fixed, Closed, Scoped, ItemStatus::Unknown] {
        let p = w.status(&id, s);
        assert_eq!(
            w.code(&app(), WorkCall::TaskSetStatus(p)),
            ErrorCode::InvalidRequest
        );
    }
}

#[test]
fn reasons_go_exactly_where_they_belong() {
    let w = World::new();
    let id = w.task();
    // Blocked needs a reason.
    let mut p = w.status(&id, Blocked);
    p.blocked_reason = None;
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    // A reason with another status, a resolution or anyway reason without Done, a shortcut.
    let mut p = w.status(&id, InProgress);
    p.blocked_reason = Some("x".into());
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.status(&id, InProgress);
    p.resolution = Some(Resolution::Completed);
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.status(&id, InProgress);
    p.anyway_reason = Some("x".into());
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.status(&id, Done);
    p.resolution = Some(Resolution::Verified);
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.status(&id, Done);
    p.shortcut = Some(Shortcut::None);
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.status(&id, Blocked);
    p.blocked_reason = Some("two\nlines".into());
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(w.version(&id), 1, "nothing was recorded");
}

#[test]
fn moves_set_and_clear_what_goes_with_the_status() {
    let w = World::new();
    let id = w.task();
    w.set(&id, Blocked);
    assert_eq!(
        w.item(&id).task.blocked_reason.as_deref(),
        Some("waiting on CI")
    );
    assert!(matches!(
        w.last_event(&id).change,
        WorkEventKind::StatusChanged { reason: Some(ref r), .. } if r == "waiting on CI"
    ));
    w.set(&id, Todo);
    assert_eq!(w.item(&id).task.blocked_reason, None);
    w.set(&id, Done);
    assert_eq!(w.item(&id).resolution, Some(Resolution::Completed));
    // A reopen clears the resolution.
    w.set(&id, InProgress);
    let item = w.item(&id);
    assert_eq!((item.status, item.resolution), (InProgress, None));
    let mut p = w.status(&id, Done);
    p.resolution = Some(Resolution::Cancelled);
    w.ok(&app(), WorkCall::TaskSetStatus(p));
    assert_eq!(w.item(&id).resolution, Some(Resolution::Cancelled));
}

#[test]
fn a_task_call_on_another_kind_is_refused() {
    let w = World::new();
    let id = w.task();
    // Make it a bug behind the library's back (phase 1 has no bug calls yet).
    w.store
        .work
        .lock()
        .unwrap()
        .items
        .get_mut(&id)
        .unwrap()
        .kind = ItemKind::Bug;
    let p = w.status(&id, InProgress);
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    let mut u = w.update(&id);
    u.title = Some("x".into());
    assert_eq!(
        w.code(&app(), WorkCall::TaskUpdate(u)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn a_deleted_task_takes_no_more_moves() {
    let w = World::new();
    let id = w.task();
    w.ok(&app(), w.delete(&id));
    let p = w.status(&id, InProgress);
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn create_starts_in_todo_with_the_caller_as_creator_and_watcher() {
    let w = World::new();
    let p = w.create_params(PROJECT);
    let id = p.item_id.clone();
    let accepted = w.ok(&agent(), WorkCall::TaskCreate(p));
    let s = &accepted.items[0];
    assert_eq!((s.status, s.version, s.number), (Todo, 1, Some(1)));
    assert_eq!(s.created_by, "ada");
    assert_eq!(s.priority, Some(Priority::Normal));
    assert_eq!(s.owner_office_id, "hq");
    let item = w.item(&id);
    assert_eq!(item.watchers, vec!["ada".to_owned()]);
    let e = w.last_event(&id);
    assert_eq!((e.version, e.actor_id.as_deref()), (1, Some("ada")));
    assert_eq!(e.change, WorkEventKind::Created);
}
