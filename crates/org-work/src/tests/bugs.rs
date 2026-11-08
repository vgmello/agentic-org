//! Bugs (§1.4, §1.10, §4.5): report, update, triage, assign, mark fixed, verify, close, reopen,
//! the state machine, duplicates and numbering.

use org_proto::work::*;
use org_proto::ErrorCode;

use super::support::*;
use crate::WorkCall;

fn checklist_step(w: &World, required: bool) -> NewStep {
    NewStep {
        step_id: w.id(),
        text: "Add a regression test".into(),
        required,
    }
}

// ---- report ------------------------------------------------------------------------------

#[test]
fn a_report_makes_a_new_numbered_bug_reported_by_the_caller() {
    let w = World::new();
    let mut p = w.report_params(PROJECT);
    p.expected = Some("The coupon is taken off.".into());
    p.steps = Some("1. Add a coupon".into());
    p.environment = Some("Safari 18".into());
    let id = p.item_id.clone();
    let accepted = w.ok(&agent(), WorkCall::BugReport(p));

    let bug = w.item(&id);
    assert_eq!(bug.kind, ItemKind::Bug);
    assert_eq!(bug.status, ItemStatus::New);
    assert_eq!(bug.number, Some(1));
    assert_eq!(bug.created_by, "ada", "the reporter is the bound member");
    assert_eq!(bug.watchers, ["ada"]);
    assert_eq!(bug.priority, Some(Priority::Normal));
    assert_eq!(bug.assignee_id, None);
    assert_eq!(bug.bug.severity, Some(Severity::Major));
    assert!(!bug.bug.severity_confirmed, "confirmed only at triage");
    assert_eq!(
        bug.bug.area_id.as_deref(),
        Some(GENERAL),
        "General by default"
    );
    assert_eq!(bug.owner_office_id, "hq");

    let summary = &accepted.items[0];
    assert_eq!(summary.kind, ItemKind::Bug);
    assert_eq!(summary.area_id.as_deref(), Some(GENERAL));
    assert_eq!(summary.severity, Some(Severity::Major));
    assert!(!summary.severity_confirmed);
    assert_eq!(summary.reopen_count, 0);
    assert!(summary.fixed_by.is_empty());
    assert_eq!(w.events(&id)[0].change, WorkEventKind::Created);
    assert_eq!(w.events(&id)[0].actor_id.as_deref(), Some("ada"));
}

#[test]
fn a_report_can_name_its_area_its_checklist_and_where_it_came_from() {
    let w = World::new();
    let task = w.task();
    let (step, link) = (w.id(), w.id());
    let id = w.bug_with(|p| {
        p.area_id = Some(AREA_UI.into());
        p.checklist = vec![NewStep {
            step_id: step.clone(),
            text: "Test it".into(),
            required: true,
        }];
        p.from = Some(BugReportedFrom {
            link_id: link.clone(),
            to: LinkTarget::Item {
                item_id: task.clone(),
            },
        });
    });
    let bug = w.item(&id);
    assert_eq!(bug.bug.area_id.as_deref(), Some(AREA_UI));
    assert_eq!(bug.live_steps().count(), 1);
    let l = bug.live_links().next().expect("a link");
    assert_eq!(
        (l.id.as_str(), l.kind, l.created_by.as_str()),
        (link.as_str(), LinkKind::ReportedFrom, "pat")
    );
    assert_eq!(l.to, LinkTarget::Item { item_id: task });

    // From a doc: the project must exist.
    let id = w.bug_with(|p| {
        p.from = Some(BugReportedFrom {
            link_id: w.id(),
            to: LinkTarget::Doc {
                project_id: PROJECT.into(),
                doc_key: "ADR-0001".into(),
            },
        })
    });
    assert_eq!(w.item(&id).live_links().count(), 1);
}

