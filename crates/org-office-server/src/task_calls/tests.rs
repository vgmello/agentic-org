use super::*;
use org_office::TestOffice;
use org_proto::tasks::{
    LabelMatchMode, PatchField, TaskSortMode, TaskStatus, TaskStatusScope,
    TasksAddDependencyParams, TasksAssignParams, TasksCreateParams, TasksInspectParams,
    TasksListParams, TasksTransitionParams,
};
use org_proto::Outcome;

fn uuid(n: u64) -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    bytes[7] = (n & 0xff) as u8;
    bytes[8] = 0x80;
    bytes[15] = (n & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[test]
fn task_service_dispatches_create_list_dependencies_and_transition() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let live = Arc::new(LiveEvents::new(Box::new(|| {})));
    let svc = TaskService::new(Arc::clone(&office), live, None);

    // Unbound caller rejected.
    let list_req = Request::new::<TasksList>(1, &TasksListParams::default()).unwrap();
    let unbound = svc.dispatch(None, &list_req).unwrap();
    assert!(matches!(unbound.outcome, Outcome::Error(e) if e.code == ErrorCode::NotBound));

    // Create two tasks with labels, files, and order.
    let r1 = Request::new::<TasksCreate>(
        2,
        &TasksCreateParams {
            command_id: uuid(10),
            task_id: uuid(11),
            team_id: Some(root_team.clone()),
            project_id: None,
            assigned_team_id: None,
            assignee_id: None,
            task_type: Some("task".into()),
            work_type: None,
            title: "First task".into(),
            body_markdown: String::new(),
            priority: None,
            effort: None,
            severity: None,
            rank_order: Some(10),
            labels: vec!["backend".into(), "security".into()],
            reference_files: vec!["docs/a.md".into()],
            modified_files: vec!["src/a.rs".into()],
            depends_on: vec![],
            attachment_ids: vec![],
            details: None,
            dod: None,
        },
    )
    .unwrap();
    let res1: TasksMutationResult = match svc.dispatch(Some(&owner), &r1).unwrap().outcome {
        Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        Outcome::Error(e) => panic!("create 1 failed: {e:?}"),
    };
    assert_eq!(res1.task.key, "T-A1");

    let r2 = Request::new::<TasksCreate>(
        3,
        &TasksCreateParams {
            command_id: uuid(12),
            task_id: uuid(13),
            team_id: Some(root_team),
            project_id: None,
            assigned_team_id: None,
            assignee_id: None,
            task_type: Some("bug".into()),
            work_type: None,
            title: "Second bug".into(),
            body_markdown: String::new(),
            priority: None,
            effort: None,
            severity: Some("major".into()),
            rank_order: Some(1),
            labels: vec!["backend".into()],
            reference_files: vec![],
            modified_files: vec![],
            depends_on: vec![res1.task.key.clone()],
            attachment_ids: vec![],
            details: None,
            dod: None,
        },
    )
    .unwrap();
    let res2: TasksMutationResult = match svc.dispatch(Some(&owner), &r2).unwrap().outcome {
        Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        Outcome::Error(e) => panic!("create 2 failed: {e:?}"),
    };
    assert_eq!(res2.task.key, "T-A2");
    assert_eq!(res2.task.depends_on, vec![res1.task.id.clone()]);

    // Cycle refused through RPC.
    let cyc = Request::new::<TasksAddDependency>(
        4,
        &TasksAddDependencyParams {
            command_id: uuid(14),
            task: res1.task.key.clone(),
            depends_on_task: res2.task.key.clone(),
        },
    )
    .unwrap();
    assert!(matches!(
        svc.dispatch(Some(&owner), &cyc).unwrap().outcome,
        Outcome::Error(e) if e.code == ErrorCode::Conflict
    ));

    // Ordered list returns T-A2 (rank 1) before T-A1 (rank 10), and label AND filter narrows.
    let list_ordered = Request::new::<TasksList>(
        5,
        &TasksListParams {
            labels: vec!["backend".into(), "security".into()],
            label_match: LabelMatchMode::All,
            sort: TaskSortMode::Order,
            ..Default::default()
        },
    )
    .unwrap();
    let listed: org_proto::tasks::TasksListResult =
        match svc.dispatch(Some(&owner), &list_ordered).unwrap().outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("list failed: {e:?}"),
        };
    assert_eq!(listed.tasks.len(), 1);
    assert_eq!(listed.tasks[0].key, "T-A1");

    // Assign + transition + inspect round-trip.
    let assign_req = Request::new::<TasksAssign>(
        6,
        &TasksAssignParams {
            command_id: uuid(15),
            task: res1.task.key.clone(),
            expected_revision: res1.task.revision,
            assigned_team_id: PatchField::Unchanged,
            assignee_id: PatchField::Set(Some(owner.clone())),
        },
    )
    .unwrap();
    let assigned: TasksMutationResult =
        match svc.dispatch(Some(&owner), &assign_req).unwrap().outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("assign failed: {e:?}"),
        };
    let tr_req = Request::new::<TasksTransition>(
        7,
        &TasksTransitionParams {
            command_id: uuid(16),
            task: res1.task.key.clone(),
            expected_revision: assigned.task.revision,
            status: TaskStatus::InProgress,
            reason: None,
            blocked_reason: None,
            waiting_on_owner: None,
            resolution: None,
            note: None,
            done_anyway_reason: None,
            final_summary: None,
        },
    )
    .unwrap();
    assert!(matches!(
        svc.dispatch(Some(&owner), &tr_req).unwrap().outcome,
        Outcome::Result(_)
    ));
    let insp_req =
        Request::new::<TasksInspect>(8, &TasksInspectParams::new(res1.task.key)).unwrap();
    let inspected: org_proto::tasks::TaskInspectResult =
        match svc.dispatch(Some(&owner), &insp_req).unwrap().outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("inspect failed: {e:?}"),
        };
    assert_eq!(inspected.task.status, TaskStatus::InProgress);
    assert_eq!(inspected.dependent_tasks.len(), 1);
    assert_eq!(inspected.history.len(), 3);
}

