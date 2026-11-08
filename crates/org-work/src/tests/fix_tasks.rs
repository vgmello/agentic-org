//! A bug follows its fix tasks (§2.4): the automatic In progress and Fixed, back to In progress
//! when a fix task is reopened, and Undo; `fixes` links and the summaries that show them.

use org_proto::work::*;
use org_proto::ErrorCode;

use super::support::*;
use crate::WorkCall;

fn undo(w: &World, id: &str, version: u64) -> WorkCall {
    WorkCall::Undo(WorkUndoParams {
        change_id: w.id(),
        target_id: id.into(),
        event_version: version,
    })
}

fn cancel(w: &World, task: &str) {
    let mut p = w.status(task, ItemStatus::Done);
    p.resolution = Some(Resolution::Cancelled);
    w.ok(&app(), WorkCall::TaskSetStatus(p));
}

fn link_fixes(w: &World, task: &str, bug: &str) -> WorkCall {
    WorkCall::Link(WorkLinkParams {
        change_id: w.id(),
        link_id: w.id(),
        from_item_id: task.into(),
        kind: LinkKind::Fixes,
        to: LinkTarget::Item {
            item_id: bug.into(),
        },
    })
}

// ---- the links ---------------------------------------------------------------------------

#[test]
fn a_task_created_with_fixes_links_to_the_bug_and_both_summaries_show_it() {
    let w = World::new();
    let bug = w.bug();
    let link_id = w.id();
    let task = w.task_with(|p| {
        p.fixes = vec![NewFixLink {
            link_id: link_id.clone(),
            bug_id: bug.clone(),
        }]
    });
    let t = w.item(&task);
    assert_eq!(t.fixes(), std::slice::from_ref(&bug));
    assert_eq!(t.summary().fixes, std::slice::from_ref(&bug));
    let l = t.live_links().next().expect("a link");
    assert_eq!((l.id.as_str(), l.kind), (link_id.as_str(), LinkKind::Fixes));
    assert_eq!(l.created_by, OWNER);
    // The task is created to do: the bug doesn't move yet.
    assert_eq!(w.item(&bug).status, ItemStatus::New);

    // The bug's summary lists its fix tasks (in the answer of a call on the bug).
    let p = w.bug_assign(&bug, Some("riley"));
    let accepted = w.ok(&app(), WorkCall::BugAssign(p));
    assert_eq!(accepted.items[0].fixed_by, std::slice::from_ref(&task));
    let second = w.fix_task(&bug);
    let p = w.bug_assign(&bug, None);
    let accepted = w.ok(&app(), WorkCall::BugAssign(p));
    let mut fixed_by = accepted.items[0].fixed_by.clone();
    fixed_by.sort();
    let mut want = vec![task.clone(), second.clone()];
    want.sort();
    assert_eq!(fixed_by, want);

    // A deleted fix task drops out of the list.
    w.ok(&app(), w.delete(&second));
    let p = w.bug_assign(&bug, Some("riley"));
    let accepted = w.ok(&app(), WorkCall::BugAssign(p));
    assert_eq!(accepted.items[0].fixed_by, [task]);
}