#[test]
fn report_refusals() {
    let w = World::new();
    let task = w.task();
    let other_bug = w.bug();
    type Edit = Box<dyn Fn(&mut BugReportParams)>;
    let doc_from = |project: &str| {
        let project = project.to_owned();
        let link_id = w.id();
        move |p: &mut BugReportParams| {
            p.from = Some(BugReportedFrom {
                link_id: link_id.clone(),
                to: LinkTarget::Doc {
                    project_id: project.clone(),
                    doc_key: "x".into(),
                },
            })
        }
    };
    let cases: Vec<(Edit, ErrorCode)> = vec![
        (
            Box::new(|p| p.project_id = "nope".into()),
            ErrorCode::NotFound,
        ),
        (
            Box::new(|p| p.area_id = Some("nope".into())),
            ErrorCode::NotFound,
        ),
        (
            Box::new(|p| p.area_id = Some("p-pay-general".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.project_id = ARCHIVED.into()),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.severity = Severity::Unknown),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.title = " ".into()),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.title = "x".repeat(121)),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.what_happened = "".into()),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.what_happened = "x".repeat(WORK_TEXT_MAX_CHARS + 1)),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.environment = Some("x".repeat(WORK_ENVIRONMENT_MAX_CHARS + 1))),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.item_id = "b1".into()),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.affects_feature_id = Some("f".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new({
                let t = task.clone();
                let l = w.id();
                move |p| {
                    p.from = Some(BugReportedFrom {
                        link_id: l.clone(),
                        to: LinkTarget::Item { item_id: t.clone() },
                    });
                    p.checklist = vec![NewStep {
                        step_id: l.clone(),
                        text: "a clash with the link id".into(),
                        required: false,
                    }];
                }
            }),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new({
                let l = w.id();
                move |p| {
                    p.from = Some(BugReportedFrom {
                        link_id: l.clone(),
                        to: LinkTarget::Item { item_id: w_id() },
                    })
                }
            }),
            ErrorCode::NotFound,
        ),
        (
            Box::new({
                let (b, l) = (other_bug.clone(), w.id());
                move |p| {
                    p.from = Some(BugReportedFrom {
                        link_id: l.clone(),
                        to: LinkTarget::Item { item_id: b.clone() },
                    })
                }
            }),
            ErrorCode::InvalidRequest,
        ),
        (Box::new(doc_from("nope")), ErrorCode::NotFound),
    ];
    for (i, (f, code)) in cases.into_iter().enumerate() {
        let mut p = w.report_params(PROJECT);
        f(&mut p);
        assert_eq!(w.code(&pat(), WorkCall::BugReport(p)), code, "case {i}");
    }
    // A taken id.
    let mut p = w.report_params(PROJECT);
    p.item_id = other_bug;
    assert_eq!(
        w.code(&pat(), WorkCall::BugReport(p)),
        ErrorCode::AlreadyExists
    );
}

/// A well-formed id that names nothing.
fn w_id() -> String {
    crate::ulid_from_parts(T0, 999_999)
}