#[test]
fn private_notes_never_emit_live_events_until_explicit_publish() {
    use org_proto::private_notes::{
        TasksNotesAddParams, TasksNotesPublishParams, TasksNotesUpdateParams,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let notifications = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&notifications);
    let live = Arc::new(LiveEvents::new(Box::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    })));
    let svc = TaskService::new(Arc::clone(&office), live, None);

    let create_req = Request::new::<TasksCreate>(
        1,
        &TasksCreateParams {
            command_id: uuid(100),
            task_id: uuid(101),
            team_id: Some(root_team),
            project_id: None,
            assigned_team_id: None,
            assignee_id: None,
            task_type: Some("task".into()),
            work_type: None,
            title: "Private note host test".into(),
            body_markdown: String::new(),
            priority: None,
            effort: None,
            severity: None,
            rank_order: None,
            labels: vec![],
            reference_files: vec![],
            modified_files: vec![],
            depends_on: vec![],
            attachment_ids: vec![],
            details: None,
            dod: None,
        },
    )
    .unwrap();
    assert!(matches!(
        svc.dispatch(Some(&owner), &create_req).unwrap().outcome,
        Outcome::Result(_)
    ));
    let before = notifications.load(Ordering::Relaxed);

    let note_id = uuid(102);
    let add_req = Request::new::<TasksNotesAdd>(
        2,
        &TasksNotesAddParams {
            command_id: uuid(103),
            task: "T-A1".into(),
            note_id: note_id.clone(),
            body_markdown: "Private draft".into(),
        }
        .into(),
    )
    .unwrap();
    assert!(matches!(
        svc.dispatch(Some(&owner), &add_req).unwrap().outcome,
        Outcome::Result(_)
    ));
    let edit_req = Request::new::<TasksNotesUpdate>(
        3,
        &TasksNotesUpdateParams {
            command_id: uuid(104),
            note_id: note_id.clone(),
            expected_revision: 1,
            body_markdown: "Refined private draft".into(),
        }
        .into(),
    )
    .unwrap();
    assert!(matches!(
        svc.dispatch(Some(&owner), &edit_req).unwrap().outcome,
        Outcome::Result(_)
    ));
    assert_eq!(
        notifications.load(Ordering::Relaxed),
        before,
        "private note add/update must not emit shared LiveEvents"
    );

    let pub_req = Request::new::<TasksNotesPublish>(
        4,
        &TasksNotesPublishParams {
            command_id: uuid(105),
            note_id,
            expected_revision: 2,
            comment_id: uuid(106),
        },
    )
    .unwrap();
    assert!(matches!(
        svc.dispatch(Some(&owner), &pub_req).unwrap().outcome,
        Outcome::Result(_)
    ));
    assert_eq!(notifications.load(Ordering::Relaxed), before + 1);
    // Replay does not emit a duplicate LiveEvents notification.
    assert!(matches!(
        svc.dispatch(Some(&owner), &pub_req).unwrap().outcome,
        Outcome::Result(_)
    ));
    assert_eq!(notifications.load(Ordering::Relaxed), before + 1);
}

