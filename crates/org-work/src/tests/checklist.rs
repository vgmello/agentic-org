//! Checklists: the required-step guard, made-task steps following their task, Undo, and the
//! step calls with their own versions.

use org_proto::work::*;
use org_proto::ErrorCode;

use super::support::*;
use crate::WorkCall;

use ItemStatus::*;

#[test]
fn done_with_a_required_step_left_is_checklist_incomplete_and_changes_nothing() {
    let w = World::new();
    let (id, _) = w.task_with_step(true);
    let before = w.item(&id);
    let e = w.err(&agent(), WorkCall::TaskSetStatus(w.status(&id, Done)));
    assert_eq!(e.code, ErrorCode::ChecklistIncomplete);
    assert!(
        e.message.contains("Write the tests"),
        "names the step: {}",
        e.message
    );
    assert_eq!(w.item(&id), before);
}

#[test]
fn done_anyway_records_the_reason_and_the_steps() {
    let w = World::new();
    let (id, step) = w.task_with_step(true);
    let mut p = w.status(&id, Done);
    p.anyway_reason = Some("covered by the e2e suite".into());
    let accepted = w.ok(&agent(), WorkCall::TaskSetStatus(p));
    assert!(accepted.items[0].done_anyway);
    let detail = w.item(&id).task.done_anyway.unwrap();
    assert_eq!(detail.reason, "covered by the e2e suite");
    assert_eq!(detail.steps, vec![step]);
    assert!(matches!(
        w.last_event(&id).change,
        WorkEventKind::StatusChanged {
            anyway: Some(_),
            ..
        }
    ));
    // A reopen clears it.
    w.set(&id, Todo);
    assert_eq!(w.item(&id).task.done_anyway, None);
}

#[test]
fn cancelled_skips_the_guard_and_optional_steps_never_block() {
    let w = World::new();
    let (id, _) = w.task_with_step(true);
    let mut p = w.status(&id, Done);
    p.resolution = Some(Resolution::Cancelled);
    w.ok(&agent(), WorkCall::TaskSetStatus(p));
    assert_eq!(w.item(&id).task.done_anyway, None);

    let (id, _) = w.task_with_step(false);
    w.set(&id, Done);
    // A ticked required step doesn't block either.
    let (id, step) = w.task_with_step(true);
    w.ok(&agent(), w.check(&step, true));
    w.set(&id, Done);
}

#[test]
fn a_tick_records_an_event_only_when_it_changes_something() {
    let w = World::new();
    let (id, step) = w.task_with_step(false);
    let a = w.ok(&agent(), w.check(&step, true));
    assert!(a.position.is_some());
    let s = w.step(&id, &step);
    assert!(s.done);
    assert_eq!(s.checked_by.as_deref(), Some("ada"));
    assert_eq!(s.version, 2);
    let n = w.events(&id).len();
    let again = w.ok(&agent(), w.check(&step, true));
    assert_eq!(again.position, None, "no change, no position");
    assert_eq!(again.items[0].checklist.done, 1);
    assert_eq!(w.events(&id).len(), n);
}

