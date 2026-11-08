//! Per-field conflicts (§2.3).

use org_proto::work::*;
use org_proto::ErrorCode;

use super::support::*;
use crate::WorkCall;

fn title(w: &World, id: &str, expected: u64, t: &str) -> WorkCall {
    let mut p = w.update(id);
    p.expected_version = expected;
    p.title = Some(t.into());
    WorkCall::TaskUpdate(p)
}

#[test]
fn a_tick_and_a_comment_never_make_a_title_edit_stale() {
    let w = World::new();
    let (id, step) = w.task_with_step(false);
    w.set(&id, ItemStatus::InProgress);
    let seen = w.version(&id);
    w.ok(&agent(), w.check(&step, true));
    w.ok(&agent(), WorkCall::Comment(w.comment(&id, "on it")));
    assert_eq!(w.version(&id), seen + 2);
    w.ok(
        &app(),
        title(&w, &id, seen, "Coupon edge cases, all of them"),
    );
    assert_eq!(w.item(&id).title, "Coupon edge cases, all of them");
}

#[test]
fn a_title_edit_after_another_title_edit_is_conflict() {
    let w = World::new();
    let id = w.task();
    let seen = w.version(&id);
    w.ok(&agent(), title(&w, &id, seen, "One"));
    let before = w.item(&id);
    assert_eq!(
        w.code(&app(), title(&w, &id, seen, "Two")),
        ErrorCode::Conflict
    );
    assert_eq!(w.item(&id), before);
    // Another field from the same old version is fine.
    let mut p = w.update(&id);
    p.expected_version = seen;
    p.priority = Some(Priority::High);
    w.ok(&app(), WorkCall::TaskUpdate(p));
}

#[test]
fn two_status_moves_from_the_same_version_conflict_rather_than_wrong_state() {
    let w = World::new();
    let id = w.task();
    let first = w.status(&id, ItemStatus::InProgress);
    let mut second = w.status(&id, ItemStatus::InProgress);
    second.change_id = w.id();
    w.ok(&app(), WorkCall::TaskSetStatus(first));
    // InProgress → InProgress would be a wrong state; stale wins.
    assert_eq!(
        w.code(&agent(), WorkCall::TaskSetStatus(second)),
        ErrorCode::Conflict
    );
}

