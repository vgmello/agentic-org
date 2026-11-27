//! How `tasks.list` parameters become one query, and what they refuse (`TASK-105`).

use org_proto::tasks::{TaskStatus, TaskStatusScope, TasksListParams};
use org_proto::ErrorCode;
use org_store::{testkit::MemoryEngine, Engine};

use super::{done_since, list_query, normalized_path, statuses};
use crate::tasks::features::decode_refusal;
use crate::tasks::query::TaskStatusSet;

use TaskStatus::{Blocked, Cancelled, Done, InProgress, Todo};

fn params(
    scope: Option<TaskStatusScope>,
    status: Option<TaskStatus>,
    exclude: &[TaskStatus],
    since: Option<&str>,
) -> TasksListParams {
    TasksListParams {
        status_scope: scope,
        status,
        exclude_status: exclude.to_vec(),
        done_since: since.map(str::to_owned),
        ..TasksListParams::default()
    }
}

fn set(statuses: &[TaskStatus]) -> TaskStatusSet {
    statuses
        .iter()
        .fold(TaskStatusSet::EMPTY, |set, status| set.with(*status))
}

const SINCE: Option<&str> = Some("2026-11-20T00:00:00Z");

#[test]
fn the_status_filters_compose_into_one_set() {
    use TaskStatusScope::{All, Closed, Open};
    let cases: [(TasksListParams, TaskStatusSet); 10] = [
        // No lifecycle filter: every status, as before `TASK-105`.
        (params(None, None, &[], None), TaskStatusSet::ALL),
        (params(Some(All), None, &[], None), TaskStatusSet::ALL),
        (params(Some(Open), None, &[], None), TaskStatusSet::OPEN),
        (params(Some(Closed), None, &[], None), TaskStatusSet::CLOSED),
        // A board: the open work and what was done since.
        (
            params(Some(Open), None, &[], SINCE),
            set(&[Todo, InProgress, Blocked, Done]),
        ),
        // `done_since` narrows the done tasks of other scopes; it never widens them.
        (
            params(Some(Closed), None, &[], SINCE),
            TaskStatusSet::CLOSED,
        ),
        (params(None, None, &[], SINCE), TaskStatusSet::ALL),
        (
            params(Some(Open), Some(Blocked), &[], None),
            set(&[Blocked]),
        ),
        (params(Some(Open), Some(Done), &[], SINCE), set(&[Done])),
        (
            params(Some(Open), None, &[Blocked, Cancelled], SINCE),
            set(&[Todo, InProgress, Done]),
        ),
    ];
    for (given, expected) in cases {
        assert_eq!(statuses(&given).ok(), Some(expected), "{given:?}");
    }
}

#[test]
fn contradicting_status_filters_are_refused() {
    use TaskStatusScope::{Closed, Open};
    for given in [
        params(Some(Open), Some(Done), &[], None),
        params(Some(Open), Some(Cancelled), &[], SINCE),
        params(Some(Closed), Some(Todo), &[], None),
        params(None, Some(Todo), &[Todo], None),
    ] {
        let refused = statuses(&given).expect_err("a contradiction");
        assert_eq!(
            decode_refusal(&refused).code,
            ErrorCode::InvalidRequest,
            "{given:?}"
        );
    }
}

#[test]
fn done_since_accepts_rfc_3339_instants_only() {
    assert_eq!(done_since(None).ok(), Some(None));
    let instants = [
        ("2026-11-20T00:00:00Z", 1_795_132_800_000),
        ("2026-11-20T00:00:00.250Z", 1_795_132_800_250),
        ("2026-11-20T02:00:00+02:00", 1_795_132_800_000),
        (" 2026-11-20T00:00:00Z ", 1_795_132_800_000),
    ];
    for (raw, ms) in instants {
        assert_eq!(done_since(Some(raw)).ok(), Some(Some(ms)), "{raw}");
    }
    for bad in [
        "",
        "yesterday",
        "2026-11-20",
        "2026-11-20T00:00:00",
        "2026-11-20 00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-02-30T00:00:00Z",
        "2026-11-20T00:00:00.Z",
        "1969-12-31T23:59:59Z",
        "2026-11-20T00:00:00Z and later",
    ] {
        let refused = done_since(Some(bad)).expect_err(bad);
        assert_eq!(decode_refusal(&refused).code, ErrorCode::InvalidRequest);
    }
}

#[test]
fn a_modified_file_filter_is_normalized_or_refused() {
    assert_eq!(
        normalized_path(Some("./src/a/")).ok().flatten().as_deref(),
        Some("src/a")
    );
    assert_eq!(normalized_path(None).ok(), Some(None));
    for bad in ["", "  ", "./", "/", ".", "src/../etc", ".."] {
        assert!(normalized_path(Some(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn list_query_refuses_conflicts_and_unknown_dependency_references() {
    let engine = MemoryEngine::new();
    let view = engine.snapshot().unwrap();
    let refused = |given: TasksListParams| {
        let refusal = list_query(&view, &given).expect_err("refused");
        decode_refusal(&refusal).code
    };
    assert_eq!(
        refused(TasksListParams {
            unassigned: true,
            assignee_id: Some("someone".into()),
            ..TasksListParams::default()
        }),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        refused(TasksListParams {
            done_since: Some("last week".into()),
            ..TasksListParams::default()
        }),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        refused(TasksListParams {
            depends_on: Some("T-ZZ9".into()),
            ..TasksListParams::default()
        }),
        ErrorCode::NotFound
    );
    assert_eq!(
        refused(TasksListParams {
            ready: true,
            dependency_blocked: true,
            ..TasksListParams::default()
        }),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        refused(TasksListParams {
            ready: true,
            status: Some(Blocked),
            ..TasksListParams::default()
        }),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        refused(TasksListParams {
            ready: true,
            status_scope: Some(TaskStatusScope::Closed),
            ..TasksListParams::default()
        }),
        ErrorCode::InvalidRequest
    );

    let ready_q = list_query(
        &view,
        &TasksListParams {
            ready: true,
            ..TasksListParams::default()
        },
    )
    .expect("valid ready query");
    assert_eq!(ready_q.statuses, set(&[Todo, InProgress]));
    assert_eq!(ready_q.dependency_blocked, Some(false));

    let dep_q = list_query(
        &view,
        &TasksListParams {
            dependency_blocked: true,
            ..TasksListParams::default()
        },
    )
    .expect("valid dependency_blocked query");
    assert_eq!(dep_q.dependency_blocked, Some(true));

    let query = list_query(
        &view,
        &TasksListParams {
            task_type: Some(" Bug ".into()),
            team_id: Some(" team ".into()),
            query: Some("  Auth Token ".into()),
            labels: vec!["Backend".into(), "backend".into()],
            offset: Some(20),
            limit: Some(10),
            status_scope: Some(TaskStatusScope::Open),
            done_since: SINCE.map(str::to_owned),
            ..TasksListParams::default()
        },
    )
    .expect("a valid query");
    assert_eq!(query.task_type.as_deref(), Some("Bug"));
    assert_eq!(query.team_id.as_deref(), Some("team"));
    assert_eq!(query.text.as_deref(), Some("auth token"));
    assert_eq!(query.labels.map(|l| l.labels), Some(vec!["backend".into()]));
    assert_eq!((query.offset, query.limit), (20, Some(10)));
    assert_eq!(query.done_since_ms, Some(1_795_132_800_000));
    assert!(query.statuses.contains(Done) && !query.statuses.contains(Cancelled));
}
