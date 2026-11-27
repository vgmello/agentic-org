//! The SQLite tasks adapter answers every batched read exactly as the `MemoryEngine`
//! reference does (`ENG-1`), and a `tasks.list` page costs a fixed number of statements,
//! stays fast at thousands of tasks, and reads open work from the open-task partial indexes
//! (`TASK-105`).

use std::ops::RangeInclusive;
use std::time::{Duration, Instant};

use org_proto::tasks::{
    LabelMatchMode, TaskPriority, TaskSortMode, TaskStatus, TaskStatusScope, TasksListParams,
};
use org_proto::ErrorCode;
use org_store::{testkit::MemoryEngine, Engine, Id};
use rusqlite::params_from_iter;

use super::super::SqliteEngine;
use super::list::{filter, page_statement};
use super::statements;
use crate::tasks::features::{decode_refusal, inspect_task, list_tasks};
use crate::tasks::ports::{TasksRead, TasksWrite};
use crate::tasks::query::{TaskListQuery, TaskStatusSet};
use crate::tasks::records::{
    ChecklistStepRecord, TaskCommentRecord, TaskDetailsRecord, TaskEventRecord, TaskLinkRecord,
    TaskMeta, TaskRecord, TaskRepositoryRecord, TaskWatcherRecord,
};
use crate::time::parse_rfc3339_ms;

const ORG: &str = "01900000-0000-7000-8000-00000000a105";

/// A canonical-looking ID of `kind` (1 task, 2 team, 3 project, 4 person, 5 child row).
fn id(kind: u16, n: u64) -> Id {
    Id::parse(&format!("01900000-{kind:04x}-7000-8000-{n:012x}")).expect("an ID")
}

fn sqlite() -> (SqliteEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir
        .path()
        .canonicalize()
        .expect("a real path")
        .join("office.db");
    let organization = Id::parse(ORG).expect("an ID");
    let engine = SqliteEngine::open(&path).expect("a new projection");
    (engine.for_organization(&organization), dir)
}

const STATUSES: [TaskStatus; 5] = [
    TaskStatus::Todo,
    TaskStatus::InProgress,
    TaskStatus::Blocked,
    TaskStatus::Done,
    TaskStatus::Cancelled,
];

/// Task `n`'s `updated_at`: every RFC 3339 form, plus none and one that is no time at all.
fn updated_at(n: u64) -> Option<String> {
    let day = 1 + n % 28;
    let hour = n % 24;
    match n % 19 {
        0 => None,
        1 => Some("not a time".to_owned()),
        2 => Some(format!("2026-11-{day:02}T{hour:02}:30:00Z")),
        3 => Some(format!("2026-11-{day:02}T{hour:02}:30:00.250+02:00")),
        _ => Some(format!("2026-11-{day:02}T{hour:02}:30:00.{:03}Z", n % 1000)),
    }
}