#[test]
fn fixes_refusals_on_create() {
    let w = World::new();
    let bug = w.bug();
    let other_bug = {
        let p = w.report_params(OTHER_PROJECT);
        let id = p.item_id.clone();
        w.ok(&pat(), WorkCall::BugReport(p));
        id
    };
    let task = w.task();
    let deleted = w.bug();
    w.ok(&app(), w.delete(&deleted));
    let l = || w.id();
    type Edit = Box<dyn Fn(&mut TaskCreateParams)>;
    let fix = |bug_id: &str| NewFixLink {
        link_id: w.id(),
        bug_id: bug_id.to_owned(),
    };
    let cases: Vec<(Edit, ErrorCode, &str)> = vec![
        (
            Box::new({
                let f = fix("nope");
                move |p| p.fixes = vec![f.clone()]
            }),
            ErrorCode::NotFound,
            "no such bug",
        ),
        (
            Box::new({
                let f = fix(&other_bug);
                move |p| p.fixes = vec![f.clone()]
            }),
            ErrorCode::InvalidRequest,
            "another project",
        ),
        (
            Box::new({
                let f = fix(&task);
                move |p| p.fixes = vec![f.clone()]
            }),
            ErrorCode::InvalidRequest,
            "a task",
        ),
        (
            Box::new({
                let f = fix(&deleted);
                move |p| p.fixes = vec![f.clone()]
            }),
            ErrorCode::InvalidRequest,
            "deleted",
        ),
        (
            Box::new({
                let (a, b) = (fix(&bug), fix(&bug));
                move |p| p.fixes = vec![a.clone(), b.clone()]
            }),
            ErrorCode::InvalidRequest,
            "the same bug twice",
        ),
        (
            Box::new({
                let f = NewFixLink {
                    link_id: "not-a-ulid".into(),
                    bug_id: bug.clone(),
                };
                move |p| p.fixes = vec![f.clone()]
            }),
            ErrorCode::InvalidRequest,
            "link id",
        ),
        (
            Box::new({
                let id = l();
                let f = NewFixLink {
                    link_id: id.clone(),
                    bug_id: bug.clone(),
                };
                move |p| {
                    p.fixes = vec![f.clone()];
                    p.item_id = id.clone();
                }
            }),
            ErrorCode::InvalidRequest,
            "link id equal to the item id",
        ),
    ];
    for (f, code, what) in cases {
        let mut p = w.create_params(PROJECT);
        f(&mut p);
        assert_eq!(w.code(&app(), WorkCall::TaskCreate(p)), code, "{what}");
    }
    // A taken link id.
    let taken = w
        .item(&w.fix_task(&bug))
        .live_links()
        .next()
        .expect("link")
        .id
        .clone();
    let mut p = w.create_params(PROJECT);
    p.fixes = vec![NewFixLink {
        link_id: taken,
        bug_id: bug,
    }];
    assert_eq!(
        w.code(&app(), WorkCall::TaskCreate(p)),
        ErrorCode::AlreadyExists
    );
}

#[test]
fn work_link_fixes_goes_from_a_task_to_a_bug_of_its_project() {
    let w = World::new();
    let bug = w.bug();
    let task = w.task();
    let other_bug = {
        let p = w.report_params(OTHER_PROJECT);
        let id = p.item_id.clone();
        w.ok(&pat(), WorkCall::BugReport(p));
        id
    };
    w.ok(&agent(), link_fixes(&w, &task, &bug));
    assert_eq!(w.item(&task).fixes(), std::slice::from_ref(&bug));

    assert_eq!(
        w.code(&agent(), link_fixes(&w, &task, &bug)),
        ErrorCode::AlreadyExists
    );
    // From a bug, to a task, to another project's bug, to a doc.
    assert_eq!(
        w.code(&agent(), link_fixes(&w, &bug, &bug)),
        ErrorCode::InvalidRequest
    );
    let other_task = w.task();
    assert_eq!(
        w.code(&agent(), link_fixes(&w, &task, &other_task)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        w.code(&agent(), link_fixes(&w, &task, &other_bug)),
        ErrorCode::InvalidRequest
    );
    let doc = WorkCall::Link(WorkLinkParams {
        change_id: w.id(),
        link_id: w.id(),
        from_item_id: task.clone(),
        kind: LinkKind::Fixes,
        to: LinkTarget::Doc {
            project_id: PROJECT.into(),
            doc_key: "ADR-0001".into(),
        },
    });
    assert_eq!(w.code(&agent(), doc), ErrorCode::InvalidRequest);
    let gone = w.bug();
    w.ok(&app(), w.delete(&gone));
    assert_eq!(
        w.code(&agent(), link_fixes(&w, &task, &gone)),
        ErrorCode::InvalidRequest
    );
}

// ---- Bug -> In progress ------------------------------------------------------------------

#[test]
fn the_first_fix_task_to_start_moves_a_triaged_bug_to_in_progress() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    let v = w.version(&bug);

    let accepted = w.set(&task, ItemStatus::InProgress);
    assert_eq!(accepted.items.len(), 2, "the task and the bug");
    let b = w.item(&bug);
    assert_eq!(b.status, ItemStatus::InProgress);
    let e = w.last_event(&bug);
    assert_eq!(e.version, v + 1);
    assert!(e.auto);
    assert_eq!(e.actor_id, None);
    assert_eq!(
        e.cause,
        Some(EventRef {
            target_id: task.clone(),
            version: w.version(&task)
        }),
        "caused by the task's status event"
    );
    assert_eq!(
        e.change,
        WorkEventKind::StatusChanged {
            from: ItemStatus::Triaged,
            to: ItemStatus::InProgress,
            resolution: None,
            reason: None,
            anyway: None
        }
    );

    // A second task starting changes nothing more.
    let second = w.fix_task(&bug);
    let before = w.version(&bug);
    w.set(&second, ItemStatus::InProgress);
    assert_eq!(w.version(&bug), before);
}

