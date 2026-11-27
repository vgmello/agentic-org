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
}