/// Task `n`: owners, statuses, labels, files and times spread so every filter splits them.
fn task(n: u64) -> TaskRecord {
    let labels = ["backend", "p0", "security", "ui"];
    let labels: Vec<String> = (0..4)
        .filter(|bit| n >> bit & 1 == 1)
        .map(|bit| labels[bit].to_owned())
        .collect();
    let modified_files = match n % 5 {
        0 => vec!["src/a/b.rs".to_owned()],
        1 => vec!["./src/a".to_owned()],
        2 => vec!["src/ab.rs".to_owned(), "src/z.rs".to_owned()],
        _ => Vec::new(),
    };
    let titles = [
        "Auth token rotation",
        "Billing export",
        "UI polish 100%_done",
    ];
    TaskRecord {
        id: id(1, n),
        office_code: if n.is_multiple_of(7) { "B" } else { "A" }.to_owned(),
        number: n,
        team_id: id(2, n % 3),
        assigned_team_id: (n.is_multiple_of(5)).then(|| id(2, 1)),
        project_id: (!n.is_multiple_of(4)).then(|| id(3, n % 4)),
        task_type: ["task", "bug", "BUG"][(n % 3) as usize].to_owned(),
        work_type: None,
        title: format!("Task {n} {}", titles[(n % 3) as usize]),
        status: STATUSES[(n % 5) as usize],
        assignee_id: (!n.is_multiple_of(3)).then(|| id(4, n % 4)),
        body_markdown: if n.is_multiple_of(2) {
            "Fix the LOGIN flow"
        } else {
            ""
        }
        .to_owned(),
        feature_id: None,
        milestone_id: None,
        priority: [TaskPriority::Normal, TaskPriority::High][(n % 2) as usize],
        effort: None,
        severity: None,
        resolution: None,
        blocked_reason: None,
        rank_order: (!n.is_multiple_of(4)).then_some((n * 37) % 11),
        labels,
        reference_files: if n.is_multiple_of(6) {
            vec!["docs/spec.md".to_owned()]
        } else {
            Vec::new()
        },
        modified_files,
        repositories: if n.is_multiple_of(9) {
            vec![TaskRepositoryRecord {
                repository_id: id(6, 1),
                branch: Some("main".to_owned()),
                integrated_commit: None,
            }]
        } else {
            Vec::new()
        },
        details: (n.is_multiple_of(8)).then(|| TaskDetailsRecord {
            definition_of_done: Some("shipped".to_owned()),
            ..TaskDetailsRecord::default()
        }),
        meta: TaskMeta {
            revision: 1,
            created_at: Some("2026-11-01T00:00:00.000Z".to_owned()),
            created_by_kind: "system".to_owned(),
            created_by_id: None,
            updated_at: updated_at(n),
            updated_by_kind: "system".to_owned(),
            updated_by_id: None,
        },
    }
}

fn seed<Tx: TasksWrite>(tx: &mut Tx, range: RangeInclusive<u64>) {
    for n in range {
        let task = task(n);
        tx.put_task(&task).expect("a task row");
        if n.is_multiple_of(3) {
            let steps: Vec<ChecklistStepRecord> = (0..2)
                .map(|p| ChecklistStepRecord {
                    id: id(5, n * 10 + p),
                    position: p,
                    text: format!("step {p}"),
                    required: true,
                    done: p == 0,
                    linked_task_id: None,
                    checked_by_employee_id: None,
                    checked_at: None,
                    revision: 1,
                })
                .collect();
            tx.put_checklist_steps(&task.id, &steps).expect("steps");
        }
        if n % 4 == 1 {
            let comment = TaskCommentRecord {
                task_id: task.id.clone(),
                id: id(7, n),
                position: 0,
                author_id: id(4, 1),
                created_at: "2026-11-02T00:00:00.000Z".to_owned(),
                body_markdown: "looks good".to_owned(),
            };
            tx.put_task_comment(&comment).expect("a comment");
        }
        if n % 5 == 2 {
            let watcher = TaskWatcherRecord {
                id: id(8, n),
                watcher_kind: "employee".to_owned(),
                watcher_id: Some(id(4, 2)),
            };
            tx.put_task_watchers(&task.id, &[watcher])
                .expect("a watcher");
        }
        if n > 2 && n.is_multiple_of(6) {
            let links: Vec<TaskLinkRecord> = [n - 1, n - 2]
                .into_iter()
                .map(|target| TaskLinkRecord {
                    source_id: task.id.clone(),
                    id: id(9, n * 10 + target % 10),
                    kind: "depends_on".to_owned(),
                    target_kind: "task".to_owned(),
                    target_id: id(1, target),
                })
                .collect();
            tx.put_task_links(&task.id, &links).expect("links");
        }
    }
}

fn scoped(scope: Option<TaskStatusScope>, since: Option<&str>) -> TasksListParams {
    TasksListParams {
        status_scope: scope,
        done_since: since.map(str::to_owned),
        ..TasksListParams::default()
    }
}