#[test]
fn a_new_bug_waits_for_triage_and_a_task_in_progress_then_counts_at_triage() {
    let w = World::new();
    let bug = w.bug();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::InProgress);
    assert_eq!(
        w.item(&bug).status,
        ItemStatus::New,
        "not triaged: no change"
    );

    let v = w.version(&bug);
    w.triage(&bug);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
    assert_eq!(w.kinds_after(&bug, v), ["triaged", "auto:status_changed"]);
    let auto = w.last_event(&bug);
    assert_eq!(
        auto.cause.expect("cause").version,
        v + 1,
        "caused by the triage"
    );
}

#[test]
fn linking_a_task_already_in_progress_starts_the_bug() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.task();
    w.set(&task, ItemStatus::InProgress);
    w.ok(&agent(), link_fixes(&w, &task, &bug));
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
    let e = w.last_event(&bug);
    assert!(e.auto);
    assert_eq!(e.cause.expect("cause").target_id, task);

    // A task that is only to do doesn't.
    let bug2 = w.triaged();
    let todo = w.task();
    w.ok(&agent(), link_fixes(&w, &todo, &bug2));
    assert_eq!(w.item(&bug2).status, ItemStatus::Triaged);
}

#[test]
fn a_blocked_or_todo_fix_task_does_nothing() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    let v = w.version(&bug);
    w.set(&task, ItemStatus::Blocked);
    w.set(&task, ItemStatus::Todo);
    assert_eq!(w.version(&bug), v);
}

// ---- Bug -> Fixed ------------------------------------------------------------------------

#[test]
fn the_bug_is_fixed_only_when_every_fix_task_is_done_and_one_completed() {
    let w = World::new();
    let bug = w.triaged();
    let (a, b) = (w.fix_task(&bug), w.fix_task(&bug));
    w.set(&a, ItemStatus::InProgress);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);

    w.set(&a, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress, "b is not done");
    w.set(&b, ItemStatus::InProgress);
    let v = w.version(&bug);
    let accepted = w.set(&b, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
    assert_eq!(
        accepted
            .items
            .iter()
            .find(|i| i.id == bug)
            .expect("bug")
            .status,
        ItemStatus::Fixed
    );
    let e = w.last_event(&bug);
    assert_eq!(e.version, v + 1);
    assert!(e.auto);
    assert_eq!(e.cause.expect("cause").target_id, b);
    // Nothing was typed in: no note, no "what fixed it".
    assert_eq!(w.item(&bug).bug.closed_reason, None);
}

#[test]
fn a_triaged_bug_whose_only_fix_task_goes_straight_to_done_is_fixed() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
}