#[test]
fn a_made_task_step_ticks_only_on_done_completed_and_unticks_on_reopen() {
    let w = World::new();
    let (parent, step) = w.task_with_step(true);
    let (task, call) = w.make_task(&step);
    let accepted = w.ok(&agent(), call);
    assert_eq!(accepted.items.len(), 2, "both items are in the answer");
    assert_eq!(
        w.step(&parent, &step).task_id.as_deref(),
        Some(task.as_str())
    );
    let made = w.item(&task);
    assert_eq!(made.title, "Write the tests");
    assert_eq!(made.number, Some(2));
    assert!(
        matches!(&made.part_of().unwrap().to, LinkTarget::Item { item_id } if *item_id == parent)
    );

    // It can't be ticked by hand.
    assert_eq!(
        w.code(&agent(), w.check(&step, true)),
        ErrorCode::InvalidRequest
    );

    // Done · Cancelled leaves it unticked.
    let mut p = w.status(&task, Done);
    p.resolution = Some(Resolution::Cancelled);
    w.ok(&agent(), WorkCall::TaskSetStatus(p));
    assert!(!w.step(&parent, &step).done);

    // Reopen, then Done · Completed ticks it, automatically, caused by the task's event.
    w.set(&task, InProgress);
    let parent_v = w.version(&parent);
    let accepted = w.set(&task, Done);
    assert_eq!(accepted.items.len(), 2);
    assert!(w.step(&parent, &step).done);
    let auto = w.last_event(&parent);
    assert_eq!(auto.version, parent_v + 1);
    assert!(auto.auto);
    assert_eq!(auto.actor_id, None);
    assert_eq!(
        auto.cause,
        Some(EventRef {
            target_id: task.clone(),
            version: w.version(&task)
        })
    );
    // So the parent can be done now.
    w.set(&parent, Done);

    // Reopening the made task unticks the step.
    w.set(&parent, Todo);
    w.set(&task, Todo);
    assert!(!w.step(&parent, &step).done);
}

fn auto_ticked(w: &World) -> (String, String, String, u64) {
    let (parent, step) = w.task_with_step(true);
    let (task, call) = w.make_task(&step);
    w.ok(&agent(), call);
    w.set(&task, Done);
    let v = w.version(&parent);
    assert!(w.events(&parent).last().unwrap().auto);
    (parent, step, task, v)
}

fn undo(w: &World, target: &str, version: u64) -> WorkCall {
    WorkCall::Undo(WorkUndoParams {
        change_id: w.id(),
        target_id: target.into(),
        event_version: version,
    })
}

#[test]
fn undo_restores_an_automatic_tick_and_is_not_re_applied_until_a_new_trigger() {
    let w = World::new();
    let (parent, step, task, v) = auto_ticked(&w);
    w.ok(&app(), undo(&w, &parent, v));
    assert!(!w.step(&parent, &step).done);
    let e = w.last_event(&parent);
    assert_eq!(e.change, WorkEventKind::Undone { event_version: v });
    assert_eq!(e.actor_id.as_deref(), Some(OWNER));
    assert!(!e.auto);
    assert_eq!(e.cause.as_ref().map(|c| c.version), Some(v));
    // Already undone.
    assert_eq!(
        w.code(&app(), undo(&w, &parent, v)),
        ErrorCode::InvalidRequest
    );
    // Nothing re-ticks it on its own; a new trigger (reopen, done again) does.
    let n = w.events(&parent).len();
    w.set(&parent, InProgress);
    assert!(!w.step(&parent, &step).done);
    w.set(&task, Todo);
    assert_eq!(w.events(&parent).len(), n + 1, "the untick was a no-op");
    w.set(&task, Done);
    assert!(w.step(&parent, &step).done);
}

#[test]
fn undo_is_refused_after_an_hour_after_a_later_change_and_for_a_manual_event() {
    let w = World::new();
    let (parent, _, _, v) = auto_ticked(&w);
    w.clock.advance(HOUR + 1);
    assert_eq!(
        w.code(&app(), undo(&w, &parent, v)),
        ErrorCode::InvalidRequest
    );

    let w = World::new();
    let (parent, _, task, v) = auto_ticked(&w);
    // A later automatic untick (the task reopened) touched the same step.
    w.set(&task, Todo);
    assert_eq!(w.code(&app(), undo(&w, &parent, v)), ErrorCode::Conflict);

    let w = World::new();
    let id = w.task();
    w.set(&id, InProgress);
    assert_eq!(w.code(&app(), undo(&w, &id, 2)), ErrorCode::InvalidRequest);
    assert_eq!(w.code(&app(), undo(&w, &id, 9)), ErrorCode::NotFound);
    assert_eq!(w.code(&app(), undo(&w, &w.id(), 1)), ErrorCode::NotFound);
}