/// Every filter alone and combined, every order, pages, and refusals.
fn questions() -> Vec<TasksListParams> {
    use TaskStatusScope::{All, Closed, Open};
    let mut out = Vec::new();
    let sinces = [
        None,
        Some("2026-11-15T00:00:00Z"),
        Some("2026-11-15T02:00:00+02:00"),
        Some("2026-11-10T12:30:00.250Z"),
        Some("2026-11-01T00:00:00.001Z"),
    ];
    for scope in [None, Some(Open), Some(Closed), Some(All)] {
        for since in sinces {
            for sort in [
                TaskSortMode::Order,
                TaskSortMode::Number,
                TaskSortMode::UpdatedDesc,
            ] {
                out.push(TasksListParams {
                    sort,
                    ..scoped(scope, since)
                });
            }
        }
    }
    let open = || scoped(Some(Open), None);
    let board = || scoped(Some(Open), Some("2026-11-12T00:00:00Z"));
    let p = |n| Some(id(3, n).as_str().to_owned());
    let t = |n| Some(id(2, n).as_str().to_owned());
    let labels = |names: &[&str], mode| TasksListParams {
        labels: names.iter().map(|l| (*l).to_owned()).collect(),
        label_match: mode,
        ..open()
    };
    out.extend([
        TasksListParams {
            project_id: p(1),
            ..open()
        },
        TasksListParams {
            project_id: p(2),
            ..board()
        },
        TasksListParams {
            team_id: t(1),
            ..open()
        },
        TasksListParams {
            team_id: t(2),
            sort: TaskSortMode::Number,
            ..board()
        },
        TasksListParams {
            assigned_team_id: t(1),
            ..TasksListParams::default()
        },
        TasksListParams {
            assignee_id: Some(id(4, 1).as_str().to_owned()),
            ..open()
        },
        TasksListParams {
            assignee_id: Some(format!(" {} ", id(4, 2))),
            ..board()
        },
        TasksListParams {
            unassigned: true,
            ..open()
        },
        labels(&["backend", "security"], LabelMatchMode::All),
        labels(&["UI", "p0"], LabelMatchMode::Any),
        labels(&["missing"], LabelMatchMode::Any),
        TasksListParams {
            modified_file: Some("src/a".into()),
            ..TasksListParams::default()
        },
        TasksListParams {
            modified_file: Some("./src/a/b.rs".into()),
            ..open()
        },
        TasksListParams {
            modified_file: Some("src/a/".into()),
            project_id: p(3),
            ..board()
        },
        TasksListParams {
            query: Some("auth".into()),
            ..open()
        },
        TasksListParams {
            query: Some("100%_".into()),
            ..TasksListParams::default()
        },
        TasksListParams {
            query: Some("%".into()),
            ..TasksListParams::default()
        },
        TasksListParams {
            query: Some("T-B7".into()),
            ..TasksListParams::default()
        },
        TasksListParams {
            query: Some("login".into()),
            team_id: t(0),
            ..board()
        },
        TasksListParams {
            query: Some("spec.md".into()),
            ..TasksListParams::default()
        },
        TasksListParams {
            task_type: Some(" bug ".into()),
            ..open()
        },
        TasksListParams {
            exclude_status: vec![TaskStatus::Blocked],
            ..board()
        },
        TasksListParams {
            status: Some(TaskStatus::Done),
            ..board()
        },
        TasksListParams {
            status: Some(TaskStatus::Cancelled),
            ..scoped(Some(Closed), Some("2026-11-20T00:00:00Z"))
        },
        TasksListParams {
            depends_on: Some("T-A11".into()),
            ..TasksListParams::default()
        },
        TasksListParams {
            dependent_of: Some("T-A12".into()),
            ..TasksListParams::default()
        },
        TasksListParams {
            dependent_of: Some(id(1, 24).as_str().to_owned()),
            ..open()
        },
        TasksListParams {
            limit: Some(0),
            ..board()
        },
        TasksListParams {
            limit: Some(7),
            offset: Some(14),
            ..board()
        },
        TasksListParams {
            offset: Some(100_000),
            ..TasksListParams::default()
        },
        TasksListParams {
            offset: Some(u64::MAX),
            limit: Some(3),
            ..open()
        },
        // Refused alike.
        TasksListParams {
            status: Some(TaskStatus::Done),
            ..open()
        },
        TasksListParams {
            done_since: Some("2026-11-15".into()),
            ..open()
        },
        TasksListParams {
            depends_on: Some("T-Z1".into()),
            ..TasksListParams::default()
        },
    ]);
    out
}