#[test]
fn a_report_in_a_task_call_or_a_task_call_on_a_bug_is_a_kind_mismatch() {
    let w = World::new();
    let bug = w.bug();
    let task = w.task();
    let p = w.status(&bug, ItemStatus::InProgress);
    assert_eq!(
        w.code(&app(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
    let p = w.update(&bug);
    assert_eq!(
        w.code(&app(), WorkCall::TaskUpdate(p)),
        ErrorCode::InvalidRequest
    );
    let p = w.bug_assign(&task, None);
    assert_eq!(
        w.code(&app(), WorkCall::BugAssign(p)),
        ErrorCode::InvalidRequest
    );
    let p = w.triage_params(&task, AREA_PAY);
    assert_eq!(
        w.code(&app(), WorkCall::BugTriage(p)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn bug_numbers_are_per_project_and_separate_from_task_numbers() {
    let w = World::new();
    let b1 = w.bug();
    let t1 = w.task();
    let b2 = w.bug();
    let other = {
        let p = w.report_params(OTHER_PROJECT);
        let id = p.item_id.clone();
        w.ok(&pat(), WorkCall::BugReport(p));
        id
    };
    assert_eq!(w.item(&b1).number, Some(1));
    assert_eq!(w.item(&t1).number, Some(1), "T and B count separately");
    assert_eq!(w.item(&b2).number, Some(2));
    assert_eq!(
        w.item(&other).number,
        Some(1),
        "each project counts its own"
    );

    // Never reused, deleted bugs included.
    w.ok(&app(), w.delete(&b2));
    let b3 = w.bug();
    assert_eq!(w.item(&b3).number, Some(3));
}

#[test]
fn a_retried_report_gets_the_stored_answer() {
    let w = World::new();
    let p = w.report_params(PROJECT);
    let first = w.ok(&pat(), WorkCall::BugReport(p.clone()));
    match w.call(&pat(), WorkCall::BugReport(p)) {
        crate::TxnOutput::Replayed(Ok(again)) => assert_eq!(again, first),
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(w.store.snapshot().items.len(), 1);
}

// ---- update ------------------------------------------------------------------------------

#[test]
fn update_changes_the_fields_given_in_one_event() {
    let w = World::new();
    let id = w.bug();
    let mut p = w.bug_update(&id);
    p.title = Some("Coupon total is wrong".into());
    p.what_happened = Some("Off by a cent".into());
    p.expected = Some("Exact".into());
    p.steps = Some("1. Pay".into());
    p.environment = Some("Chrome".into());
    p.severity = Some(Severity::Critical);
    p.priority = Some(Priority::Urgent);
    w.ok(&app(), WorkCall::BugUpdate(p));
    let bug = w.item(&id);
    assert_eq!(bug.title, "Coupon total is wrong");
    assert_eq!(bug.bug.what_happened, "Off by a cent");
    assert_eq!(bug.bug.expected.as_deref(), Some("Exact"));
    assert_eq!(bug.bug.severity, Some(Severity::Critical));
    assert_eq!(bug.priority, Some(Priority::Urgent));
    assert!(!bug.bug.severity_confirmed, "only triage confirms");
    let WorkEventKind::FieldsChanged { fields } = w.last_event(&id).change else {
        panic!("one fields_changed event")
    };
    let names: Vec<&str> = fields.iter().map(|f| f.field.as_str()).collect();
    assert_eq!(
        names,
        [
            "title",
            "what_happened",
            "expected",
            "steps",
            "environment",
            "severity",
            "priority"
        ]
    );
    assert_eq!(w.version(&id), 2);

    let mut p = w.bug_update(&id);
    p.clear_expected = true;
    p.clear_steps = true;
    p.clear_environment = true;
    w.ok(&app(), WorkCall::BugUpdate(p));
    let bug = w.item(&id);
    assert_eq!(
        (bug.bug.expected, bug.bug.steps, bug.bug.environment),
        (None, None, None)
    );
}

#[test]
fn update_refusals() {
    let w = World::new();
    let id = w.bug();
    type Edit = Box<dyn Fn(&mut BugUpdateParams)>;
    let cases: Vec<(Edit, ErrorCode)> = vec![
        (Box::new(|_| {}), ErrorCode::InvalidRequest),
        (
            Box::new(|p| {
                p.expected = Some("x".into());
                p.clear_expected = true;
            }),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.title = Some("".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.what_happened = Some(" ".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.severity = Some(Severity::Unknown)),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.priority = Some(Priority::Unknown)),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.area_id = Some("nope".into())),
            ErrorCode::NotFound,
        ),
        (
            Box::new(|p| p.area_id = Some("p-pay-general".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.affects_feature_id = Some("f".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.clear_affects_feature = true),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| {
                p.title = Some("x".into());
                p.expected_version = 99;
            }),
            ErrorCode::InvalidRequest,
        ),
    ];
    for (i, (f, code)) in cases.into_iter().enumerate() {
        let mut p = w.bug_update(&id);
        f(&mut p);
        assert_eq!(w.code(&app(), WorkCall::BugUpdate(p)), code, "case {i}");
    }
    assert_eq!(w.version(&id), 1, "nothing changed");

    // A closed bug keeps its area, but its text can still be fixed.
    let closed = w.bug_at(ItemStatus::Closed);
    let mut p = w.bug_update(&closed);
    p.area_id = Some(AREA_UI.into());
    assert_eq!(
        w.code(&app(), WorkCall::BugUpdate(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.bug_update(&closed);
    p.title = Some("A better title".into());
    w.ok(&app(), WorkCall::BugUpdate(p));

    // A deleted bug takes no change.
    w.ok(&app(), w.delete(&id));
    let mut p = w.bug_update(&id);
    p.title = Some("x".into());
    assert_eq!(
        w.code(&app(), WorkCall::BugUpdate(p)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn updates_conflict_per_field() {
    let w = World::new();
    let id = w.bug();
    let v = w.version(&id);
    // A comment and a tick don't make a title edit stale; another title edit does.
    w.ok(&app(), WorkCall::Comment(w.comment(&id, "Seen it too")));
    let mut p = w.bug_update(&id);
    p.expected_version = v;
    p.title = Some("First".into());
    w.ok(&app(), WorkCall::BugUpdate(p));
    let mut p = w.bug_update(&id);
    p.expected_version = v;
    p.title = Some("Second".into());
    assert_eq!(w.code(&app(), WorkCall::BugUpdate(p)), ErrorCode::Conflict);
    // A different field is still fine.
    let mut p = w.bug_update(&id);
    p.expected_version = v;
    p.environment = Some("Safari".into());
    w.ok(&app(), WorkCall::BugUpdate(p));
}

#[test]
fn a_reroute_after_a_triage_is_stale() {
    let w = World::new();
    let id = w.bug();
    let v = w.version(&id);
    w.triage(&id);
    let mut p = w.bug_update(&id);
    p.expected_version = v;
    p.area_id = Some(AREA_UI.into());
    assert_eq!(
        w.code(&app(), WorkCall::BugUpdate(p)),
        ErrorCode::Conflict,
        "the triage confirmed the area"
    );
}

// ---- triage ------------------------------------------------------------------------------

#[test]
fn triage_confirms_area_and_severity_in_one_event() {
    let w = World::new();
    let id = w.bug();
    let mut p = w.triage_params(&id, AREA_UI);
    p.severity = Severity::Minor;
    p.priority = Priority::Low;
    p.assignee_id = Some("riley".into());
    let accepted = w.ok(&agent(), WorkCall::BugTriage(p));

    let bug = w.item(&id);
    assert_eq!(bug.status, ItemStatus::Triaged);
    assert_eq!(bug.bug.area_id.as_deref(), Some(AREA_UI));
    assert_eq!(bug.bug.severity, Some(Severity::Minor));
    assert!(bug.bug.severity_confirmed);
    assert_eq!(bug.priority, Some(Priority::Low));
    assert_eq!(bug.assignee_id.as_deref(), Some("riley"));
    assert_eq!(accepted.items[0].status, ItemStatus::Triaged);
    let e = w.last_event(&id);
    assert_eq!(e.actor_id.as_deref(), Some("ada"));
    assert_eq!(
        e.change,
        WorkEventKind::Triaged {
            area_id: AREA_UI.into(),
            severity: Severity::Minor,
            priority: Priority::Low,
            assignee_id: Some("riley".into()),
        }
    );
    // One event, no bookkeeping: the bug was in an area riley leads.
    assert_eq!(w.version(&id), 2);
}

#[test]
fn triage_without_an_assignee_keeps_the_assignee() {
    let w = World::new();
    let id = w.bug();
    let p = w.bug_assign(&id, Some("pau"));
    w.ok(&app(), WorkCall::BugAssign(p));
    w.triage(&id);
    assert_eq!(w.item(&id).assignee_id.as_deref(), Some("pau"));
}

#[test]
fn triage_refusals() {
    let w = World::new();
    let id = w.bug();
    let mut p = w.triage_params(&id, "nope");
    assert_eq!(
        w.code(&app(), WorkCall::BugTriage(p.clone())),
        ErrorCode::NotFound
    );
    p.area_id = "p-pay-general".into();
    assert_eq!(
        w.code(&app(), WorkCall::BugTriage(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.triage_params(&id, AREA_PAY);
    p.severity = Severity::Unknown;
    assert_eq!(
        w.code(&app(), WorkCall::BugTriage(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.triage_params(&id, AREA_PAY);
    p.priority = Priority::Unknown;
    assert_eq!(
        w.code(&app(), WorkCall::BugTriage(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.triage_params(&id, AREA_PAY);
    p.assignee_id = Some("off".into());
    assert_eq!(
        w.code(&app(), WorkCall::BugTriage(p)),
        ErrorCode::UnknownRecipient
    );
    // A deleted area.
    w.store.org_mut(|o| o.delete_area(AREA_UI, AREA_PAY));
    let p = w.triage_params(&id, AREA_UI);
    assert_eq!(
        w.code(&app(), WorkCall::BugTriage(p)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(w.item(&id).status, ItemStatus::New);
}

#[test]
fn two_triages_from_the_same_version_conflict() {
    let w = World::new();
    let id = w.bug();
    let (a, b) = (
        w.triage_params(&id, AREA_PAY),
        w.triage_params(&id, AREA_UI),
    );
    w.ok(&app(), WorkCall::BugTriage(a));
    assert_eq!(
        w.code(&agent(), WorkCall::BugTriage(b)),
        ErrorCode::Conflict,
        "stale beats wrong state"
    );
    // From the current version it is a wrong state.
    let p = w.triage_params(&id, AREA_UI);
    assert_eq!(
        w.code(&agent(), WorkCall::BugTriage(p)),
        ErrorCode::InvalidRequest
    );
}

// ---- assign ------------------------------------------------------------------------------

#[test]
fn assign_sets_and_clears_the_assignee_on_an_open_bug() {
    let w = World::new();
    let id = w.bug();
    let p = w.bug_assign(&id, Some("riley"));
    w.ok(&app(), WorkCall::BugAssign(p));
    assert_eq!(w.item(&id).assignee_id.as_deref(), Some("riley"));
    assert_eq!(
        w.last_event(&id).change,
        WorkEventKind::Assigned {
            from: None,
            to: Some("riley".into())
        }
    );
    let p = w.bug_assign(&id, None);
    w.ok(&app(), WorkCall::BugAssign(p));
    assert_eq!(w.item(&id).assignee_id, None);

    let p = w.bug_assign(&id, Some("off"));
    assert_eq!(
        w.code(&app(), WorkCall::BugAssign(p)),
        ErrorCode::UnknownRecipient
    );
    let p = w.bug_assign(&id, Some("pau"));
    w.ok(&app(), WorkCall::BugAssign(p));

    let closed = w.bug_at(ItemStatus::Closed);
    let p = w.bug_assign(&closed, Some("riley"));
    assert_eq!(
        w.code(&app(), WorkCall::BugAssign(p)),
        ErrorCode::InvalidRequest
    );
}

// ---- mark fixed --------------------------------------------------------------------------

#[test]
fn mark_fixed_records_what_fixed_it() {
    let w = World::new();
    let id = w.triaged();
    let p = w.fixed_params(&id);
    w.ok(&agent(), WorkCall::BugMarkFixed(p));
    let bug = w.item(&id);
    assert_eq!(bug.status, ItemStatus::Fixed);
    assert_eq!(bug.bug.closed_reason.as_deref(), Some("Rounded the total"));
    assert_eq!(bug.resolution, None);
    let e = w.last_event(&id);
    assert_eq!(e.actor_id.as_deref(), Some("ada"));
    assert!(!e.auto);
    assert_eq!(
        e.change,
        WorkEventKind::StatusChanged {
            from: ItemStatus::Triaged,
            to: ItemStatus::Fixed,
            resolution: None,
            reason: Some("Rounded the total".into()),
            anyway: None,
            duplicate_of: None,
        }
    );

    let mut p = w.fixed_params(&w.triaged());
    p.note = "two\nlines".into();
    assert_eq!(
        w.code(&app(), WorkCall::BugMarkFixed(p)),
        ErrorCode::InvalidRequest
    );
    let mut p = w.fixed_params(&w.triaged());
    p.note = " ".into();
    assert_eq!(
        w.code(&app(), WorkCall::BugMarkFixed(p)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn mark_fixed_is_guarded_by_required_steps_like_done() {
    let w = World::new();
    let (required, optional) = (checklist_step(&w, true), checklist_step(&w, false));
    let (req_id, opt_id) = (required.step_id.clone(), optional.step_id.clone());
    let id = w.bug_with(|p| {
        p.area_id = Some(AREA_PAY.into());
        p.checklist = vec![required, optional];
    });
    w.triage(&id);

    let p = w.fixed_params(&id);
    let err = w.err(&app(), WorkCall::BugMarkFixed(p));
    assert_eq!(err.code, ErrorCode::ChecklistIncomplete);
    assert!(err.message.contains("Add a regression test"));
    assert_eq!(w.item(&id).status, ItemStatus::Triaged, "nothing changed");

    // With a reason it is Fixed, and the step is recorded; optional steps never block.
    let mut p = w.fixed_params(&id);
    p.anyway_reason = Some("Test follows in the next release".into());
    let accepted = w.ok(&app(), WorkCall::BugMarkFixed(p));
    assert!(accepted.items[0].done_anyway);
    let WorkEventKind::StatusChanged { anyway, .. } = w.last_event(&id).change else {
        panic!("status")
    };
    let anyway = anyway.expect("recorded");
    assert_eq!(anyway.steps, std::slice::from_ref(&req_id));
    assert!(!anyway.steps.contains(&opt_id));
}

#[test]
fn a_ticked_required_step_lets_mark_fixed_through() {
    let w = World::new();
    let step = checklist_step(&w, true);
    let step_id = step.step_id.clone();
    let id = w.bug_with(|p| {
        p.area_id = Some(AREA_PAY.into());
        p.checklist = vec![step];
    });
    w.triage(&id);
    w.ok(&app(), w.check(&step_id, true));
    let p = w.fixed_params(&id);
    let accepted = w.ok(&app(), WorkCall::BugMarkFixed(p));
    assert!(!accepted.items[0].done_anyway);
}

// ---- verify ------------------------------------------------------------------------------

#[test]
fn verifying_closes_the_bug_and_records_whether_the_reporter_did() {
    let w = World::new();
    // The reporter (pat) verifies.
    let id = w.bug_at(ItemStatus::Fixed);
    let p = w.verify_params(&id, VerifyOutcome::Fixed);
    w.ok(&pat(), WorkCall::BugVerify(p));
    let bug = w.item(&id);
    assert_eq!(
        (bug.status, bug.resolution),
        (ItemStatus::Closed, Some(Resolution::Verified))
    );
    assert_eq!(
        bug.bug.closed_reason.as_deref(),
        Some("Rounded the total"),
        "what fixed it stays"
    );
    assert_eq!(
        w.last_event(&id).change,
        WorkEventKind::Verified {
            outcome: VerifyOutcome::Fixed,
            by_reporter: true,
            note: None
        }
    );

    // Anyone may; the event says it wasn't the reporter.
    let id = w.bug_at(ItemStatus::Fixed);
    let p = w.verify_params(&id, VerifyOutcome::Fixed);
    w.ok(&agent(), WorkCall::BugVerify(p));
    let WorkEventKind::Verified { by_reporter, .. } = w.last_event(&id).change else {
        panic!("verified")
    };
    assert!(!by_reporter);
}

#[test]
fn still_happens_sends_the_bug_back_to_triaged_keeping_assignee_and_fix_links() {
    let w = World::new();
    let id = w.triaged();
    let p = w.bug_assign(&id, Some("riley"));
    w.ok(&app(), WorkCall::BugAssign(p));
    let task = w.fix_task(&id);
    w.set(&task, ItemStatus::InProgress);
    w.set(&task, ItemStatus::Done);
    assert_eq!(w.item(&id).status, ItemStatus::Fixed);

    // A note is required.
    let mut p = w.verify_params(&id, VerifyOutcome::StillHappens);
    p.note = None;
    assert_eq!(
        w.code(&pat(), WorkCall::BugVerify(p)),
        ErrorCode::InvalidRequest
    );

    let p = w.verify_params(&id, VerifyOutcome::StillHappens);
    let accepted = w.ok(&pat(), WorkCall::BugVerify(p));
    let bug = w.item(&id);
    assert_eq!(bug.status, ItemStatus::Triaged);
    assert_eq!(bug.bug.reopen_count, 1);
    assert_eq!(accepted.items[0].reopen_count, 1);
    assert_eq!(bug.assignee_id.as_deref(), Some("riley"));
    assert_eq!(bug.bug.closed_reason, None);
    assert_eq!(
        accepted.items[0].fixed_by,
        std::slice::from_ref(&task),
        "fix links kept"
    );
    assert_eq!(w.item(&task).fixes(), std::slice::from_ref(&id));
    // Never re-fixed on its own: it stays Triaged until a fix task changes again.
    assert!(!w.last_event(&id).auto);
}

#[test]
fn verify_is_only_for_a_fixed_bug() {
    let w = World::new();
    for status in [
        ItemStatus::New,
        ItemStatus::Triaged,
        ItemStatus::InProgress,
        ItemStatus::Closed,
    ] {
        let id = w.bug_at(status);
        for outcome in [VerifyOutcome::Fixed, VerifyOutcome::StillHappens] {
            let p = w.verify_params(&id, outcome);
            assert_eq!(
                w.code(&pat(), WorkCall::BugVerify(p)),
                ErrorCode::InvalidRequest,
                "{status:?} {outcome:?}"
            );
        }
    }
    let id = w.bug_at(ItemStatus::Fixed);
    let mut p = w.verify_params(&id, VerifyOutcome::Fixed);
    p.outcome = VerifyOutcome::Unknown;
    assert_eq!(
        w.code(&pat(), WorkCall::BugVerify(p)),
        ErrorCode::InvalidRequest
    );
}

// ---- close and reopen --------------------------------------------------------------------

#[test]
fn wont_fix_and_cant_reproduce_need_a_reason() {
    let w = World::new();
    for resolution in [Resolution::WontFix, Resolution::CantReproduce] {
        let id = w.bug();
        let mut p = w.wont_fix_params(&id);
        p.resolution = Some(resolution);
        p.reason = None;
        assert_eq!(
            w.code(&app(), WorkCall::BugClose(p.clone())),
            ErrorCode::InvalidRequest
        );
        p.change_id = w.id();
        p.reason = Some("x".repeat(WORK_REASON_MAX_CHARS + 1));
        assert_eq!(
            w.code(&app(), WorkCall::BugClose(p.clone())),
            ErrorCode::InvalidRequest
        );
        p.change_id = w.id();
        p.reason = Some("Works for me".into());
        w.ok(&app(), WorkCall::BugClose(p));
        let bug = w.item(&id);
        assert_eq!(
            (bug.status, bug.resolution),
            (ItemStatus::Closed, Some(resolution))
        );
        assert_eq!(bug.bug.closed_reason.as_deref(), Some("Works for me"));
    }
    // Neither a duplicate nor a resolution, a resolution that isn't a close, and both.
    let id = w.bug();
    let mut p = w.wont_fix_params(&id);
    p.resolution = None;
    assert_eq!(
        w.code(&app(), WorkCall::BugClose(p)),
        ErrorCode::InvalidRequest
    );
    for r in [
        Resolution::Verified,
        Resolution::Completed,
        Resolution::Duplicate,
    ] {
        let mut p = w.wont_fix_params(&id);
        p.resolution = Some(r);
        assert_eq!(
            w.code(&app(), WorkCall::BugClose(p)),
            ErrorCode::InvalidRequest,
            "{r:?}"
        );
    }
    let original = w.bug();
    let mut p = w.duplicate_params(&id, &original);
    p.reason = Some("also".into());
    assert_eq!(
        w.code(&app(), WorkCall::BugClose(p)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn a_duplicate_names_its_original_and_its_reporter_watches_it() {
    let w = World::new();
    // The original was reported by the agent, so its reporter (pat, the duplicate's) is new.
    let original = {
        let mut p = w.report_params(PROJECT);
        p.area_id = Some(AREA_PAY.into());
        let id = p.item_id.clone();
        w.ok(&agent(), WorkCall::BugReport(p));
        id
    };
    let dup = w.bug_with(|p| p.area_id = Some(AREA_PAY.into()));
    let p = w.duplicate_params(&dup, &original);
    let accepted = w.ok(&agent(), WorkCall::BugClose(p));

    let bug = w.item(&dup);
    assert_eq!(
        (bug.status, bug.resolution),
        (ItemStatus::Closed, Some(Resolution::Duplicate))
    );
    assert_eq!(bug.bug.duplicate_of.as_deref(), Some(original.as_str()));
    assert_eq!(
        accepted.items[0].duplicate_of.as_deref(),
        Some(original.as_str())
    );
    let WorkEventKind::StatusChanged {
        resolution, reason, ..
    } = w.last_event(&dup).change
    else {
        panic!("status")
    };
    assert_eq!(
        (resolution, reason),
        (Some(Resolution::Duplicate), Some(original.clone())),
        "the event names the original"
    );
    // The original's watchers gain the duplicate's reporter (pat), once.
    assert!(w.item(&original).watchers.contains(&"pat".to_owned()));
    assert_eq!(accepted.items.len(), 2);
    assert_eq!(
        w.last_event(&original).change,
        WorkEventKind::Watched {
            member_id: "pat".into(),
            watching: true
        }
    );
    // The reporter already watching: no event on the original.
    let again = w.bug_with(|p| p.area_id = Some(AREA_PAY.into()));
    let before = w.version(&original);
    let p = w.duplicate_params(&again, &original);
    let accepted = w.ok(&agent(), WorkCall::BugClose(p));
    assert_eq!(accepted.items.len(), 1);
    assert_eq!(w.version(&original), before);
}

#[test]
fn duplicate_refusals() {
    let w = World::new();
    let id = w.bug();
    let task = w.task();
    let other_project = {
        let p = w.report_params(OTHER_PROJECT);
        let i = p.item_id.clone();
        w.ok(&pat(), WorkCall::BugReport(p));
        i
    };
    let deleted = w.bug();
    w.ok(&app(), w.delete(&deleted));
    let was_dup = w.bug();
    let real = w.bug();
    let p = w.duplicate_params(&was_dup, &real);
    w.ok(&app(), WorkCall::BugClose(p));

    let cases = [
        (id.clone(), ErrorCode::InvalidRequest, "itself"),
        (other_project, ErrorCode::InvalidRequest, "another project"),
        (task, ErrorCode::InvalidRequest, "a task"),
        (deleted, ErrorCode::InvalidRequest, "deleted"),
        (was_dup, ErrorCode::InvalidRequest, "closed as a duplicate"),
        (w_id(), ErrorCode::NotFound, "no such bug"),
    ];
    for (original, code, what) in cases {
        let p = w.duplicate_params(&id, &original);
        assert_eq!(w.code(&app(), WorkCall::BugClose(p)), code, "{what}");
    }
    assert_eq!(w.item(&id).status, ItemStatus::New);

    // Closed for another reason, the original is still a fine original.
    let closed = w.bug_at(ItemStatus::Closed);
    let p = w.duplicate_params(&id, &closed);
    w.ok(&app(), WorkCall::BugClose(p));
}

#[test]
fn two_bugs_cannot_be_duplicates_of_each_other() {
    let w = World::new();
    let (a, b) = (w.bug(), w.bug());
    let p = w.duplicate_params(&a, &b);
    w.ok(&app(), WorkCall::BugClose(p));
    let p = w.duplicate_params(&b, &a);
    assert_eq!(
        w.code(&app(), WorkCall::BugClose(p)),
        ErrorCode::InvalidRequest,
        "a is closed as a duplicate"
    );
}

#[test]
fn reopening_a_closed_bug_goes_back_to_triaged() {
    let w = World::new();
    let id = w.bug_at(ItemStatus::Closed);
    let mut p = w.reopen_params(&id);
    p.note = " ".into();
    assert_eq!(
        w.code(&app(), WorkCall::BugReopen(p)),
        ErrorCode::InvalidRequest
    );
    let p = w.reopen_params(&id);
    w.ok(&pat(), WorkCall::BugReopen(p));
    let bug = w.item(&id);
    assert_eq!(bug.status, ItemStatus::Triaged);
    assert_eq!(bug.resolution, None);
    assert_eq!(bug.bug.closed_reason, None);
    assert_eq!(bug.bug.reopen_count, 1);
    assert_eq!(
        w.last_event(&id).change,
        WorkEventKind::Reopened {
            note: Some("It came back".into())
        }
    );
    // And again.
    let p = w.wont_fix_params(&id);
    w.ok(&app(), WorkCall::BugClose(p));
    let p = w.reopen_params(&id);
    w.ok(&app(), WorkCall::BugReopen(p));
    assert_eq!(w.item(&id).bug.reopen_count, 2);
}

#[test]
fn reopening_a_duplicate_goes_to_new_and_clears_the_original() {
    let w = World::new();
    let original = w.bug();
    let dup = w.triaged();
    assert!(w.item(&dup).bug.severity_confirmed);
    let p = w.duplicate_params(&dup, &original);
    w.ok(&app(), WorkCall::BugClose(p));
    let p = w.reopen_params(&dup);
    let accepted = w.ok(&app(), WorkCall::BugReopen(p));
    let bug = w.item(&dup);
    assert_eq!(bug.status, ItemStatus::New, "triage looks again");
    assert!(
        !bug.bug.severity_confirmed,
        "and confirms the severity again"
    );
    assert!(!accepted.items[0].severity_confirmed);
    assert_eq!(bug.bug.duplicate_of, None);
    assert_eq!(bug.resolution, None);
    assert_eq!(accepted.items[0].duplicate_of, None);
}

// ---- the state machine, as a table -------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Op {
    Update,
    Triage,
    Assign,
    MarkFixed,
    VerifyFixed,
    VerifyStillHappens,
    CloseDuplicate,
    CloseWontFix,
    Reopen,
}

const OPS: [Op; 9] = [
    Op::Update,
    Op::Triage,
    Op::Assign,
    Op::MarkFixed,
    Op::VerifyFixed,
    Op::VerifyStillHappens,
    Op::CloseDuplicate,
    Op::CloseWontFix,
    Op::Reopen,
];

/// §1.10's table: which call is allowed from which status. `bug.update` and edits to text are
/// allowed everywhere; the rest follow the arrows.
fn allowed(from: ItemStatus, op: Op) -> bool {
    use ItemStatus::*;
    match op {
        Op::Update => true,
        Op::Triage => from == New,
        Op::Assign => from != Closed,
        Op::MarkFixed => matches!(from, Triaged | InProgress),
        Op::VerifyFixed | Op::VerifyStillHappens => from == Fixed,
        Op::CloseDuplicate | Op::CloseWontFix => from != Closed,
        Op::Reopen => from == Closed,
    }
}

fn call_for(w: &World, id: &str, op: Op) -> WorkCall {
    match op {
        Op::Update => {
            let mut p = w.bug_update(id);
            p.title = Some("Another title".into());
            WorkCall::BugUpdate(p)
        }
        Op::Triage => WorkCall::BugTriage(w.triage_params(id, AREA_PAY)),
        Op::Assign => WorkCall::BugAssign(w.bug_assign(id, Some("riley"))),
        Op::MarkFixed => WorkCall::BugMarkFixed(w.fixed_params(id)),
        Op::VerifyFixed => WorkCall::BugVerify(w.verify_params(id, VerifyOutcome::Fixed)),
        Op::VerifyStillHappens => {
            WorkCall::BugVerify(w.verify_params(id, VerifyOutcome::StillHappens))
        }
        Op::CloseDuplicate => {
            let original = w.bug_in(Some(AREA_PAY));
            WorkCall::BugClose(w.duplicate_params(id, &original))
        }
        Op::CloseWontFix => WorkCall::BugClose(w.wont_fix_params(id)),
        Op::Reopen => WorkCall::BugReopen(w.reopen_params(id)),
    }
}

#[test]
fn every_bug_transition_is_allowed_or_refused_as_the_design_says() {
    use ItemStatus::*;
    let statuses = [New, Triaged, InProgress, Fixed, Closed];
    for from in statuses {
        for op in OPS {
            let w = World::new();
            let id = w.bug_at(from);
            let before = w.store.snapshot();
            let call = call_for(&w, &id, op);
            // `call_for` may make a second bug (the original of a duplicate).
            let others = w.store.snapshot().items.len();
            let version = w.version(&id);
            if allowed(from, op) {
                let accepted = w.ok(&app(), call);
                assert!(!accepted.items.is_empty(), "{from:?} {op:?}");
                let events = w.events(&id);
                let new: Vec<_> = events.iter().filter(|e| e.version > version).collect();
                assert!(!new.is_empty(), "{from:?} {op:?} records an event");
                assert_eq!(
                    new[0].actor_id.as_deref(),
                    Some(OWNER),
                    "{from:?} {op:?}: the first event is the caller's"
                );
                assert!(!new[0].auto, "{from:?} {op:?}");
                assert_eq!(
                    new[0].version,
                    version + 1,
                    "{from:?} {op:?}: one event per call"
                );
            } else {
                let err = w.err(&app(), call);
                assert_eq!(err.code, ErrorCode::InvalidRequest, "{from:?} {op:?}");
                assert_eq!(w.version(&id), version, "{from:?} {op:?} changed nothing");
                assert_eq!(
                    w.store.snapshot().items.len(),
                    others,
                    "{from:?} {op:?} created nothing"
                );
                assert_eq!(
                    w.store.snapshot().feed.len(),
                    before.feed.len() + (others - before.items.len()),
                    "{from:?} {op:?} wrote no feed row"
                );
            }
        }
    }
}

#[test]
fn where_each_allowed_call_leads() {
    use ItemStatus::*;
    let cases = [
        (New, Op::Triage, Triaged),
        (Triaged, Op::MarkFixed, Fixed),
        (InProgress, Op::MarkFixed, Fixed),
        (Fixed, Op::VerifyFixed, Closed),
        (Fixed, Op::VerifyStillHappens, Triaged),
        (New, Op::CloseDuplicate, Closed),
        (Triaged, Op::CloseWontFix, Closed),
        (InProgress, Op::CloseWontFix, Closed),
        (Fixed, Op::CloseWontFix, Closed),
        (Closed, Op::Reopen, Triaged),
        (New, Op::Assign, New),
    ];
    for (from, op, to) in cases {
        let w = World::new();
        let id = w.bug_at(from);
        let call = call_for(&w, &id, op);
        w.ok(&app(), call);
        assert_eq!(w.item(&id).status, to, "{from:?} {op:?}");
    }
}

#[test]
fn bug_calls_need_the_bound_member_to_be_able_to_act() {
    let w = World::new();
    // An offboarded agent can't report, and an agent can't act as the owner.
    let mut p = w.report_params(PROJECT);
    p.title = "x".into();
    assert_eq!(
        w.code(&crate::Caller::agent("off"), WorkCall::BugReport(p.clone())),
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        w.code(&crate::Caller::agent(OWNER), WorkCall::BugReport(p)),
        ErrorCode::NotAuthorized
    );
}
