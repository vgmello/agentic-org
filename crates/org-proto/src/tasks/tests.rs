//! Wire tests for the `tasks.list` lifecycle filters (`TASK-105`).

use serde_json::json;

use super::{TaskStatus, TaskStatusScope, TasksListParams};

const STATUSES: [TaskStatus; 5] = [
    TaskStatus::Todo,
    TaskStatus::InProgress,
    TaskStatus::Blocked,
    TaskStatus::Cancelled,
    TaskStatus::Done,
];

#[test]
fn a_status_scope_round_trips_in_snake_case_and_admits_its_half_of_the_lifecycle() {
    for scope in [
        TaskStatusScope::Open,
        TaskStatusScope::Closed,
        TaskStatusScope::All,
    ] {
        let wire = serde_json::to_value(scope).unwrap();
        assert_eq!(wire, json!(scope.as_str()));
        assert_eq!(TaskStatusScope::parse(scope.as_str()), Some(scope));
        assert_eq!(
            serde_json::from_value::<TaskStatusScope>(wire).unwrap(),
            scope
        );
    }
    assert_eq!(TaskStatusScope::parse("Open"), None);
    assert_eq!(TaskStatusScope::parse(""), None);
    assert!(serde_json::from_value::<TaskStatusScope>(json!("everything")).is_err());

    let closed: Vec<TaskStatus> = STATUSES.into_iter().filter(|s| s.is_closed()).collect();
    assert_eq!(closed, [TaskStatus::Cancelled, TaskStatus::Done]);
    for status in STATUSES {
        assert_eq!(TaskStatusScope::Open.admits(status), !status.is_closed());
        assert_eq!(TaskStatusScope::Closed.admits(status), status.is_closed());
        assert!(TaskStatusScope::All.admits(status));
    }
}

#[test]
fn list_params_without_the_lifecycle_filters_keep_their_old_wire_shape() {
    // A caller that predates `TASK-105` sends neither field and reads the same JSON back.
    let old = json!({"status": "todo", "limit": 5, "offset": 10});
    let params: TasksListParams = serde_json::from_value(old).unwrap();
    assert_eq!(params.status_scope, None);
    assert_eq!(params.done_since, None);
    assert_eq!(
        serde_json::to_value(TasksListParams::default()).unwrap(),
        json!({"label_match": "all", "sort": "order"})
    );

    let new = json!({"status_scope": "open", "done_since": "2026-11-20T00:00:00Z"});
    let params: TasksListParams = serde_json::from_value(new.clone()).unwrap();
    assert_eq!(params.status_scope, Some(TaskStatusScope::Open));
    assert_eq!(params.done_since.as_deref(), Some("2026-11-20T00:00:00Z"));
    let echoed = serde_json::to_value(&params).unwrap();
    assert_eq!(echoed["status_scope"], new["status_scope"]);
    assert_eq!(echoed["done_since"], new["done_since"]);

    assert!(serde_json::from_value::<TasksListParams>(json!({"status_scope": "recent"})).is_err());

    // `TASK-84`: `ready` and `dependency_blocked` default false and omit when false.
    let blocking_params = TasksListParams {
        ready: true,
        ..TasksListParams::default()
    };
    let wire = serde_json::to_value(&blocking_params).unwrap();
    assert_eq!(wire["ready"], json!(true));
    assert!(wire.get("dependency_blocked").is_none());

    let dep_params: TasksListParams =
        serde_json::from_value(json!({"dependency_blocked": true})).unwrap();
    assert!(dep_params.dependency_blocked);
    assert!(!dep_params.ready);
}

#[test]
fn blocking_dependency_and_history_params_round_trip() {
    use super::{BlockingDependencyWire, TasksHistoryParams, TasksTransitionParams};

    let dep = BlockingDependencyWire {
        id: "0193a5c0-0000-7000-8000-000000000101".into(),
        key: "T-A1".into(),
        title: "Upstream".into(),
        status: TaskStatus::Cancelled,
        cancelled: true,
    };
    let wire = serde_json::to_value(&dep).unwrap();
    assert_eq!(
        serde_json::from_value::<BlockingDependencyWire>(wire).unwrap(),
        dep
    );

    let hist = TasksHistoryParams {
        task: "T-A2".into(),
        event_type: Some("status_changed".into()),
        blocks_only: true,
    };
    let hist_wire = serde_json::to_value(&hist).unwrap();
    assert_eq!(
        serde_json::from_value::<TasksHistoryParams>(hist_wire).unwrap(),
        hist
    );
    let legacy_hist: TasksHistoryParams = serde_json::from_value(json!({"task": "T-A2"})).unwrap();
    assert_eq!(legacy_hist, TasksHistoryParams::new("T-A2"));

    let tr: TasksTransitionParams = serde_json::from_value(json!({
        "command_id": "0193a5c0-0000-7000-8000-000000000001",
        "task": "T-A2",
        "expected_revision": 2,
        "status": "blocked",
        "reason": "Waiting on DNS",
        "waiting_on_owner": true
    }))
    .unwrap();
    assert_eq!(tr.reason.as_deref(), Some("Waiting on DNS"));
    assert_eq!(tr.waiting_on_owner, Some(true));
}
