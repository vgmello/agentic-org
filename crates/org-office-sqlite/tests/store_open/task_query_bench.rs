//! `TASK-105` AC #4: O(1) constant bounded SQL statement count across 10 / 100 / 500 tasks
//! (zero N+1 growth), `< 5 ms` median and p95 filtered `tasks.list` latency at `2,200+` seeded
//! tasks with realistic open+closed distribution, `EXPLAIN QUERY PLAN` partial open-task index
//! verification, and WAL checkpoint non-starvation under repeated snapshot reads.

use std::time::{Duration, Instant};

use org_office::organization_family::ports::OrganizationWrite;
use org_office::organization_family::records::{
    By, Employee, EmployeeKind, EmployeeStatus, Meta, Office, OfficeCode, OfficeRole, Organization,
    Repository, RepositoryKind, Team, TeamStatus, DEFAULT_MAX_MEMBERS,
};
use org_office::projects::ports::ProjectsWrite;
use org_office::projects::records::Project;
use org_office::tasks::features::list_tasks;
use org_office::tasks::ports::TasksWrite;
use org_office::tasks::query::{TaskListQuery, TaskStatusSet};
use org_office::tasks::records::{
    ChecklistStepRecord, TaskCommentRecord, TaskLinkRecord, TaskMeta, TaskRecord, TaskWatcherRecord,
};
use org_office_sqlite::{task_sql_statement_count, SqliteEngine};
use org_proto::tasks::{
    ChecklistSection, TaskPriority, TaskStatus, TaskStatusScope, TasksListParams,
};
use org_store::{Cursor, Engine, FeedPosition, HighWaters, Id, Sha256};

const BENCH_ORG: &str = "01900000-0000-7000-8000-00000000b105";

fn bench_id(kind: u16, n: u64) -> Id {
    Id::parse(&format!("01900000-{kind:04x}-7000-8000-{n:012x}")).expect("valid UUIDv7")
}

fn open_bench_sqlite() -> (SqliteEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir
        .path()
        .canonicalize()
        .expect("real path")
        .join("office.db");
    let org = Id::parse(BENCH_ORG).expect("org id");
    let engine = SqliteEngine::open(&path)
        .expect("open sqlite engine")
        .for_organization(&org);
    (engine, dir)
}

fn bench_meta() -> Meta {
    Meta {
        revision: 1,
        created_at: Some("2026-09-01T00:00:00.000Z".to_owned()),
        created_by: By::System,
        updated_at: Some("2026-09-01T00:00:00.000Z".to_owned()),
        updated_by: By::System,
    }
}