#[test]
fn private_decisions_list_across_tasks_and_never_notify_live_events() {
    use org_proto::private_notes::{
        PrivateNoteAddParams, PrivateNoteField, PrivateNoteKind, PrivateNoteListParams,
        PrivateNoteUpdateParams, TasksNotesListResult, TasksNotesPublishParams,
        TasksNotesRemoveParams,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let notifications = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&notifications);
    let live = Arc::new(LiveEvents::new(Box::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    })));
    let svc = TaskService::new(Arc::clone(&office), live, None);
    let call = |request: Request| match svc.dispatch(Some(&owner), &request).unwrap().outcome {
        Outcome::Result(value) => Ok(value),
        Outcome::Error(e) => Err(e.code),
    };

    let decision_id = uuid(201);
    let add = Request::new::<TasksNotesAdd>(
        1,
        &PrivateNoteAddParams {
            command_id: uuid(200),
            note_id: decision_id.clone(),
            task: None,
            project_id: None,
            kind: PrivateNoteKind::Decision,
            title: None,
            why: Some("SECRET_WHY_SERVER".into()),
            alternatives: None,
            tags: vec!["Arch".into()],
            body_markdown: "Keep notes at HQ\nonly".into(),
            checkpoint: false,
        },
    )
    .unwrap();
    call(add).unwrap();
    let list = Request::new::<TasksNotesList>(
        2,
        &PrivateNoteListParams {
            kind: Some(PrivateNoteKind::Decision),
            tag: Some("arch".into()),
            fields: vec![PrivateNoteField::Title],
            ..Default::default()
        },
    )
    .unwrap();
    let listed: TasksNotesListResult = serde_json::from_value(call(list).unwrap()).unwrap();
    assert_eq!(listed.total, 1);
    assert_eq!(listed.task_id, None);
    let note = &listed.notes[0];
    assert_eq!(note.title.as_deref(), Some("Keep notes at HQ"));
    assert_eq!(
        (note.body_markdown.as_ref(), note.why.as_ref()),
        (None, None)
    );
    assert!(note.tags.is_empty(), "tags were not asked for");

    let edit = Request::new::<TasksNotesUpdate>(
        3,
        &PrivateNoteUpdateParams {
            command_id: uuid(202),
            note_id: decision_id.clone(),
            expected_revision: 1,
            kind: None,
            title: None,
            why: None,
            alternatives: None,
            tags: Some(vec!["recall".into()]),
            body_markdown: None,
        },
    )
    .unwrap();
    call(edit).unwrap();
    let publish = Request::new::<TasksNotesPublish>(
        4,
        &TasksNotesPublishParams {
            command_id: uuid(203),
            note_id: decision_id.clone(),
            expected_revision: 2,
            comment_id: uuid(204),
        },
    )
    .unwrap();
    assert_eq!(call(publish), Err(ErrorCode::InvalidRequest));
    let remove = Request::new::<TasksNotesRemove>(
        5,
        &TasksNotesRemoveParams {
            command_id: uuid(205),
            note_id: decision_id,
            expected_revision: 2,
        },
    )
    .unwrap();
    call(remove).unwrap();
    assert_eq!(
        notifications.load(Ordering::Relaxed),
        0,
        "private notes never reach shared LiveEvents"
    );
}

fn list(
    svc: &TaskService,
    owner: &str,
    params: TasksListParams,
) -> Result<TasksListResultView, ErrorCode> {
    let request = Request::new::<TasksList>(90, &params).unwrap();
    match svc.dispatch(Some(owner), &request).unwrap().outcome {
        Outcome::Result(v) => {
            let listed: org_proto::tasks::TasksListResult = serde_json::from_value(v).unwrap();
            Ok(TasksListResultView {
                keys: listed.tasks.into_iter().map(|t| t.key).collect(),
                total: listed.total,
            })
        }
        Outcome::Error(e) => Err(e.code),
    }
}

#[derive(Debug, PartialEq)]
struct TasksListResultView {
    keys: Vec<String>,
    total: u64,
}

fn view(keys: &[&str], total: u64) -> Result<TasksListResultView, ErrorCode> {
    Ok(TasksListResultView {
        keys: keys.iter().map(|k| (*k).to_owned()).collect(),
        total,
    })
}