#[test]
fn undo_of_bookkeeping_is_refused() {
    let w = World::new();
    let id = w.task();
    let mut c = w.comment(&id, "Which API?");
    c.comment_kind = Some(CommentKind::Question);
    c.asks = Some(OWNER.into());
    w.ok(&agent(), WorkCall::Comment(c));
    let v = w.version(&id);
    assert!(matches!(
        w.last_event(&id).change,
        WorkEventKind::WaitingStarted { .. }
    ));
    assert_eq!(w.code(&app(), undo(&w, &id, v)), ErrorCode::InvalidRequest);
}

#[test]
fn unlinking_or_deleting_the_made_task_makes_a_plain_step() {
    let w = World::new();
    let (parent, step) = w.task_with_step(false);
    let (task, call) = w.make_task(&step);
    w.ok(&agent(), call);
    w.ok(
        &agent(),
        WorkCall::ChecklistUnlinkTask(ChecklistUnlinkTaskParams {
            change_id: w.id(),
            step_id: step.clone(),
        }),
    );
    assert_eq!(w.step(&parent, &step).task_id, None);
    assert!(w.item(&task).part_of().is_none());
    assert!(matches!(
        w.last_event(&task).change,
        WorkEventKind::Unlinked { .. }
    ));
    // Now it can be ticked by hand; unlinking again is refused.
    w.ok(&agent(), w.check(&step, true));
    let again = WorkCall::ChecklistUnlinkTask(ChecklistUnlinkTaskParams {
        change_id: w.id(),
        step_id: step.clone(),
    });
    assert_eq!(w.code(&agent(), again), ErrorCode::InvalidRequest);

    let (parent, step) = w.task_with_step(false);
    let (task, call) = w.make_task(&step);
    w.ok(&agent(), call);
    let accepted = w.ok(&app(), w.delete(&task));
    assert_eq!(accepted.items.len(), 2);
    assert_eq!(w.step(&parent, &step).task_id, None);
    assert!(matches!(
        w.last_event(&parent).change,
        WorkEventKind::StepTaskUnlinked { .. }
    ));
}

#[test]
fn make_task_refusals() {
    let w = World::new();
    let (_, step) = w.task_with_step(false);
    let (_, call) = w.make_task(&step);
    w.ok(&agent(), call);
    let (_, again) = w.make_task(&step);
    assert_eq!(w.code(&agent(), again), ErrorCode::InvalidRequest);

    // An archived project takes no new items.
    let (_, step) = w.task_with_step(false);
    w.store.org_mut(|o| o.set_archived(PROJECT, true));
    let (_, call) = w.make_task(&step);
    assert_eq!(w.code(&agent(), call), ErrorCode::InvalidRequest);
    w.store.org_mut(|o| o.set_archived(PROJECT, false));

    // A taken task id, an unknown step.
    let (parent, step) = w.task_with_step(false);
    let call = WorkCall::ChecklistMakeTask(ChecklistMakeTaskParams {
        change_id: w.id(),
        step_id: step,
        task_id: parent,
        link_id: w.id(),
        title: None,
    });
    assert_eq!(w.code(&agent(), call), ErrorCode::AlreadyExists);
    let (_, call) = w.make_task(&w.id());
    assert_eq!(w.code(&agent(), call), ErrorCode::NotFound);
}

#[test]
fn make_task_cuts_a_long_step_text_to_a_title_and_keeps_the_team() {
    let w = World::new();
    let step = w.id();
    let s = step.clone();
    let parent = w.task_with(|p| {
        p.team_id = Some(TEAM.into());
        p.checklist = vec![NewStep {
            step_id: s,
            text: "x".repeat(200),
            required: false,
        }];
    });
    let (task, call) = w.make_task(&step);
    w.ok(&agent(), call);
    let t = w.item(&task);
    assert_eq!(t.title.chars().count(), WORK_TITLE_MAX_CHARS);
    assert_eq!(t.task.team_id.as_deref(), Some(TEAM));
    assert_eq!(t.project_id, w.item(&parent).project_id);
}