#[test]
fn cancelled_fix_tasks_fix_nothing() {
    let w = World::new();
    let bug = w.triaged();
    let (a, b) = (w.fix_task(&bug), w.fix_task(&bug));
    let v = w.version(&bug);
    cancel(&w, &a);
    cancel(&w, &b);
    assert_eq!(
        w.item(&bug).status,
        ItemStatus::Triaged,
        "all cancelled: no change"
    );
    assert_eq!(w.version(&bug), v, "and no event");

    // One cancelled and one completed is fixed.
    let c = w.fix_task(&bug);
    w.set(&c, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
}

#[test]
fn deleted_fix_tasks_are_ignored_when_counting() {
    let w = World::new();
    let bug = w.triaged();
    let (a, b) = (w.fix_task(&bug), w.fix_task(&bug));
    let v = w.version(&bug);
    w.ok(&app(), w.delete(&b));
    assert_eq!(w.version(&bug), v, "deleting a fix task is no trigger");
    w.set(&a, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
}

#[test]
fn unlinking_or_deleting_a_fix_task_changes_nothing() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::InProgress);
    let v = w.version(&bug);
    let link = w.item(&task).live_links().next().expect("link").id.clone();
    w.ok(
        &app(),
        WorkCall::Unlink(WorkUnlinkParams {
            change_id: w.id(),
            link_id: link,
        }),
    );
    assert_eq!(w.version(&bug), v);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);

    let done_bug = w.bug_at(ItemStatus::Fixed);
    let t = w.fix_task(&done_bug);
    let v = w.version(&done_bug);
    w.ok(&app(), w.delete(&t));
    assert_eq!(w.version(&done_bug), v);
    assert_eq!(w.item(&done_bug).status, ItemStatus::Fixed);
}

#[test]
fn fixed_stays_when_a_required_bug_step_is_left_and_an_event_says_so() {
    let w = World::new();
    let step_id = w.id();
    let bug = w.bug_with(|p| {
        p.area_id = Some(AREA_PAY.into());
        p.checklist = vec![NewStep {
            step_id: step_id.clone(),
            text: "Add a regression test".into(),
            required: true,
        }];
    });
    w.triage(&bug);
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::InProgress);
    let v = w.version(&bug);
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress, "no change");
    let e = w.last_event(&bug);
    assert_eq!(e.version, v + 1, "but it is recorded");
    assert!(e.auto);
    assert_eq!(
        e.change,
        WorkEventKind::StatusChanged {
            from: ItemStatus::InProgress,
            to: ItemStatus::InProgress,
            resolution: None,
            reason: Some("all fix tasks are done; required steps are left".into()),
            anyway: None,
        }
    );
    // There is nothing to undo in it.
    assert_eq!(
        w.code(&app(), undo(&w, &bug, e.version)),
        ErrorCode::InvalidRequest
    );

    // The tick is not an edge either: the rule runs when a fix task changes again.
    w.ok(&app(), w.check(&step_id, true));
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
    w.set(&task, ItemStatus::InProgress);
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
}

#[test]
fn triaging_a_bug_whose_fix_tasks_are_already_done_fixes_it() {
    let w = World::new();
    let bug = w.bug();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::New);
    let v = w.version(&bug);
    w.triage(&bug);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
    assert_eq!(w.kinds_after(&bug, v), ["triaged", "auto:status_changed"]);
}

// ---- Fixed -> In progress ----------------------------------------------------------------

#[test]
fn reopening_a_fix_task_takes_a_fixed_bug_back_to_in_progress() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);

    w.set(&task, ItemStatus::InProgress);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
    let e = w.last_event(&bug);
    assert!(e.auto);
    assert_eq!(
        e.change,
        WorkEventKind::StatusChanged {
            from: ItemStatus::Fixed,
            to: ItemStatus::InProgress,
            resolution: None,
            reason: None,
            anyway: None
        }
    );
    // And to todo.
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
    w.set(&task, ItemStatus::Todo);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
}