/// Seeds `total` tasks with a realistic Kanban distribution (~90% historical closed, ~10% open)
/// spread across 5 projects and 4 teams, with labels, modified files, checklist steps, comments,
/// watchers, and `depends_on` links, then commits cleanly so WAL snapshots serve every read.
fn seed_realistic_org(engine: &SqliteEngine, total: u64, open_count: u64) {
    let mut tx = engine.begin().expect("begin write tx");
    let org_id = Id::parse(BENCH_ORG).expect("org id");
    let repo_id = bench_id(9, 1);
    let office_id = bench_id(10, 1);
    let meta = bench_meta();

    tx.put_organization(&Organization {
        id: org_id,
        slug: "bench-org".to_owned(),
        name: "Bench Org".to_owned(),
        organization_repository_id: repo_id.clone(),
        state_branch: "main".to_owned(),
        default_dod: None,
        meta: meta.clone(),
    })
    .expect("put org");
    tx.put_repositories(&[Repository {
        id: repo_id,
        key: Some("org".to_owned()),
        kind: RepositoryKind::Remote,
        remote_url: Some("https://example.invalid/bench-org.git".to_owned()),
        meta: meta.clone(),
    }])
    .expect("put repo");
    tx.put_office(&Office {
        id: office_id.clone(),
        slug: "hq".to_owned(),
        name: "HQ".to_owned(),
        role: OfficeRole::Hq,
        meta: meta.clone(),
    })
    .expect("put office");
    tx.put_office_codes(&[OfficeCode {
        code: "A".to_owned(),
        office_id,
        issued_at: "2026-09-01T00:00:00.000Z".to_owned(),
        retired_at: None,
    }])
    .expect("put office codes");
    for t in 1..=4 {
        tx.put_team(&Team {
            id: bench_id(2, t),
            name: format!("Team {t}"),
            number: t,
            parent_team_id: None,
            lead_employee_id: None,
            status: TeamStatus::Active,
            max_members: DEFAULT_MAX_MEMBERS,
            meta: meta.clone(),
        })
        .expect("put team");
    }
    for p in 1..=5 {
        tx.put_project(&Project {
            id: bench_id(3, p),
            number: p,
            slug: format!("project-{p}"),
            name: format!("Project {p}"),
            owner_team_id: bench_id(2, 1 + (p % 4)),
            archived: false,
            development_process_id: None,
            contributing_team_ids: Vec::new(),
            repositories: Vec::new(),
            default_dod: None,
            meta: meta.clone(),
        })
        .expect("put project");
    }
    for e in 1..=6 {
        tx.put_employee(&Employee {
            id: bench_id(4, e),
            handle: format!("person{e}"),
            kind: EmployeeKind::Person,
            name: format!("Person {e}"),
            home_team_id: bench_id(2, 1),
            role_id: None,
            reports_to_employee_id: None,
            status: EmployeeStatus::Active,
            contact: None,
            title: None,
            focus: None,
            role_text: None,
            title_in_team: None,
            offboarded_at: None,
            may_hire: true,
            builtin_key: None,
            meta: meta.clone(),
            body: String::new(),
        })
        .expect("put employee");
    }
    for n in 1..=total {
        let is_open = n > total.saturating_sub(open_count);
        let status = if is_open {
            match n % 7 {
                0 => TaskStatus::Blocked,
                1..=3 => TaskStatus::InProgress,
                _ => TaskStatus::Todo,
            }
        } else if n % 7 == 0 {
            TaskStatus::Cancelled
        } else {
            TaskStatus::Done
        };
        let day = 1 + (n % 26);
        let updated_at = if is_open || n > total.saturating_sub(open_count + 20) {
            format!("2026-11-{day:02}T12:00:00.000Z")
        } else {
            format!("2026-09-{day:02}T12:00:00.000Z")
        };
        let proj_idx = 1 + (n % 5);
        let team_idx = 1 + (proj_idx % 4);
        let task = TaskRecord {
            id: bench_id(1, n),
            office_code: "A".to_owned(),
            number: n,
            team_id: bench_id(2, team_idx),
            assigned_team_id: None,
            project_id: Some(bench_id(3, proj_idx)),
            task_type: if n % 4 == 0 { "bug" } else { "task" }.to_owned(),
            work_type: None,
            title: format!("Bench task #{n} subsystem pipeline"),
            status,
            assignee_id: (n % 3 != 0).then(|| bench_id(4, 1 + (n % 6))),
            body_markdown: format!("Work description for benchmark task {n}"),
            feature_id: None,
            milestone_id: None,
            priority: if n % 5 == 0 {
                TaskPriority::High
            } else {
                TaskPriority::Normal
            },
            effort: Some((n % 5) + 1),
            severity: None,
            resolution: status.is_closed().then(|| {
                if status == TaskStatus::Done {
                    "completed".to_owned()
                } else {
                    "cancelled".to_owned()
                }
            }),
            blocked_reason: (status == TaskStatus::Blocked)
                .then(|| format!("Waiting on external vendor review #{n}")),
            blocked_since: (status == TaskStatus::Blocked)
                .then(|| "2026-11-20T09:00:00.000Z".to_owned()),
            waiting_on_owner: status == TaskStatus::Blocked && n % 10 == 0,
            rank_order: Some((n * 17) % 100),
            labels: vec![
                if n % 2 == 0 { "backend" } else { "infra" }.to_owned(),
                format!("wave-{}", n % 3),
            ],
            reference_files: vec!["docs/arch.md".to_owned()],
            modified_files: vec![format!("crates/org-office/src/m{}.rs", n % 8)],
            repositories: Vec::new(),
            details: None,
            final_summary: None,
            meta: TaskMeta {
                revision: 1,
                created_at: Some("2026-09-01T00:00:00.000Z".to_owned()),
                created_by_kind: "system".to_owned(),
                created_by_id: None,
                updated_at: Some(updated_at),
                updated_by_kind: "system".to_owned(),
                updated_by_id: None,
            },
        };
        tx.put_task(&task).expect("put task");
        if n % 3 == 0 {
            let steps = vec![ChecklistStepRecord {
                id: bench_id(5, n),
                section: ChecklistSection::Acceptance,
                position: 0,
                text: "Verify acceptance".to_owned(),
                required: true,
                done: status == TaskStatus::Done,
                linked_task_id: None,
                checked_by_employee_id: None,
                checked_at: None,
                revision: 1,
            }];
            tx.put_checklist_steps(&task.id, &steps).expect("put steps");
        }
        if n % 4 == 0 {
            tx.put_task_comment(&TaskCommentRecord {
                task_id: task.id.clone(),
                id: bench_id(6, n),
                position: 0,
                author_id: bench_id(4, 1),
                created_at: "2026-11-10T00:00:00.000Z".to_owned(),
                body_markdown: "Checked progress".to_owned(),
            })
            .expect("put comment");
        }
        if n % 5 == 0 {
            tx.put_task_watchers(
                &task.id,
                &[TaskWatcherRecord {
                    id: bench_id(7, n),
                    watcher_kind: "employee".to_owned(),
                    watcher_id: Some(bench_id(4, 2)),
                }],
            )
            .expect("put watcher");
        }
        // Wire dependencies among open tasks so `ready` and `dependency_blocked` exercise joins.
        if is_open && n > 2 && n % 4 == 0 {
            tx.put_task_links(
                &task.id,
                &[TaskLinkRecord {
                    source_id: task.id.clone(),
                    id: bench_id(8, n),
                    kind: "depends_on".to_owned(),
                    target_kind: "task".to_owned(),
                    target_id: bench_id(1, n - 1),
                }],
            )
            .expect("put link");
        }
    }
    engine
        .commit(
            tx,
            &Cursor {
                ordinal: 1,
                sha256: Sha256::of(b"task-105-ac4-bench"),
                feed: FeedPosition::default(),
                high_waters: HighWaters::default(),
            },
        )
        .expect("commit seed");
}