#[test]
fn both_engines_answer_every_list_question_alike() {
    let (engine, _dir) = sqlite();
    let memory = MemoryEngine::new();
    let mut on_sqlite = engine.begin().unwrap();
    let mut in_memory = memory.begin().unwrap();
    seed(&mut on_sqlite, 1..=240);
    seed(&mut in_memory, 1..=240);
    let questions = questions();
    let mut answered = 0;
    for params in &questions {
        let sqlite = list_tasks(&on_sqlite, params).map_err(|r| decode_refusal(&r));
        let memory = list_tasks(&in_memory, params).map_err(|r| decode_refusal(&r));
        assert_eq!(sqlite, memory, "{params:?}");
        answered += usize::from(sqlite.is_ok_and(|page| !page.tasks.is_empty()));
    }
    // All but the 3 refusals and the 4 pages that are empty by design (an unknown label,
    // `limit = 0`, and two offsets past the end) list tasks.
    assert_eq!(answered, questions.len() - 7);

    assert_eq!(on_sqlite.tasks().unwrap(), in_memory.tasks().unwrap());
    let ids: Vec<Id> = (0..=245).map(|n| id(1, n)).collect();
    assert_eq!(
        on_sqlite.task_rollups(&ids).unwrap(),
        in_memory.task_rollups(&ids).unwrap()
    );
    for reference in ["T-A1", "T-A12", "T-B42", "T-A240"] {
        assert_eq!(
            inspect_task(&on_sqlite, reference).ok(),
            inspect_task(&in_memory, reference).ok(),
            "{reference}"
        );
    }
}

/// What the scopes and `done_since` must answer, checked against the seed itself.
#[test]
fn scopes_and_done_since_keep_the_tasks_their_rules_name() {
    let memory = MemoryEngine::new();
    let mut tx = memory.begin().unwrap();
    seed(&mut tx, 1..=240);
    let (engine, _dir) = sqlite();
    let mut on_sqlite = engine.begin().unwrap();
    seed(&mut on_sqlite, 1..=240);
    let since = "2026-11-15T00:00:00Z";
    let since_ms = parse_rfc3339_ms(since).unwrap();
    let recent = |n: u64| {
        updated_at(n)
            .as_deref()
            .and_then(parse_rfc3339_ms)
            .is_some_and(|at| at >= since_ms)
    };
    let expect = |keep: &dyn Fn(u64) -> bool| (1..=240).filter(|n| keep(*n)).count() as u64;
    let status = |n: u64| task(n).status;
    let cases: [(TasksListParams, u64); 5] = [
        (scoped(None, None), 240),
        (
            scoped(Some(TaskStatusScope::Open), None),
            expect(&|n| !status(n).is_closed()),
        ),
        (
            scoped(Some(TaskStatusScope::Closed), None),
            expect(&|n| status(n).is_closed()),
        ),
        (
            scoped(Some(TaskStatusScope::Open), Some(since)),
            expect(&|n| !status(n).is_closed() || (status(n) == TaskStatus::Done && recent(n))),
        ),
        (
            scoped(Some(TaskStatusScope::Closed), Some(since)),
            expect(&|n| {
                status(n) == TaskStatus::Cancelled || (status(n) == TaskStatus::Done && recent(n))
            }),
        ),
    ];
    for (params, total) in cases {
        for view in [&on_sqlite as &dyn ListView, &tx as &dyn ListView] {
            let page = view.list(&params);
            assert_eq!(page.0, total, "{params:?}");
            assert_eq!(page.1, total as usize, "{params:?}");
        }
    }
    // Paging walks the one order: the pages together are the whole answer.
    let board = scoped(Some(TaskStatusScope::Open), Some(since));
    let whole = list_tasks(&on_sqlite, &board).unwrap();
    let mut paged = Vec::new();
    let mut offset = 0;
    loop {
        let params = TasksListParams {
            offset: Some(offset),
            limit: Some(7),
            ..board.clone()
        };
        let page = list_tasks(&on_sqlite, &params).unwrap();
        assert_eq!(page.total, whole.total);
        if page.tasks.is_empty() {
            break;
        }
        offset += page.tasks.len() as u64;
        paged.extend(page.tasks);
    }
    assert_eq!(paged, whole.tasks);
}