#[test]
fn a_bug_fixed_by_hand_goes_back_when_a_fix_task_is_reopened() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::Done);
    w.set(&task, ItemStatus::InProgress);
    let p = w.fixed_params(&bug);
    w.ok(&app(), WorkCall::BugMarkFixed(p));
    w.set(&task, ItemStatus::Done);
    w.set(&task, ItemStatus::Todo);
    let b = w.item(&bug);
    assert_eq!(b.status, ItemStatus::InProgress);
    assert_eq!(
        b.bug.closed_reason, None,
        "no longer 'fixed': its note goes"
    );
}

#[test]
fn still_happens_and_a_reopen_are_not_fixed_again_by_old_fix_tasks() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
    let p = w.verify_params(&bug, VerifyOutcome::StillHappens);
    w.ok(&pat(), WorkCall::BugVerify(p));
    assert_eq!(w.item(&bug).status, ItemStatus::Triaged);
    // Another unrelated change to the done task is no edge for the bug.
    let v = w.version(&bug);
    w.ok(
        &app(),
        WorkCall::TaskUpdate({
            let mut p = w.update(&task);
            p.title = Some("Fix the coupon, again".into());
            p
        }),
    );
    assert_eq!(w.version(&bug), v);

    // After a close and a reopen it is Triaged as well, and stays so.
    let closed = w.bug_at(ItemStatus::Closed);
    let t = w.fix_task(&closed);
    w.set(&t, ItemStatus::Done);
    let p = w.reopen_params(&closed);
    w.ok(&app(), WorkCall::BugReopen(p));
    assert_eq!(w.item(&closed).status, ItemStatus::Triaged);
}

#[test]
fn a_task_that_fixes_two_bugs_moves_both() {
    let w = World::new();
    let (a, b) = (w.triaged(), w.triaged());
    let task = w.fix_task(&a);
    w.ok(&agent(), link_fixes(&w, &task, &b));
    let accepted = w.set(&task, ItemStatus::InProgress);
    assert_eq!(accepted.items.len(), 3);
    assert_eq!(w.item(&a).status, ItemStatus::InProgress);
    assert_eq!(w.item(&b).status, ItemStatus::InProgress);
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&a).status, ItemStatus::Fixed);
    assert_eq!(w.item(&b).status, ItemStatus::Fixed);
}

#[test]
fn a_task_made_from_the_bugs_own_step_that_also_fixes_it_changes_the_bug_once() {
    let w = World::new();
    let step = w.id();
    let bug = w.bug_with(|p| {
        p.area_id = Some(AREA_PAY.into());
        p.checklist = vec![NewStep {
            step_id: step.clone(),
            text: "Cover it".into(),
            required: true,
        }];
    });
    w.triage(&bug);
    let (task, call) = w.make_task(&step);
    w.ok(&app(), call);
    w.ok(&agent(), link_fixes(&w, &task, &bug));
    w.set(&task, ItemStatus::InProgress);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
    let v = w.version(&bug);
    // One call: the step ticks and the bug is fixed (its required step is no longer left),
    // as two events on the same bug at consecutive versions.
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
    assert_eq!(
        w.kinds_after(&bug, v),
        ["auto:step_checked", "auto:status_changed"]
    );
    assert_eq!(w.version(&bug), v + 2);
}

// ---- Undo --------------------------------------------------------------------------------