fn measure_statements(
    snap: &org_office_sqlite::SqliteSnapshot,
    params: &TasksListParams,
) -> (u64, usize) {
    let before = task_sql_statement_count();
    let res = list_tasks(snap, params).expect("list_tasks succeeds");
    (task_sql_statement_count() - before, res.tasks.len())
}

#[test]
fn task_105_ac4_constant_bounded_sql_statement_count_across_10_100_500_tasks() {
    let mut all_costs = Vec::new();
    for size in [10u64, 100, 500] {
        let (engine, _dir) = open_bench_sqlite();
        seed_realistic_org(&engine, size, size);
        let snap = engine.snapshot().expect("snapshot");

        let (cost_all, returned_all) = measure_statements(
            &snap,
            &TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                limit: Some(size as u32),
                ..TasksListParams::default()
            },
        );
        assert_eq!(returned_all as u64, size);

        let (cost_ready, returned_ready) = measure_statements(
            &snap,
            &TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                ready: true,
                limit: Some(size as u32),
                ..TasksListParams::default()
            },
        );
        assert!(returned_ready > 0, "size={size}");

        let (cost_dep, returned_dep) = measure_statements(
            &snap,
            &TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                dependency_blocked: true,
                limit: Some(size as u32),
                ..TasksListParams::default()
            },
        );
        assert!(returned_dep > 0, "size={size}");

        eprintln!(
            "TASK-105 AC #4 SQL statement count (size={size}): open={cost_all}, ready={cost_ready}, dep_blocked={cost_dep}"
        );
        // 1 COUNT(*) + 1 page SELECT + 4 batched hydration + 5 batched rollups = 11 statements.
        assert!(
            cost_all <= 12,
            "expected O(1) <= 12 statements, got {cost_all}"
        );
        assert_eq!(cost_all, cost_ready);
        assert_eq!(cost_all, cost_dep);
        all_costs.push(cost_all);
    }
    assert_eq!(
        all_costs[0], all_costs[1],
        "SQL statement count must not grow from 10 to 100 returned tasks: {all_costs:?}"
    );
    assert_eq!(
        all_costs[1], all_costs[2],
        "SQL statement count must not grow from 100 to 500 returned tasks: {all_costs:?}"
    );
}