#[test]
fn task_list_filters_by_excluded_status_assignee_and_modified_file_and_pages_with_a_total() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let live = Arc::new(LiveEvents::new(Box::new(|| {})));
    let svc = TaskService::new(Arc::clone(&office), live, None);
    for (n, file, assignee) in [
        (1u64, "src/a/x.rs", Some(owner.clone())),
        (2, "src/ab.rs", None),
        (3, "docs/y.md", None),
    ] {
        let create = Request::new::<TasksCreate>(
            n,
            &TasksCreateParams {
                command_id: uuid(100 + n),
                task_id: uuid(200 + n),
                team_id: Some(root_team.clone()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: assignee,
                task_type: Some("task".into()),
                work_type: None,
                title: format!("Task {n}"),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![file.into()],
                depends_on: vec![],
                attachment_ids: vec![],
                details: None,
                dod: None,
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &create).unwrap().outcome,
            Outcome::Result(_)
        ));
    }
    let by_number = || TasksListParams {
        sort: TaskSortMode::Number,
        ..Default::default()
    };
    assert_eq!(
        list(
            &svc,
            &owner,
            TasksListParams {
                unassigned: true,
                ..by_number()
            }
        ),
        view(&["T-A2", "T-A3"], 2)
    );
    // A directory names the files under it, component-wise: `src/a` is not `src/ab.rs`.
    assert_eq!(
        list(
            &svc,
            &owner,
            TasksListParams {
                modified_file: Some("./src/a/".into()),
                ..by_number()
            }
        ),
        view(&["T-A1"], 1)
    );
    assert_eq!(
        list(
            &svc,
            &owner,
            TasksListParams {
                exclude_status: vec![TaskStatus::Todo],
                ..by_number()
            }
        ),
        view(&[], 0)
    );
    // Pages: the total counts every match, filtered here or by the office.
    assert_eq!(
        list(
            &svc,
            &owner,
            TasksListParams {
                offset: Some(1),
                limit: Some(1),
                ..by_number()
            }
        ),
        view(&["T-A2"], 3)
    );
    assert_eq!(
        list(
            &svc,
            &owner,
            TasksListParams {
                unassigned: true,
                offset: Some(1),
                limit: Some(5),
                ..by_number()
            }
        ),
        view(&["T-A3"], 2)
    );
    assert_eq!(
        list(
            &svc,
            &owner,
            TasksListParams {
                limit: Some(0),
                ..by_number()
            }
        ),
        view(&[], 3)
    );
    for refused in [
        TasksListParams {
            status: Some(TaskStatus::Todo),
            exclude_status: vec![TaskStatus::Todo],
            ..by_number()
        },
        TasksListParams {
            unassigned: true,
            assignee_id: Some(owner.clone()),
            ..by_number()
        },
        TasksListParams {
            modified_file: Some("../etc".into()),
            ..by_number()
        },
    ] {
        assert_eq!(list(&svc, &owner, refused), Err(ErrorCode::InvalidRequest));
    }
}

#[test]
fn task_list_answers_a_status_scope_and_done_since_through_the_office_query() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let live = Arc::new(LiveEvents::new(Box::new(|| {})));
    let svc = TaskService::new(Arc::clone(&office), live, None);
    for n in 1u64..=3 {
        let create = Request::new::<TasksCreate>(
            n,
            &TasksCreateParams {
                command_id: uuid(300 + n),
                task_id: uuid(400 + n),
                team_id: Some(root_team.clone()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: format!("Task {n}"),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                attachment_ids: vec![],
                details: None,
                dod: None,
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &create).unwrap().outcome,
            Outcome::Result(_)
        ));
    }
    for (n, key, status) in [
        (1u64, "T-A2", TaskStatus::Done),
        (2, "T-A3", TaskStatus::Cancelled),
    ] {
        let transition = Request::new::<TasksTransition>(
            10 + n,
            &TasksTransitionParams {
                command_id: uuid(500 + n),
                task: key.into(),
                expected_revision: 1,
                status,
                reason: None,
                blocked_reason: None,
                waiting_on_owner: None,
                resolution: None,
                note: None,
                done_anyway_reason: None,
                final_summary: None,
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &transition).unwrap().outcome,
            Outcome::Result(_)
        ));
    }
    let scoped = |scope, since: Option<&str>| TasksListParams {
        status_scope: scope,
        done_since: since.map(str::to_owned),
        sort: TaskSortMode::Number,
        ..Default::default()
    };
    let long_ago = Some("1970-01-02T00:00:00Z");
    let far_ahead = Some("9999-12-31T23:59:59.999Z");
    let cases = [
        (scoped(None, None), view(&["T-A1", "T-A2", "T-A3"], 3)),
        (
            scoped(Some(TaskStatusScope::All), None),
            view(&["T-A1", "T-A2", "T-A3"], 3),
        ),
        (
            scoped(Some(TaskStatusScope::Open), None),
            view(&["T-A1"], 1),
        ),
        (
            scoped(Some(TaskStatusScope::Closed), None),
            view(&["T-A2", "T-A3"], 2),
        ),
        // A board: open work and what was done since; cancelled work stays off it.
        (
            scoped(Some(TaskStatusScope::Open), long_ago),
            view(&["T-A1", "T-A2"], 2),
        ),
        (
            scoped(Some(TaskStatusScope::Open), far_ahead),
            view(&["T-A1"], 1),
        ),
        (
            scoped(Some(TaskStatusScope::Closed), far_ahead),
            view(&["T-A3"], 1),
        ),
        (scoped(None, far_ahead), view(&["T-A1", "T-A3"], 2)),
    ];
    for (params, expected) in cases {
        assert_eq!(list(&svc, &owner, params.clone()), expected, "{params:?}");
    }
    for refused in [
        scoped(Some(TaskStatusScope::Open), Some("2026-11-20")),
        TasksListParams {
            status: Some(TaskStatus::Done),
            ..scoped(Some(TaskStatusScope::Open), None)
        },
    ] {
        assert_eq!(list(&svc, &owner, refused), Err(ErrorCode::InvalidRequest));
    }
}