#[test]
fn undo_puts_a_bug_back_and_is_not_applied_again_without_a_new_trigger() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::InProgress);
    let auto = w.version(&bug);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);

    let accepted = w.ok(&pat(), undo(&w, &bug, auto));
    assert_eq!(accepted.items[0].status, ItemStatus::Triaged);
    assert_eq!(w.item(&bug).status, ItemStatus::Triaged);
    let e = w.last_event(&bug);
    assert_eq!(
        e.change,
        WorkEventKind::Undone {
            event_version: auto
        }
    );
    assert_eq!(e.actor_id.as_deref(), Some("pat"));
    assert_eq!(
        e.cause,
        Some(EventRef {
            target_id: bug.clone(),
            version: auto
        })
    );
    assert!(!e.auto);

    // Nothing re-applies it: the task is still in progress, but that edge is past.
    let other = w.task();
    w.set(&other, ItemStatus::InProgress);
    assert_eq!(w.item(&bug).status, ItemStatus::Triaged);
    // A new edge does.
    w.set(&task, ItemStatus::Blocked);
    w.set(&task, ItemStatus::InProgress);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);

    // It can't be undone twice.
    assert_eq!(
        w.code(&pat(), undo(&w, &bug, auto)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn undoing_fixed_goes_back_and_undoing_a_reopen_goes_back_to_fixed() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::InProgress);
    w.set(&task, ItemStatus::Done);
    let fixed_at = w.version(&bug);
    assert_eq!(w.item(&bug).status, ItemStatus::Fixed);
    w.ok(&app(), undo(&w, &bug, fixed_at));
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);

    // Fixed again by hand with a note, reopened by its task, and the reopen undone.
    let p = w.fixed_params(&bug);
    w.ok(&app(), WorkCall::BugMarkFixed(p));
    w.set(&task, ItemStatus::Todo);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
    let auto = w.version(&bug);
    w.ok(&app(), undo(&w, &bug, auto));
    let b = w.item(&bug);
    assert_eq!(b.status, ItemStatus::Fixed);
    assert_eq!(
        b.bug.closed_reason.as_deref(),
        Some("Rounded the total"),
        "its note is back"
    );
}

#[test]
fn undo_refusals_for_a_bug() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::InProgress);
    let auto = w.version(&bug);

    // Not automatic: the triage.
    assert_eq!(
        w.code(&app(), undo(&w, &bug, auto - 1)),
        ErrorCode::InvalidRequest
    );
    // A later change to the status makes it stale.
    let p = w.fixed_params(&bug);
    w.ok(&app(), WorkCall::BugMarkFixed(p));
    assert_eq!(w.code(&app(), undo(&w, &bug, auto)), ErrorCode::Conflict);

    // After an hour.
    let bug2 = w.triaged();
    let task2 = w.fix_task(&bug2);
    w.set(&task2, ItemStatus::InProgress);
    let auto2 = w.version(&bug2);
    w.clock.advance(UNDO_WINDOW + 1);
    assert_eq!(
        w.code(&app(), undo(&w, &bug2, auto2)),
        ErrorCode::InvalidRequest
    );
}

const UNDO_WINDOW: u64 = crate::UNDO_WINDOW_MS;

#[test]
fn a_comment_after_the_automatic_change_does_not_block_its_undo() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::InProgress);
    let auto = w.version(&bug);
    w.ok(&app(), WorkCall::Comment(w.comment(&bug, "On it")));
    w.ok(&app(), undo(&w, &bug, auto));
    assert_eq!(w.item(&bug).status, ItemStatus::Triaged);
}

// ---- one transaction ---------------------------------------------------------------------

#[test]
fn a_task_and_its_bug_commit_together_or_not_at_all() {
    use crate::testkit::FailPoint;
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    let before = w.store.snapshot();

    w.store.fail_next(FailPoint::RecordChange);
    let p = w.status(&task, ItemStatus::InProgress);
    assert!(
        w.svc.call(app(), WorkCall::TaskSetStatus(p)).is_err(),
        "the storage error reaches the caller"
    );
    assert_eq!(
        w.store.snapshot(),
        before,
        "neither the task nor the bug moved"
    );

    // Retried, both move, once.
    w.set(&task, ItemStatus::InProgress);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
    assert_eq!(w.kinds_after(&bug, 2), ["auto:status_changed"]);
}

#[test]
fn a_rerun_batch_gives_the_same_bug_outcome_without_a_double_write() {
    let w = World::new();
    let bug = w.triaged();
    let task = w.fix_task(&bug);
    w.store.rerun_next();
    w.set(&task, ItemStatus::InProgress);
    assert_eq!(w.item(&bug).status, ItemStatus::InProgress);
    assert_eq!(w.kinds_after(&bug, 2), ["auto:status_changed"]);
    assert_eq!(w.version(&task), 2);
}