#[test]
fn an_expected_version_above_the_current_one_is_invalid() {
    let w = World::new();
    let id = w.task();
    assert_eq!(
        w.code(&app(), title(&w, &id, 2, "x")),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn assign_conflicts_only_with_another_assignment() {
    let w = World::new();
    let id = w.task();
    let seen = w.version(&id);
    w.ok(&agent(), title(&w, &id, seen, "Renamed"));
    let assign = |to: &str, expected| {
        WorkCall::TaskAssign(TaskAssignParams {
            change_id: w.id(),
            item_id: id.clone(),
            expected_version: expected,
            assignee_id: Some(to.into()),
        })
    };
    w.ok(&app(), assign("ada", seen));
    assert_eq!(w.code(&app(), assign("riley", seen)), ErrorCode::Conflict);
    let e = w.last_event(&id);
    assert_eq!(
        e.change,
        WorkEventKind::Assigned {
            from: None,
            to: Some("ada".into())
        }
    );
}

#[test]
fn a_waiting_event_never_makes_a_title_edit_stale_but_a_comment_makes_a_delete_stale() {
    let w = World::new();
    let id = w.task();
    let seen = w.version(&id);
    let mut c = w.comment(&id, "Which API?");
    c.comment_kind = Some(CommentKind::Question);
    c.asks = Some(OWNER.into());
    w.ok(&agent(), WorkCall::Comment(c));
    assert_eq!(w.version(&id), seen + 2, "the comment and its waiting span");
    w.ok(&app(), title(&w, &id, seen, "x"));
    let stale = WorkCall::Delete(WorkDeleteParams {
        change_id: w.id(),
        item_id: id.clone(),
        expected_version: seen,
    });
    assert_eq!(w.code(&app(), stale), ErrorCode::Conflict);
}

#[test]
fn update_validation() {
    let w = World::new();
    let id = w.task();
    let p = w.update(&id);
    assert_eq!(
        w.code(&app(), WorkCall::TaskUpdate(p)),
        ErrorCode::InvalidRequest,
        "nothing"
    );
    let mut p = w.update(&id);
    p.description = Some("x".into());
    p.clear_description = true;
    assert_eq!(
        w.code(&app(), WorkCall::TaskUpdate(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.update(&id);
    p.clear_feature = true;
    assert_eq!(
        w.code(&app(), WorkCall::TaskUpdate(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.update(&id);
    p.team_id = Some("team-z".into());
    assert_eq!(
        w.code(&app(), WorkCall::TaskUpdate(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.update(&id);
    p.priority = Some(Priority::Unknown);
    assert_eq!(
        w.code(&app(), WorkCall::TaskUpdate(p)),
        ErrorCode::InvalidRequest
    );

    let mut p = w.update(&id);
    p.description = Some("  Details  ".into());
    p.team_id = Some(TEAM.into());
    w.ok(&app(), WorkCall::TaskUpdate(p));
    let item = w.item(&id);
    assert_eq!(item.task.description.as_deref(), Some("Details"));
    let WorkEventKind::FieldsChanged { fields } = w.last_event(&id).change else {
        panic!("fields changed")
    };
    assert_eq!(
        fields.iter().map(|f| f.field.as_str()).collect::<Vec<_>>(),
        ["description", "team_id"]
    );
    let mut p = w.update(&id);
    p.clear_description = true;
    p.clear_team = true;
    w.ok(&app(), WorkCall::TaskUpdate(p));
    let item = w.item(&id);
    assert_eq!((item.task.description, item.task.team_id), (None, None));
}

// ---- bugs ----------------------------------------------------------------------------------

#[test]
fn a_triage_does_not_make_an_edit_of_another_field_stale() {
    let w = World::new();
    let id = w.bug();
    let v = w.version(&id);
    w.triage(&id);
    // A title edit and an assign from before the triage still go through.
    let mut p = w.bug_update(&id);
    p.expected_version = v;
    p.title = Some("A clearer title".into());
    w.ok(&app(), WorkCall::BugUpdate(p));
    let mut p = w.bug_assign(&id, Some("riley"));
    p.expected_version = v;
    w.ok(&app(), WorkCall::BugAssign(p));
    // But the fields the triage set are stale: severity, priority and the area.
    let edits: [fn(&mut BugUpdateParams); 3] = [
        |p| p.severity = Some(Severity::Minor),
        |p| p.priority = Some(Priority::Low),
        |p| p.area_id = Some(AREA_UI.into()),
    ];
    for edit in edits {
        let mut p = w.bug_update(&id);
        p.expected_version = v;
        edit(&mut p);
        assert_eq!(w.code(&app(), WorkCall::BugUpdate(p)), ErrorCode::Conflict);
    }
}

#[test]
fn a_triage_that_assigns_makes_a_stale_assign_conflict() {
    let w = World::new();
    let id = w.bug();
    let mut t = w.triage_params(&id, AREA_PAY);
    t.assignee_id = Some("riley".into());
    let stale = w.bug_assign(&id, Some("pau"));
    w.ok(&app(), WorkCall::BugTriage(t));
    assert_eq!(
        w.code(&app(), WorkCall::BugAssign(stale)),
        ErrorCode::Conflict
    );
}

#[test]
fn verify_and_reopen_only_make_status_calls_stale() {
    let w = World::new();
    let id = w.bug_at(ItemStatus::Fixed);
    let v = w.version(&id);
    let p = w.verify_params(&id, VerifyOutcome::StillHappens);
    w.ok(&pat(), WorkCall::BugVerify(p));
    let mut p = w.bug_update(&id);
    p.expected_version = v;
    p.title = Some("Still off".into());
    w.ok(&app(), WorkCall::BugUpdate(p));
    let mut stale = w.fixed_params(&id);
    stale.expected_version = v;
    assert_eq!(
        w.code(&app(), WorkCall::BugMarkFixed(stale)),
        ErrorCode::Conflict
    );

    let closed = w.bug_at(ItemStatus::Closed);
    let v = w.version(&closed);
    let p = w.reopen_params(&closed);
    w.ok(&app(), WorkCall::BugReopen(p));
    let mut p = w.bug_assign(&closed, Some("riley"));
    p.expected_version = v;
    w.ok(&app(), WorkCall::BugAssign(p));
}

#[test]
fn the_note_that_a_rule_changed_nothing_does_not_make_a_status_call_stale() {
    let w = World::new();
    let step = w.id();
    let bug = w.bug_with(|p| {
        p.area_id = Some(AREA_PAY.into());
        p.checklist = vec![NewStep {
            step_id: step.clone(),
            text: "Add a test".into(),
            required: true,
        }];
    });
    w.triage(&bug);
    let task = w.fix_task(&bug);
    w.set(&task, ItemStatus::InProgress);
    let seen = w.version(&bug);
    w.set(&task, ItemStatus::Done); // records "all fix tasks done; required steps left"
    assert!(w.version(&bug) > seen);
    let mut p = w.wont_fix_params(&bug);
    p.expected_version = seen;
    w.ok(&app(), WorkCall::BugClose(p));
}