fn edit(w: &World, step: &str, expected: u64, text: &str) -> WorkCall {
    WorkCall::ChecklistEdit(ChecklistEditParams {
        change_id: w.id(),
        step_id: step.into(),
        expected_version: expected,
        text: Some(text.into()),
        required: None,
    })
}

fn remove(w: &World, step: &str, expected: u64) -> WorkCall {
    WorkCall::ChecklistRemove(ChecklistRemoveParams {
        change_id: w.id(),
        step_id: step.into(),
        expected_version: expected,
    })
}

#[test]
fn a_text_edit_and_a_tick_made_concurrently_are_both_kept() {
    let w = World::new();
    let (id, step) = w.task_with_step(false);
    let seen = w.step(&id, &step).version;
    w.ok(&agent(), w.check(&step, true));
    w.ok(&app(), edit(&w, &step, seen, "Write more tests"));
    let s = w.step(&id, &step);
    assert!(s.done);
    assert_eq!(s.text, "Write more tests");
    // A second edit from the same old version is stale: the content changed.
    assert_eq!(
        w.code(&app(), edit(&w, &step, seen, "Other")),
        ErrorCode::Conflict
    );
    // A version above the step's is invalid.
    assert_eq!(
        w.code(&app(), edit(&w, &step, s.version + 1, "Other")),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn remove_after_a_make_task_is_conflict_and_ends_the_tasks_link_otherwise() {
    let w = World::new();
    let (id, step) = w.task_with_step(false);
    let seen = w.step(&id, &step).version;
    let (task, call) = w.make_task(&step);
    w.ok(&agent(), call);
    assert_eq!(w.code(&app(), remove(&w, &step, seen)), ErrorCode::Conflict);
    let now = w.step(&id, &step).version;
    w.ok(&app(), remove(&w, &step, now));
    assert!(w.step(&id, &step).removed);
    assert_eq!(w.item(&id).summary().checklist.total, 0);
    assert!(w.item(&task).part_of().is_none());
    // A removed step is gone for every call; its id stays taken.
    assert_eq!(w.code(&app(), w.check(&step, true)), ErrorCode::NotFound);
    let add = WorkCall::ChecklistAdd(ChecklistAddParams {
        change_id: w.id(),
        item_id: id.clone(),
        steps: vec![NewStep {
            step_id: step.clone(),
            text: "again".into(),
            required: false,
        }],
        after_step_id: None,
    });
    assert_eq!(w.code(&app(), add), ErrorCode::AlreadyExists);
}

#[test]
fn a_tick_or_a_move_never_makes_a_remove_stale() {
    let w = World::new();
    let (id, step) = w.task_with_step(false);
    let seen = w.step(&id, &step).version;
    w.ok(&agent(), w.check(&step, true));
    w.ok(&app(), remove(&w, &step, seen));
}

fn add(w: &World, item: &str, texts: &[&str], after: Option<&str>) -> (Vec<String>, WorkCall) {
    let ids: Vec<String> = texts.iter().map(|_| w.id()).collect();
    let call = WorkCall::ChecklistAdd(ChecklistAddParams {
        change_id: w.id(),
        item_id: item.into(),
        steps: ids
            .iter()
            .zip(texts)
            .map(|(id, t)| NewStep {
                step_id: id.clone(),
                text: (*t).into(),
                required: false,
            })
            .collect(),
        after_step_id: after.map(str::to_owned),
    });
    (ids, call)
}

fn order(w: &World, item: &str) -> Vec<String> {
    w.item(item).live_steps().map(|s| s.text.clone()).collect()
}

#[test]
fn add_and_move_keep_the_order_asked_for() {
    let w = World::new();
    let id = w.task();
    let (ids, call) = add(&w, &id, &["a", "c"], None);
    w.ok(&agent(), call);
    let v = w.version(&id);
    let (_, call) = add(&w, &id, &["b"], Some(&ids[0]));
    w.ok(&agent(), call);
    assert_eq!(order(&w, &id), ["a", "b", "c"]);
    assert_eq!(w.version(&id), v + 1, "one StepsAdded event");

    let mv = |step: &str, after: Option<&str>| {
        WorkCall::ChecklistMove(ChecklistMoveParams {
            change_id: w.id(),
            step_id: step.into(),
            after_step_id: after.map(str::to_owned),
        })
    };
    w.ok(&agent(), mv(&ids[1], None));
    assert_eq!(order(&w, &id), ["c", "a", "b"]);
    w.ok(&agent(), mv(&ids[1], Some(&ids[0])));
    assert_eq!(order(&w, &id), ["a", "c", "b"]);
    // Where it already is: no event.
    let n = w.events(&id).len();
    w.ok(&agent(), mv(&ids[1], Some(&ids[0])));
    assert_eq!(w.events(&id).len(), n);
    // After itself, or after another item's step.
    assert_eq!(
        w.code(&agent(), mv(&ids[1], Some(&ids[1]))),
        ErrorCode::InvalidRequest
    );
    let (_, other) = w.task_with_step(false);
    assert_eq!(
        w.code(&agent(), mv(&ids[1], Some(&other))),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn an_item_has_at_most_100_steps() {
    let w = World::new();
    let id = w.task();
    let texts = vec!["s"; 100];
    let (_, call) = add(&w, &id, &texts, None);
    w.ok(&agent(), call);
    let (_, call) = add(&w, &id, &["one more"], None);
    assert_eq!(w.code(&agent(), call), ErrorCode::InvalidRequest);
    let (_, empty) = add(&w, &id, &[], None);
    assert_eq!(w.code(&agent(), empty), ErrorCode::InvalidRequest);
}

#[test]
fn making_a_ticked_step_into_a_task_unticks_it_until_the_task_is_completed() {
    let w = World::new();
    let (parent, step) = w.task_with_step(true);
    w.ok(&agent(), w.check(&step, true));
    let (task, call) = w.make_task(&step);
    w.ok(&agent(), call);
    let s = w.step(&parent, &step);
    assert!(!s.done);
    assert_eq!((s.checked_by, s.checked_at_ms), (None, None));
    // The open subtask blocks Done before the overridable required-step guard.
    let e = w.err(&agent(), WorkCall::TaskSetStatus(w.status(&parent, Done)));
    assert_eq!(e.code, ErrorCode::InvalidRequest);
    let mut p = w.status(&task, Done);
    p.resolution = Some(Resolution::Cancelled);
    w.ok(&agent(), WorkCall::TaskSetStatus(p));
    assert!(!w.step(&parent, &step).done);
}

#[test]
fn a_deleted_parent_gets_no_automatic_tick() {
    let w = World::new();
    let (parent, step) = w.task_with_step(false);
    let (task, call) = w.make_task(&step);
    w.ok(&agent(), call);
    w.ok(&app(), w.delete(&parent));
    let n = w.events(&parent).len();
    let accepted = w.set(&task, Done);
    assert_eq!(accepted.items.len(), 1);
    assert_eq!(w.events(&parent).len(), n);
}

#[test]
fn undoing_a_tick_is_stale_once_the_step_stopped_following_its_task() {
    let w = World::new();
    let (parent, step, _, v) = auto_ticked(&w);
    w.ok(
        &agent(),
        WorkCall::ChecklistUnlinkTask(ChecklistUnlinkTaskParams {
            change_id: w.id(),
            step_id: step,
        }),
    );
    assert_eq!(w.code(&app(), undo(&w, &parent, v)), ErrorCode::Conflict);
}

#[test]
fn b111_depth_one_refuses_promoting_a_subtasks_step_without_writes() {
    let w = World::new();
    let (_, step) = w.task_with_step(false);
    let (child, call) = w.make_task(&step);
    w.ok(&agent(), call);
    let grand_step = w.id();
    w.ok(
        &agent(),
        WorkCall::ChecklistAdd(ChecklistAddParams {
            change_id: w.id(),
            item_id: child.clone(),
            steps: vec![NewStep {
                step_id: grand_step.clone(),
                text: "Nested work".into(),
                required: false,
            }],
            after_step_id: None,
        }),
    );
    let before = w.store.snapshot();
    let (_, call) = w.make_task(&grand_step);
    assert_eq!(w.err(&agent(), call).code, ErrorCode::InvalidRequest);
    let after = w.store.snapshot();
    assert_eq!(after.items, before.items);
    assert_eq!(after.events, before.events);
    assert_eq!(after.numbers, before.numbers);
    assert_eq!(after.feed, before.feed);
    // Ending the parent link turns it back into a top-level task.
    w.ok(
        &agent(),
        WorkCall::ChecklistUnlinkTask(ChecklistUnlinkTaskParams {
            change_id: w.id(),
            step_id: step,
        }),
    );
    let (_, call) = w.make_task(&grand_step);
    w.ok(&agent(), call);
}

#[test]
fn b111_parent_done_refuses_optional_open_subtasks_even_with_anyway() {
    let w = World::new();
    let first = w.id();
    let second = w.id();
    let parent = w.task_with(|p| {
        p.checklist = vec![
            NewStep {
                step_id: first.clone(),
                text: "First optional work".into(),
                required: false,
            },
            NewStep {
                step_id: second.clone(),
                text: "Second optional work".into(),
                required: false,
            },
        ]
    });
    let (child1, call) = w.make_task(&first);
    w.ok(&agent(), call);
    let (child2, call) = w.make_task(&second);
    w.ok(&agent(), call);
    for anyway in [None, Some("Required-step override".to_owned())] {
        let before = w.item(&parent);
        let events = w.events(&parent);
        let mut p = w.status(&parent, Done);
        p.anyway_reason = anyway;
        let error = w.err(&agent(), WorkCall::TaskSetStatus(p));
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            "T-1 has 2 open subtasks: T-2, T-3. Finish or cancel them first."
        );
        assert_eq!(w.item(&parent), before);
        assert_eq!(w.events(&parent), events);
    }
    w.set(&child1, Done);
    let error = w.err(&agent(), WorkCall::TaskSetStatus(w.status(&parent, Done)));
    assert_eq!(
        error.message,
        "T-1 has 1 open subtask: T-3. Finish or cancel them first."
    );
    let mut p = w.status(&child2, Done);
    p.resolution = Some(Resolution::Cancelled);
    w.ok(&agent(), WorkCall::TaskSetStatus(p));
    w.set(&parent, Done);
}

#[test]
fn b111_numbered_task_or_bug_without_project_is_invalid_without_writes() {
    for kind in [ItemKind::Task, ItemKind::Bug] {
        let w = World::new();
        let id = if kind == ItemKind::Task {
            w.task()
        } else {
            w.bug()
        };
        {
            let mut state = w.store.work.lock().unwrap();
            let item = state.items.get_mut(&id).unwrap();
            item.number = Some(7);
            item.project_id = None;
        }
        let before = w.store.snapshot();
        let error = w.err(
            &agent(),
            WorkCall::Comment(w.comment(&id, "Must not persist")),
        );
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        let after = w.store.snapshot();
        assert_eq!(after.items, before.items);
        assert_eq!(after.events, before.events);
        assert_eq!(after.numbers, before.numbers);
        assert_eq!(after.feed, before.feed);
    }
}

#[test]
fn b111_parent_done_excludes_each_non_open_child_and_non_task() {
    for exclusion in ["completed", "cancelled", "deleted", "unlinked", "non-task"] {
        let w = World::new();
        let (parent, step) = w.task_with_step(false);
        let (child, call) = w.make_task(&step);
        w.ok(&agent(), call);
        // Retain the PartOf link for tombstones and non-tasks so each filter gets its own
        // witness. Ordinary delete also unlinks a promoted step, which would mask this guard.
        {
            let mut state = w.store.work.lock().unwrap();
            let item = state.items.get_mut(&child).unwrap();
            match exclusion {
                "completed" => {
                    item.status = Done;
                    item.resolution = Some(Resolution::Completed);
                }
                "cancelled" => {
                    item.status = Done;
                    item.resolution = Some(Resolution::Cancelled);
                }
                "deleted" => item.deleted = true,
                "unlinked" => item.links[0].removed = true,
                "non-task" => item.kind = ItemKind::Bug,
                _ => unreachable!(),
            }
        }
        w.set(&parent, Done);
    }
}

#[test]
fn b111_parent_done_orders_numbers_numerically_and_uses_short_ids() {
    let w = World::new();
    let (parent, step) = w.task_with_step(false);
    let (first, call) = w.make_task(&step);
    w.ok(&agent(), call);
    // Deliberately invert number order versus stable ids: 14 precedes 13 by id.
    let mut second = w.item(&first);
    second.id = w.id();
    second.number = Some(13);
    second.links[0].id = w.id();
    let second_id = second.id.clone();
    {
        let mut state = w.store.work.lock().unwrap();
        state.items.get_mut(&parent).unwrap().number = Some(12);
        state.items.get_mut(&first).unwrap().number = Some(14);
        state.items.insert(second.id.clone(), second);
    }
    let error = w.err(&agent(), WorkCall::TaskSetStatus(w.status(&parent, Done)));
    assert_eq!(
        error.message,
        "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first."
    );
    {
        let mut state = w.store.work.lock().unwrap();
        state.items.get_mut(&parent).unwrap().number = None;
        state.items.get_mut(&first).unwrap().number = None;
        state.items.get_mut(&second_id).unwrap().deleted = true;
    }
    let error = w.err(&agent(), WorkCall::TaskSetStatus(w.status(&parent, Done)));
    assert_eq!(
        error.message,
        format!(
            "{} has 1 open subtask: {}. Finish or cancel them first.",
            crate::short_id(&parent),
            crate::short_id(&first)
        )
    );
}

#[test]
fn b111_parent_cancel_and_other_transitions_keep_their_existing_semantics() {
    let w = World::new();
    let (parent, step) = w.task_with_step(false);
    let (_, call) = w.make_task(&step);
    w.ok(&agent(), call);
    w.set(&parent, InProgress);
    let mut p = w.status(&parent, Done);
    p.resolution = Some(Resolution::Cancelled);
    w.ok(&app(), WorkCall::TaskSetStatus(p));
}

#[test]
fn b111_number_shape_keeps_unnumbered_bugs_and_numbered_requests_valid() {
    for (kind, number, project) in [
        (ItemKind::Task, Some(7), Some(PROJECT)),
        (ItemKind::Task, None, None),
        (ItemKind::Bug, Some(7), Some(PROJECT)),
        (ItemKind::Bug, None, None),
        (ItemKind::Request, Some(7), None),
        (ItemKind::Unknown, Some(7), None),
    ] {
        let w = World::new();
        let id = w.task();
        {
            let mut state = w.store.work.lock().unwrap();
            let item = state.items.get_mut(&id).unwrap();
            item.kind = kind;
            item.number = number;
            item.project_id = project.map(str::to_owned);
        }
        w.ok(&agent(), WorkCall::Comment(w.comment(&id, "Allowed shape")));
    }
}
