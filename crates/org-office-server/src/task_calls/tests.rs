use super::*;
use org_office::TestOffice;
use org_proto::tasks::{
    LabelMatchMode, PatchField, TaskSortMode, TaskStatus, TasksAddDependencyParams,
    TasksAssignParams, TasksCreateParams, TasksInspectParams, TasksListParams,
    TasksTransitionParams,
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
            details: None,
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
            details: None,
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
            blocked_reason: None,
            resolution: None,
            note: None,
        },
    )
    .unwrap();
    assert!(matches!(
        svc.dispatch(Some(&owner), &tr_req).unwrap().outcome,
        Outcome::Result(_)
    ));
    let insp_req = Request::new::<TasksInspect>(
        8,
        &TasksInspectParams {
            task: res1.task.key,
        },
    )
    .unwrap();
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
            details: None,
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
        },
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
        },
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