/// One dispatched call's answer: its result, or the refusal's code.
fn answer<C: Call>(
    svc: &TaskService,
    member: &str,
    params: &C::Params,
) -> Result<C::Result, ErrorCode>
where
    C::Result: serde::de::DeserializeOwned,
{
    let request = Request::new::<C>(1, params).unwrap();
    match svc.dispatch(Some(member), &request).unwrap().outcome {
        Outcome::Result(v) => Ok(serde_json::from_value(v).unwrap()),
        Outcome::Error(e) => Err(e.code),
    }
}

fn create_params(n: u64, team: Option<&str>, project: Option<&str>) -> TasksCreateParams {
    TasksCreateParams {
        command_id: uuid(n),
        task_id: uuid(n + 1),
        team_id: team.map(str::to_owned),
        project_id: project.map(str::to_owned),
        assigned_team_id: None,
        assignee_id: None,
        task_type: None,
        work_type: None,
        title: format!("Task {n}"),
        body_markdown: String::new(),
        priority: None,
        effort: None,
        severity: None,
        rank_order: None,
        labels: vec![],
        reference_files: vec![],
        modified_files: vec![],
        depends_on: vec![],
        attachment_ids: vec![],
        details: None,
        dod: None,
    }
}

#[test]
fn task_service_serves_default_definitions_of_done_the_closing_gate_and_final_summaries() {
    use org_office::rules::OrgCommand;
    use org_proto::org::ProjectCreateParams;
    use org_proto::tasks::{
        OrganizationDefaultDodGetParams, OrganizationDefaultDodResult,
        OrganizationDefaultDodSetParams, ProjectDefaultDodSetParams, TasksFinalSummaryParams,
    };

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let live = Arc::new(LiveEvents::new(Box::new(|| {})));
    let svc = TaskService::new(Arc::clone(&office), live, None);

    let get = OrganizationDefaultDodGetParams {};
    let org: OrganizationDefaultDodResult =
        answer::<OrganizationDefaultDodGet>(&svc, &owner, &get).unwrap();
    assert_eq!(org.default_dod, None);
    let set = |n: u64, revision: u64, items: Vec<&str>| OrganizationDefaultDodSetParams {
        command_id: uuid(n),
        expected_revision: revision,
        default_dod: Some(items.into_iter().map(str::to_owned).collect()),
    };
    // Owner only: anyone else, member or not, is refused.
    let stranger = uuid(3000);
    let refused =
        answer::<OrganizationDefaultDodSet>(&svc, &stranger, &set(3001, org.revision, vec!["x"]));
    assert_eq!(refused.unwrap_err(), ErrorCode::NotAuthorized);
    let bad =
        answer::<OrganizationDefaultDodSet>(&svc, &owner, &set(3002, org.revision, vec!["a\nb"]));
    assert_eq!(bad.unwrap_err(), ErrorCode::InvalidRequest);
    let many: Vec<String> = (0..26).map(|i| format!("item {i}")).collect();
    let many = set(
        3003,
        org.revision,
        many.iter().map(String::as_str).collect(),
    );
    assert_eq!(
        answer::<OrganizationDefaultDodSet>(&svc, &owner, &many).unwrap_err(),
        ErrorCode::TooLarge
    );
    let stale =
        answer::<OrganizationDefaultDodSet>(&svc, &owner, &set(3004, org.revision + 7, vec!["x"]));
    assert_eq!(stale.unwrap_err(), ErrorCode::Conflict);
    let changed = answer::<OrganizationDefaultDodSet>(
        &svc,
        &owner,
        &set(3005, org.revision, vec![" Tests pass "]),
    )
    .unwrap();
    assert_eq!(changed.default_dod, Some(vec!["Tests pass".to_owned()]));
    assert_eq!(changed.revision, org.revision + 1);
    assert_eq!(
        answer::<OrganizationDefaultDodGet>(&svc, &owner, &get).unwrap(),
        changed
    );

    // A project that opts out of the organization's default.
    let project = uuid(3010);
    office
        .submit_org(
            &owner,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: uuid(3011),
                project_id: project.clone(),
                name: "Ledger".into(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: None,
                owner_team_id: Some(root_team.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: vec![],
            }),
        )
        .unwrap()
        .unwrap();
    let opt_out = ProjectDefaultDodSetParams {
        command_id: uuid(3012),
        project: project.clone(),
        expected_revision: 1,
        default_dod: Some(vec![]),
    };
    let record = answer::<ProjectDefaultDodSet>(&svc, &owner, &opt_out).unwrap();
    assert_eq!(record.default_dod, Some(vec![]));
    let missing = ProjectDefaultDodSetParams {
        project: "P-99".into(),
        command_id: uuid(3013),
        ..opt_out.clone()
    };
    assert_eq!(
        answer::<ProjectDefaultDodSet>(&svc, &owner, &missing).unwrap_err(),
        ErrorCode::NotFound
    );

    let in_project =
        answer::<TasksCreate>(&svc, &owner, &create_params(3020, None, Some(&project)))
            .unwrap()
            .task;
    assert_eq!(in_project.dod_total, 0);
    let task = answer::<TasksCreate>(&svc, &owner, &create_params(3022, Some(&root_team), None))
        .unwrap()
        .task;
    assert_eq!((task.dod_total, task.dod_done), (1, 0));

    // Completing with the copied step unticked is refused over RPC, unless a reason is given.
    let close = |n: u64, revision: u64, reason: Option<&str>| TasksTransitionParams {
        command_id: uuid(n),
        task: task.key.clone(),
        expected_revision: revision,
        status: TaskStatus::Done,
        reason: None,
        blocked_reason: None,
        waiting_on_owner: None,
        resolution: None,
        note: None,
        done_anyway_reason: reason.map(str::to_owned),
        final_summary: Some("Closed with the docs waived".into()),
    };
    let gated = answer::<TasksTransition>(&svc, &owner, &close(3030, task.revision, None));
    assert_eq!(gated.unwrap_err(), ErrorCode::ChecklistIncomplete);
    let closed =
        answer::<TasksTransition>(&svc, &owner, &close(3031, task.revision, Some("Waived")))
            .unwrap()
            .task;
    assert_eq!(closed.status, TaskStatus::Done);
    assert_eq!(
        closed
            .details
            .and_then(|d| d.done_anyway)
            .map(|d| d.reason)
            .as_deref(),
        Some("Waived")
    );
    assert_eq!(
        closed.final_summary.as_ref().map(|s| s.text.as_str()),
        Some("Closed with the docs waived")
    );

    let summary = TasksFinalSummaryParams {
        command_id: uuid(3040),
        task: task.key.clone(),
        expected_revision: closed.revision,
        final_summary: "Shipped; docs follow in T-A9".into(),
    };
    let summarized = answer::<TasksFinalSummary>(&svc, &owner, &summary)
        .unwrap()
        .task;
    let written = summarized.final_summary.expect("summary");
    assert_eq!(written.text, "Shipped; docs follow in T-A9");
    assert_eq!(written.by_id, owner);
    assert_eq!(summarized.revision, closed.revision + 1);
}