/// A view `list_tasks` can run on, as a trait object for the table above.
trait ListView {
    fn list(&self, params: &TasksListParams) -> (u64, usize);
}

impl<R: TasksRead> ListView for R {
    fn list(&self, params: &TasksListParams) -> (u64, usize) {
        let page = list_tasks(self, params).expect("an answer");
        (page.total, page.tasks.len())
    }
}

#[test]
fn history_outlines_match_the_events_on_both_engines() {
    let (engine, _dir) = sqlite();
    let memory = MemoryEngine::new();
    let mut on_sqlite = engine.begin().unwrap();
    let mut in_memory = memory.begin().unwrap();
    let event = |task: u64, revision: u64, kind: &str| TaskEventRecord {
        task_id: id(1, task),
        revision,
        schema_version: 2,
        occurred_at: Some("2026-11-02T00:00:00.000Z".to_owned()),
        actor_kind: "system".to_owned(),
        actor_id: None,
        operation_id: None,
        change_json: format!(r#"{{"type":"{kind}"}}"#),
        source_metadata_json: "{}".to_owned(),
        payload_sha256: format!("{:064x}", task * 100 + revision),
        resource_profile: "selected-task-resources/1".to_owned(),
    };
    let events = [
        event(1, 1, "created"),
        event(1, 2, "status_changed"),
        event(2, 2, "status_changed"),
        event(3, 1, "edited"),
        event(3, 3, "edited"),
    ];
    for tx in [&mut on_sqlite as &mut dyn EventSink, &mut in_memory] {
        for e in &events {
            tx.put(e);
        }
    }
    let sqlite = on_sqlite.task_history_outlines().unwrap();
    assert_eq!(sqlite, in_memory.task_history_outlines().unwrap());
    assert_eq!(sqlite[&id(1, 1)].revisions, [1, 2]);
    assert_eq!(
        sqlite[&id(1, 1)].created_change_json.as_deref(),
        Some(r#"{"type":"created"}"#)
    );
    assert_eq!(sqlite[&id(1, 2)].created_change_json, None);
    assert_eq!(sqlite[&id(1, 3)].revisions, [1, 3]);
    assert!(!sqlite.contains_key(&id(1, 4)));
}

trait EventSink {
    fn put(&mut self, event: &TaskEventRecord);
}

impl<Tx: TasksWrite> EventSink for Tx {
    fn put(&mut self, event: &TaskEventRecord) {
        self.put_task_event(event).expect("an event");
    }
}

/// The statements one `tasks.list` call runs on SQLite.
fn statements_of<R: TasksRead>(view: &R, params: &TasksListParams) -> (u64, usize) {
    let before = statements::run();
    let page = list_tasks(view, params).expect("an answer");
    (statements::run() - before, page.tasks.len())
}

#[test]
fn a_page_costs_the_same_few_statements_at_50_or_2000_tasks() {
    let board = || TasksListParams {
        limit: Some(50),
        ..scoped(Some(TaskStatusScope::Open), Some("2026-11-10T00:00:00Z"))
    };
    let questions = [
        TasksListParams {
            limit: Some(50),
            ..TasksListParams::default()
        },
        board(),
        TasksListParams {
            limit: Some(50),
            labels: vec!["backend".into()],
            query: Some("task".into()),
            ..TasksListParams::default()
        },
    ];
    let mut costs = Vec::new();
    for size in [50, 2_000] {
        let (engine, _dir) = sqlite();
        let mut tx = engine.begin().unwrap();
        seed(&mut tx, 1..=size);
        let mut row = Vec::new();
        for params in &questions {
            let (cost, listed) = statements_of(&tx, params);
            assert!(listed > 0, "{params:?}");
            row.push(cost);
        }
        let before = statements::run();
        assert_eq!(tx.tasks().unwrap().len() as u64, size);
        row.push(statements::run() - before);
        eprintln!("TASK-105 statements per call at {size} tasks: {row:?}");
        costs.push(row);
    }
    assert_eq!(
        costs[0], costs[1],
        "no statement grows with the organization"
    );
    for cost in &costs[1][..3] {
        // A count, the page, 4 hydration reads and 5 rollup reads.
        assert!(*cost <= 15, "a page costs at most 15 statements: {cost}");
    }
    assert_eq!(costs[1][3], 5, "every task, whole, is 5 statements");
}

fn plan(tx: &super::super::SqliteTx, query: &TaskListQuery) -> String {
    let (sql, values) = page_statement(filter(ORG, query).unwrap(), query);
    let mut explain = tx
        .db()
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap();
    let rows = explain
        .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
        .unwrap();
    rows.collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
        .join("\n")
}

#[test]
fn open_pages_read_in_order_from_the_open_task_partial_indexes() {
    let (engine, _dir) = sqlite();
    let mut tx = engine.begin().unwrap();
    seed(&mut tx, 1..=300);
    let open = |project: Option<u64>, team: Option<u64>| TaskListQuery {
        statuses: TaskStatusSet::OPEN,
        project_id: project.map(|n| id(3, n).as_str().to_owned()),
        team_id: team.map(|n| id(2, n).as_str().to_owned()),
        limit: Some(50),
        ..TaskListQuery::default()
    };
    let by_project = plan(&tx, &open(Some(1), None));
    eprintln!("TASK-105 open-by-project plan: {by_project}");
    assert!(by_project.contains("tasks_open_by_project"), "{by_project}");
    assert!(!by_project.contains("TEMP B-TREE"), "{by_project}");
    let by_team = plan(&tx, &open(None, Some(2)));
    eprintln!("TASK-105 open-by-team plan: {by_team}");
    assert!(by_team.contains("tasks_open_by_team"), "{by_team}");
    assert!(!by_team.contains("TEMP B-TREE"), "{by_team}");
}

#[test]
fn an_open_page_of_a_2400_task_organization_is_fast() {
    let (engine, _dir) = sqlite();
    let mut tx = engine.begin().unwrap();
    seed(&mut tx, 1..=2_400);
    let questions = [
        (
            "open project page",
            TasksListParams {
                project_id: Some(id(3, 1).as_str().to_owned()),
                limit: Some(50),
                ..scoped(Some(TaskStatusScope::Open), None)
            },
        ),
        (
            "open team board",
            TasksListParams {
                team_id: Some(id(2, 2).as_str().to_owned()),
                limit: Some(50),
                ..scoped(Some(TaskStatusScope::Open), Some("2026-11-20T00:00:00Z"))
            },
        ),
    ];
    for (name, params) in questions {
        // The best of five runs: a busy machine slows some runs, never the fastest of them.
        let mut runs = Vec::new();
        let mut listed = 0;
        for _ in 0..5 {
            let started = Instant::now();
            let page = list_tasks(&tx, &params).expect("an answer");
            runs.push(started.elapsed());
            listed = page.tasks.len();
            assert!(page.total >= 50, "{name}: {}", page.total);
        }
        let best = runs.iter().min().copied().unwrap_or_default();
        eprintln!(
            "TASK-105 {name}: {listed} of 2400 tasks, best {:.2} ms, runs {:?}",
            best.as_secs_f64() * 1000.0,
            runs.iter()
                .map(|r| format!("{:.2}", r.as_secs_f64() * 1000.0))
                .collect::<Vec<_>>()
        );
        assert_eq!(listed, 50);
        assert!(best < Duration::from_millis(50), "{name}: {best:?}");
    }
}

#[test]
fn an_invalid_done_since_is_refused_before_any_statement() {
    let (engine, _dir) = sqlite();
    let tx = engine.begin().unwrap();
    let before = statements::run();
    let refused = list_tasks(
        &tx,
        &TasksListParams {
            done_since: Some("2026-11-20T00:00:00Z' OR 1=1 --".into()),
            ..TasksListParams::default()
        },
    )
    .expect_err("refused");
    assert_eq!(decode_refusal(&refused).code, ErrorCode::InvalidRequest);
    assert_eq!(statements::run(), before);
}