#[test]
fn task_105_ac4_sub_5ms_filtered_latency_at_2200_tasks_partial_index_and_wal_checkpoint() {
    let (engine, _dir) = open_bench_sqlite();
    // 2,200 seeded tasks: 2,000 historical closed + 200 active open across 5 projects & 4 teams
    // (~40 open tasks per project, ~50 open tasks per team).
    seed_realistic_org(&engine, 2_200, 200);

    // 1. Verify partial open-task indexes are used with zero TEMP B-TREE.
    let proj_plan = engine
        .explain_task_list_plan(&TaskListQuery {
            statuses: TaskStatusSet::OPEN,
            project_id: Some(bench_id(3, 1).as_str().to_owned()),
            limit: Some(25),
            ..TaskListQuery::default()
        })
        .expect("explain project plan");
    assert!(
        proj_plan.contains("tasks_open_by_project"),
        "expected tasks_open_by_project partial index: {proj_plan}"
    );
    assert!(
        !proj_plan.contains("TEMP B-TREE"),
        "unexpected TEMP B-TREE in project plan: {proj_plan}"
    );

    let team_plan = engine
        .explain_task_list_plan(&TaskListQuery {
            statuses: TaskStatusSet::OPEN,
            team_id: Some(bench_id(2, 1).as_str().to_owned()),
            limit: Some(25),
            ..TaskListQuery::default()
        })
        .expect("explain team plan");
    assert!(
        team_plan.contains("tasks_open_by_team"),
        "expected tasks_open_by_team partial index: {team_plan}"
    );
    assert!(
        !team_plan.contains("TEMP B-TREE"),
        "unexpected TEMP B-TREE in team plan: {team_plan}"
    );

    // 2. Measure warm filtered `tasks.list` latency across 25 runs via fresh WAL snapshots.
    let cases = [
        (
            "open project page (limit 20)",
            TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                project_id: Some(bench_id(3, 1).as_str().to_owned()),
                limit: Some(20),
                ..TasksListParams::default()
            },
        ),
        (
            "ready project page (limit 20)",
            TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                project_id: Some(bench_id(3, 1).as_str().to_owned()),
                ready: true,
                limit: Some(20),
                ..TasksListParams::default()
            },
        ),
        (
            "dependency_blocked open page",
            TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                project_id: Some(bench_id(3, 1).as_str().to_owned()),
                dependency_blocked: true,
                limit: Some(20),
                ..TasksListParams::default()
            },
        ),
        (
            "open team board (done_since 7d)",
            TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                team_id: Some(bench_id(2, 1).as_str().to_owned()),
                done_since: Some("2026-11-20T00:00:00Z".to_owned()),
                limit: Some(20),
                ..TasksListParams::default()
            },
        ),
    ];

    // Warm SQLite page cache once before measuring.
    {
        let snap = engine.snapshot().expect("warm snapshot");
        let _ = list_tasks(&snap, &cases[0].1).expect("warm list");
    }

    for (label, params) in &cases {
        let mut samples = Vec::with_capacity(25);
        let mut stmt_cost = 0u64;
        for _ in 0..25 {
            let snap = engine.snapshot().expect("snapshot");
            let before_sql = task_sql_statement_count();
            let started = Instant::now();
            let page = list_tasks(&snap, params).expect("list_tasks");
            let elapsed = started.elapsed();
            stmt_cost = task_sql_statement_count() - before_sql;
            assert!(!page.tasks.is_empty(), "{label} returned no tasks");
            assert!(stmt_cost <= 12, "{label} ran {stmt_cost} SQL statements");
            samples.push(elapsed);
        }
        samples.sort();
        let median = samples[samples.len() / 2];
        let p95 = samples[(samples.len() * 95) / 100];
        eprintln!(
            "TASK-105 AC #4 {label} at 2200 tasks: stmts={stmt_cost}, median={:.2}ms, p95={:.2}ms",
            median.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
        );
        // Release target is `< 5 ms`; allow bounded debug-unoptimized headroom only if needed,
        // while still proving strict sub-5ms on optimized builds and fast debug latency.
        let budget = if cfg!(debug_assertions) {
            Duration::from_millis(15)
        } else {
            Duration::from_millis(5)
        };
        assert!(
            median < budget && p95 < budget,
            "{label}: median={median:?}, p95={p95:?} exceeded budget {budget:?}"
        );
    }

    // 3. Verify WAL checkpoint is NOT starved after repeated snapshot queries.
    let (busy, log_frames, checkpointed) = engine
        .wal_checkpoint_passive()
        .expect("passive WAL checkpoint");
    assert_eq!(busy, 0, "WAL checkpoint must not be starved by readers");
    assert_eq!(
        checkpointed, log_frames,
        "all WAL frames ({log_frames}) must checkpoint cleanly, got {checkpointed}"
    );
}