#[test]
fn tasks_inspect_server_side_fields_pruning_omits_unrequested_wire_keys() {
    use org_proto::tasks::{
        ChecklistSection, TasksChecklistAddParams, TasksCommentParams, TasksWatchParams,
    };

    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let live = Arc::new(LiveEvents::new(Box::new(|| {})));
    let svc = TaskService::new(Arc::clone(&office), live, None);

    let mut params = create_params(4001, Some(&root_team), None);
    params.body_markdown = "LARGE_MARKDOWN_BODY_TO_PRUNE".into();
    let task = answer::<TasksCreate>(&svc, &owner, &params).unwrap().task;
    let _ = answer::<TasksChecklistAdd>(
        &svc,
        &owner,
        &TasksChecklistAddParams {
            command_id: uuid(4002),
            task: task.key.clone(),
            step_id: uuid(4003),
            text: "Wire step".into(),
            required: Some(true),
            section: Some(ChecklistSection::Acceptance),
            steps: vec![],
        },
    )
    .unwrap();
    let _ = answer::<TasksComment>(
        &svc,
        &owner,
        &TasksCommentParams {
            command_id: uuid(4004),
            task: task.key.clone(),
            comment_id: uuid(4005),
            body_markdown: "Public wire comment".into(),
            attachment_ids: vec![],
        },
    )
    .unwrap();
    let _ = answer::<TasksWatch>(
        &svc,
        &owner,
        &TasksWatchParams {
            command_id: uuid(4006),
            task: task.key.clone(),
        },
    )
    .unwrap();

    // Pruned request omits body_markdown and unrequested top-level keys on the wire.
    let pruned_req = Request::new::<TasksInspect>(
        10,
        &TasksInspectParams {
            task: task.key.clone(),
            fields: Some(vec![
                "key".into(),
                "title".into(),
                "status".into(),
                "revision".into(),
                "checklist".into(),
            ]),
        },
    )
    .unwrap();
    let Outcome::Result(wire_pruned) = svc.dispatch(Some(&owner), &pruned_req).unwrap().outcome
    else {
        panic!("expected pruned inspect result");
    };
    let obj = wire_pruned.as_object().expect("json object");
    assert!(obj["task"].get("body_markdown").is_none(), "{wire_pruned}");
    assert_eq!(obj["checklist"].as_array().unwrap().len(), 1);
    for omitted in [
        "comments",
        "attachments",
        "watchers",
        "depends_on_tasks",
        "dependent_tasks",
        "history",
    ] {
        assert!(
            !obj.contains_key(omitted),
            "expected `{omitted}` omitted: {wire_pruned}"
        );
    }

    // Unfiltered request returns full body and sections.
    let full_req = Request::new::<TasksInspect>(11, &TasksInspectParams::new(task.key)).unwrap();
    let Outcome::Result(wire_full) = svc.dispatch(Some(&owner), &full_req).unwrap().outcome else {
        panic!("expected full inspect result");
    };
    assert_eq!(
        wire_full["task"]["body_markdown"],
        "LARGE_MARKDOWN_BODY_TO_PRUNE"
    );
    assert_eq!(wire_full["comments"].as_array().unwrap().len(), 1);
    assert_eq!(wire_full["watchers"].as_array().unwrap().len(), 1);
    assert!(!wire_full["history"].as_array().unwrap().is_empty());

    // Invalid, unknown, or empty `fields` entries fail closed with InvalidRequest.
    for bad in [
        vec!["chcklist".to_owned()],
        vec!["".to_owned()],
        vec!["task.nonexistent".to_owned()],
        vec!["checklist.".to_owned()],
        vec![],
    ] {
        let bad_req = Request::new::<TasksInspect>(
            12,
            &TasksInspectParams {
                task: "T-A1".into(),
                fields: Some(bad.clone()),
            },
        )
        .unwrap();
        let Outcome::Error(err) = svc.dispatch(Some(&owner), &bad_req).unwrap().outcome else {
            panic!("expected InvalidRequest for bad fields {bad:?}");
        };
        assert_eq!(err.code, ErrorCode::InvalidRequest, "{bad:?}");
    }
}

#[test]
fn task_list_query_uses_fts5_stemming_comments_and_preserves_structured_filters() {
    use org_office::EngineChoice;
    use org_proto::tasks::TasksCommentParams;

    for engine in [
        EngineChoice::Memory,
        EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()),
    ] {
        let office = Arc::new(Office::for_test(TestOffice {
            engine: engine.clone(),
        }));
        let owner = office.owner_member_id().to_owned();
        let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
        let live = Arc::new(LiveEvents::new(Box::new(|| {})));
        let svc = TaskService::new(Arc::clone(&office), live, None);

        let make_task = |n: u64, title: &str, body: &str, labels: Vec<&str>, rank: u64| {
            let req = Request::new::<TasksCreate>(
                n,
                &TasksCreateParams {
                    command_id: uuid(800 + n),
                    task_id: uuid(900 + n),
                    team_id: Some(root_team.clone()),
                    project_id: None,
                    assigned_team_id: None,
                    assignee_id: None,
                    task_type: Some("task".into()),
                    work_type: None,
                    title: title.into(),
                    body_markdown: body.into(),
                    priority: None,
                    effort: None,
                    severity: None,
                    rank_order: Some(rank),
                    labels: labels.into_iter().map(str::to_owned).collect(),
                    reference_files: vec![],
                    modified_files: vec![],
                    depends_on: vec![],
                    attachment_ids: vec![],
                    details: None,
                    dod: None,
                },
            )
            .unwrap();
            match svc.dispatch(Some(&owner), &req).unwrap().outcome {
                Outcome::Result(v) => {
                    serde_json::from_value::<TasksMutationResult>(v)
                        .unwrap()
                        .task
                }
                Outcome::Error(e) => panic!("create task failed ({engine:?}): {e:?}"),
            }
        };

        let t1 = make_task(
            1,
            "Stabilize socket retries",
            "Handle transient connection drops cleanly.",
            vec!["backend", "reliability"],
            10,
        );
        let t2 = make_task(
            2,
            "Audit UI spacing",
            "Button alignment polish.",
            vec!["frontend"],
            5,
        );
        let _t3 = make_task(
            3,
            "Backoff policy note",
            "Different topic in body.",
            vec!["backend"],
            1,
        );

        // Add a public comment mentioning "retrying" on T-A3 so search matches via comment!
        let comment_req = Request::new::<TasksComment>(
            20,
            &TasksCommentParams {
                command_id: uuid(850),
                task: "T-A3".into(),
                comment_id: uuid(851),
                body_markdown: "Investigate retrying exponential backoff on sockets.".into(),
                attachment_ids: vec![],
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &comment_req).unwrap().outcome,
            Outcome::Result(_)
        ));

        // On SQLite FTS5, Porter stemming matches `"retry"` against `"retries"` and `"retrying"`.
        // On Memory, prefix matching matches `"retr"` against both.
        let query_word = match engine {
            EngineChoice::Sqlite(_) => "retry",
            EngineChoice::Memory => "retr",
        };
        let fts_req = Request::new::<TasksList>(
            21,
            &TasksListParams {
                query: Some(query_word.into()),
                sort: TaskSortMode::Order,
                ..Default::default()
            },
        )
        .unwrap();
        let res: org_proto::tasks::TasksListResult =
            match svc.dispatch(Some(&owner), &fts_req).unwrap().outcome {
                Outcome::Result(v) => serde_json::from_value(v).unwrap(),
                Outcome::Error(e) => panic!("fts list failed ({engine:?}): {e:?}"),
            };
        assert_eq!(res.total, 2, "{engine:?}");
        let keys: Vec<&str> = res.tasks.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, vec!["T-A3", "T-A1"], "{engine:?}");

        // Combined with structured filters (`labels`, `offset`, `limit`) and `--count` (`limit: Some(0)`).
        let filtered_req = Request::new::<TasksList>(
            22,
            &TasksListParams {
                query: Some(query_word.into()),
                labels: vec!["reliability".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let filtered: org_proto::tasks::TasksListResult =
            match svc.dispatch(Some(&owner), &filtered_req).unwrap().outcome {
                Outcome::Result(v) => serde_json::from_value(v).unwrap(),
                Outcome::Error(e) => panic!("filtered fts list failed ({engine:?}): {e:?}"),
            };
        assert_eq!(filtered.total, 1, "{engine:?}");
        assert_eq!(filtered.tasks[0].id, t1.id, "{engine:?}");

        let count_req = Request::new::<TasksList>(
            23,
            &TasksListParams {
                query: Some(query_word.into()),
                limit: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        let counted: org_proto::tasks::TasksListResult =
            match svc.dispatch(Some(&owner), &count_req).unwrap().outcome {
                Outcome::Result(v) => serde_json::from_value(v).unwrap(),
                Outcome::Error(e) => panic!("count fts list failed ({engine:?}): {e:?}"),
            };
        assert_eq!(counted.total, 2, "{engine:?}");
        assert!(counted.tasks.is_empty(), "{engine:?}");

        // Pagination (`offset: 1, limit: 1`) preserves total=2 and returns second item.
        let paged_req = Request::new::<TasksList>(
            24,
            &TasksListParams {
                query: Some(query_word.into()),
                sort: TaskSortMode::Order,
                offset: Some(1),
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        let paged: org_proto::tasks::TasksListResult =
            match svc.dispatch(Some(&owner), &paged_req).unwrap().outcome {
                Outcome::Result(v) => serde_json::from_value(v).unwrap(),
                Outcome::Error(e) => panic!("paged fts list failed ({engine:?}): {e:?}"),
            };
        assert_eq!(paged.total, 2, "{engine:?}");
        assert_eq!(paged.tasks.len(), 1, "{engine:?}");
        assert_eq!(paged.tasks[0].key, "T-A1", "{engine:?}");

        // Non-matching query still validates contradictory structured filters (not swallowed).
        let bad_req = Request::new::<TasksList>(
            25,
            &TasksListParams {
                query: Some("nonexistenttermxyz".into()),
                status: Some(TaskStatus::Todo),
                exclude_status: vec![TaskStatus::Todo],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &bad_req).unwrap().outcome,
            Outcome::Error(e) if e.code == ErrorCode::InvalidRequest
        ));

        // Zero hits with valid filters returns empty list and total=0.
        let empty_req = Request::new::<TasksList>(
            26,
            &TasksListParams {
                query: Some("nonexistenttermxyz".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let empty: org_proto::tasks::TasksListResult =
            match svc.dispatch(Some(&owner), &empty_req).unwrap().outcome {
                Outcome::Result(v) => serde_json::from_value(v).unwrap(),
                Outcome::Error(e) => panic!("empty fts list failed ({engine:?}): {e:?}"),
            };
        assert_eq!(empty.total, 0, "{engine:?}");
        assert!(empty.tasks.is_empty(), "{engine:?}");
        let _ = t2;
    }
}
