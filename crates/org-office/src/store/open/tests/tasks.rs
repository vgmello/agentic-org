//! Engine-independent conformance tests for unified typed tasks (`TASK-49` B4/B5) and first-class
//! dependencies, searchable labels, reference/modified files, and ordering (`TASK-77` B4b).
//! Runs identically on BOTH `EngineChoice::Sqlite` and `EngineChoice::Memory` plus cache-loss rebuild.

use org_proto::tasks::{
    LabelMatchMode, PatchField, TaskDetailsWire, TaskPriority, TaskSortMode, TaskStatus,
    TasksAddDependencyParams, TasksAssignParams, TasksChecklistAddParams,
    TasksChecklistCheckParams, TasksChecklistEditParams, TasksChecklistMoveParams,
    TasksChecklistRemoveParams, TasksCommentParams, TasksCreateParams, TasksLabelsParams,
    TasksListParams, TasksRemoveDependencyParams, TasksTransitionParams, TasksUnwatchParams,
    TasksUpdateParams, TasksWatchParams, WorkType,
};
use org_proto::ErrorCode;
use org_store::Id;

use crate::organization::features::{AddPerson, CreateTeam, DeactivatePerson};
use crate::store::open::task_ops::{
    TaskAddDependencyOp, TaskAssignOp, TaskChecklistAddOp, TaskChecklistCheckOp,
    TaskChecklistEditOp, TaskChecklistMoveOp, TaskChecklistRemoveOp, TaskCommentOp,
    TaskCommentsRead, TaskCreateOp, TaskDependenciesRead, TaskHistoryRead, TaskInspectRead,
    TaskLabelsRead, TaskListRead, TaskRemoveDependencyOp, TaskTransitionOp, TaskUnwatchOp,
    TaskUpdateOp, TaskWatchOp,
};
use crate::store::open::{AddPersonOp, CreateOrganizationOp, CreateTeamOp, DeactivatePersonOp};

use super::*;

fn next_uuid() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(70_000);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let ms = CREATION_AT.0;
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
    bytes[7] = (seq & 0xff) as u8;
    bytes[8] = 0x80;
    bytes[14] = ((seq >> 8) & 0xff) as u8;
    bytes[15] = (seq & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

struct Seed {
    place: Place,
    store: OfficeStore,
    here: org_store::OfficeIdentity,
    owner: Id,
    peer_person: Id,
    inactive_person: Id,
    team_a: Id,
    team_b: Id,
}

fn seed_store(engine: EngineChoice) -> Seed {
    let place = Place::new();
    let store = place.create(engine);
    let here = identity();
    let cmd = creation_command();
    let owner = cmd.creator.employee_id.clone();
    let team_a = cmd.team_id.clone();
    let create = CreateOrganizationOp {
        operation_id: id(1),
        here: here.clone(),
        command: cmd,
        ca: ca_material(&here.organization_id, 1),
    };
    assert_eq!(store.run(create).unwrap().outcome, Outcome::Completed);

    let team_b = id(101);
    assert_eq!(
        store
            .run(CreateTeamOp {
                operation_id: id(2),
                here: here.clone(),
                owner: owner.clone(),
                by: Actor::Employee(owner.clone()),
                command: CreateTeam {
                    id: team_b.clone(),
                    name: "Platform".into(),
                    parent: Some(team_a.clone()),
                },
            })
            .unwrap()
            .outcome,
        Outcome::Completed
    );

    let peer_person = id(201);
    assert_eq!(
        store
            .run(AddPersonOp {
                operation_id: id(3),
                here: here.clone(),
                owner: owner.clone(),
                by: Actor::Employee(owner.clone()),
                command: AddPerson::new(peer_person.clone(), "bob", "Bob", team_b.clone()),
            })
            .unwrap()
            .outcome,
        Outcome::Completed
    );

    let inactive_person = id(202);
    assert_eq!(
        store
            .run(AddPersonOp {
                operation_id: id(4),
                here: here.clone(),
                owner: owner.clone(),
                by: Actor::Employee(owner.clone()),
                command: AddPerson::new(
                    inactive_person.clone(),
                    "former",
                    "Former",
                    team_a.clone(),
                ),
            })
            .unwrap()
            .outcome,
        Outcome::Completed
    );
    assert_eq!(
        store
            .run(DeactivatePersonOp {
                operation_id: id(5),
                here: here.clone(),
                owner: owner.clone(),
                by: Actor::Employee(owner.clone()),
                command: DeactivatePerson {
                    id: inactive_person.clone(),
                    expected_revision: 1,
                },
            })
            .unwrap()
            .outcome,
        Outcome::Completed
    );

    Seed {
        place,
        store,
        here,
        owner,
        peer_person,
        inactive_person,
        team_a,
        team_b,
    }
}

struct BasicTaskSpec<'a> {
    actor: &'a Id,
    team: &'a Id,
    title: &'a str,
    task_type: &'a str,
    rank_order: Option<u64>,
    labels: Vec<&'a str>,
    ref_files: Vec<&'a str>,
    mod_files: Vec<&'a str>,
}

fn create_basic(
    store: &OfficeStore,
    here: &org_store::OfficeIdentity,
    spec: BasicTaskSpec<'_>,
) -> org_proto::tasks::TaskView {
    store
        .run(TaskCreateOp {
            actor: spec.actor.as_str().to_owned(),
            here: here.clone(),
            numbering: store.task_numbering(),
            params: TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(spec.team.as_str().to_owned()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some(spec.task_type.into()),
                work_type: Some(WorkType::Build),
                title: spec.title.into(),
                body_markdown: format!("Details for {}", spec.title),
                priority: Some(TaskPriority::High),
                effort: Some(3),
                severity: (spec.task_type == "bug").then(|| "major".into()),
                rank_order: spec.rank_order,
                labels: spec.labels.into_iter().map(str::to_owned).collect(),
                reference_files: spec.ref_files.into_iter().map(str::to_owned).collect(),
                modified_files: spec.mod_files.into_iter().map(str::to_owned).collect(),
                depends_on: Vec::new(),
                details: (spec.task_type == "bug").then(|| TaskDetailsWire {
                    what_happened: Some("Crash on retry".into()),
                    expected: Some("Idempotent reply".into()),
                    ..Default::default()
                }),
            },
        })
        .unwrap()
        .expect("task created")
        .task
}

fn assert_auth_and_transitions(s: &Seed, t1: &org_proto::tasks::TaskView) {
    // ADR-0031 TASKAUTH-2/3: inactive employee refused NotAuthorized; active peer succeeds.
    let denied = s
        .store
        .run(TaskTransitionOp {
            actor: s.inactive_person.as_str().to_owned(),
            params: TasksTransitionParams {
                command_id: next_uuid(),
                task: t1.key.clone(),
                expected_revision: t1.revision,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: None,
            },
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::NotAuthorized);

    // Independent team/individual assignment (owning team_a stays untouched).
    let assigned = s
        .store
        .run(TaskAssignOp {
            actor: s.peer_person.as_str().to_owned(),
            params: TasksAssignParams {
                command_id: next_uuid(),
                task: t1.key.clone(),
                expected_revision: t1.revision,
                assigned_team_id: PatchField::Set(Some(s.team_b.as_str().to_owned())),
                assignee_id: PatchField::Set(Some(s.peer_person.as_str().to_owned())),
            },
        })
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(assigned.team_id, s.team_a.as_str());
    assert_eq!(
        assigned.assigned_team_id.as_deref(),
        Some(s.team_b.as_str())
    );
    assert_eq!(
        assigned.assignee_id.as_deref(),
        Some(s.peer_person.as_str())
    );
    assert_eq!(assigned.revision, 2);

    // Stale expected_revision refused with Conflict.
    let conflict = s
        .store
        .run(TaskTransitionOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksTransitionParams {
                command_id: next_uuid(),
                task: t1.key.clone(),
                expected_revision: 1,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: None,
            },
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(conflict.code, ErrorCode::Conflict);

    // Blocked requires blocked_reason; then transition to InProgress succeeds.
    let missing_reason = s
        .store
        .run(TaskTransitionOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksTransitionParams {
                command_id: next_uuid(),
                task: t1.key.clone(),
                expected_revision: assigned.revision,
                status: TaskStatus::Blocked,
                blocked_reason: None,
                resolution: None,
                note: None,
            },
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(missing_reason.code, ErrorCode::InvalidRequest);

    let moved = s
        .store
        .run(TaskTransitionOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksTransitionParams {
                command_id: next_uuid(),
                task: t1.key.clone(),
                expected_revision: assigned.revision,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: Some("Starting work".into()),
            },
        })
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(moved.status, TaskStatus::InProgress);
}

fn assert_checklist_lifecycle(s: &Seed, task_key: &str) {
    let step1 = next_uuid();
    let step2 = next_uuid();
    for (step_id, text, required) in [
        (step1.clone(), "Write unit tests", true),
        (step2.clone(), "Run security audit", false),
    ] {
        s.store
            .run(TaskChecklistAddOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistAddParams {
                    command_id: next_uuid(),
                    task: task_key.to_owned(),
                    step_id,
                    text: text.into(),
                    required,
                },
            })
            .unwrap()
            .unwrap();
    }
    s.store
        .run(TaskChecklistMoveOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksChecklistMoveParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                step_id: step2.clone(),
                position: 0,
            },
        })
        .unwrap()
        .unwrap();
    s.store
        .run(TaskChecklistEditOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksChecklistEditParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                step_id: step2.clone(),
                text: Some("Run full security audit".into()),
                required: Some(true),
            },
        })
        .unwrap()
        .unwrap();
    let (checked, _) = s
        .store
        .run(TaskChecklistCheckOp {
            actor: s.peer_person.as_str().to_owned(),
            params: TasksChecklistCheckParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                step_id: step2.clone(),
                done: true,
            },
        })
        .unwrap()
        .unwrap();
    assert_eq!(checked.checklist.len(), 2);
    assert_eq!(checked.checklist[0].id, step2);
    assert!(checked.checklist[0].done);
    assert_eq!(
        checked.checklist[0].checked_by_employee_id.as_deref(),
        Some(s.peer_person.as_str())
    );
    let (after_rm, _) = s
        .store
        .run(TaskChecklistRemoveOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksChecklistRemoveParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                step_id: step1,
            },
        })
        .unwrap()
        .unwrap();
    assert_eq!(after_rm.checklist.len(), 1);
}

fn assert_comments_watchers_and_history(s: &Seed, task_key: &str) {
    let (c1, replayed) = s
        .store
        .run(TaskCommentOp {
            actor: s.peer_person.as_str().to_owned(),
            params: TasksCommentParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                comment_id: next_uuid(),
                body_markdown: "Looks clean so far.".into(),
            },
        })
        .unwrap()
        .unwrap();
    assert!(!replayed);
    assert_eq!(c1.comment.position, 0);
    let (comments, _) = s
        .store
        .read(TaskCommentsRead {
            task: task_key.to_owned(),
        })
        .unwrap();
    assert_eq!(comments.unwrap().comments.len(), 1);

    let watched = s
        .store
        .run(TaskWatchOp {
            actor: s.peer_person.as_str().to_owned(),
            params: TasksWatchParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
            },
        })
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(watched.watcher_count, 1);
    let unwatched = s
        .store
        .run(TaskUnwatchOp {
            actor: s.peer_person.as_str().to_owned(),
            params: TasksUnwatchParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
            },
        })
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(unwatched.watcher_count, 0);

    let (history_res, _) = s
        .store
        .read(TaskHistoryRead {
            task: task_key.to_owned(),
        })
        .unwrap();
    let history = history_res.unwrap();
    assert!(history.events.len() >= 10);
    assert_eq!(history.events[0].revision, 1);
    assert_eq!(history.events[1].change["type"], "assignment_changed");
}

fn assert_sqlite_rebuild_parity(s: Seed) {
    let db_path = s
        .place
        .runtime()
        .join(s.here.office_id.as_str())
        .join("office.db");
    s.store.close().expect("close store");
    let _ = std::fs::remove_file(&db_path);
    let reopened = s.place.reopen(EngineChoice::Sqlite);
    let (inspected_res, _) = reopened
        .read(TaskInspectRead {
            task: "T-A1".into(),
        })
        .unwrap();
    let inspected = inspected_res.unwrap();
    assert_eq!(inspected.task.labels, vec!["backend", "security"]);
    assert_eq!(inspected.task.rank_order, Some(20));
    assert_eq!(inspected.checklist.len(), 1);
    assert_eq!(inspected.comments.len(), 1);
    assert!(inspected.history.len() >= 10);
    let t4 = create_basic(
        &reopened,
        &s.here,
        BasicTaskSpec {
            actor: &s.owner,
            team: &s.team_a,
            title: "Post-rebuild task",
            task_type: "task",
            rank_order: Some(1),
            labels: vec!["backend"],
            ref_files: vec![],
            mod_files: vec![],
        },
    );
    assert_eq!(t4.key, "T-A4", "office_task_counter must survive rebuild");
}

#[test]
fn engine_independent_tasks_lifecycle_auth_checklists_comments_and_rebuild() {
    for engine in ENGINES {
        let s = seed_store(engine);

        let t1 = create_basic(
            &s.store,
            &s.here,
            BasicTaskSpec {
                actor: &s.owner,
                team: &s.team_a,
                title: "Build auth layer",
                task_type: "task",
                rank_order: Some(20),
                labels: vec!["Backend", "Security"],
                ref_files: vec!["docs/auth.md", "./crates/org-proto/src/tasks.rs"],
                mod_files: vec!["crates/org-office/src/tasks/records.rs"],
            },
        );
        let t2 = create_basic(
            &s.store,
            &s.here,
            BasicTaskSpec {
                actor: &s.peer_person,
                team: &s.team_a,
                title: "Fix token leak",
                task_type: "bug",
                rank_order: Some(5),
                labels: vec!["security", "urgent-fix"],
                ref_files: vec!["docs/security.md"],
                mod_files: vec!["crates/org-office-server/src/serve.rs"],
            },
        );
        let t3 = create_basic(
            &s.store,
            &s.here,
            BasicTaskSpec {
                actor: &s.peer_person,
                team: &s.team_b,
                title: "Request dark theme",
                task_type: "request",
                rank_order: None,
                labels: vec!["frontend"],
                ref_files: vec![],
                mod_files: vec![],
            },
        );
        assert_eq!(t1.key, "T-A1");
        assert_eq!(t2.key, "T-A2");
        assert_eq!(t3.key, "T-A3");
        assert_eq!(t1.labels, vec!["backend", "security"]);
        assert_eq!(
            t1.reference_files,
            vec!["crates/org-proto/src/tasks.rs", "docs/auth.md"]
        );
        assert_eq!(
            t1.modified_files,
            vec!["crates/org-office/src/tasks/records.rs"]
        );

        assert_auth_and_transitions(&s, &t1);
        assert_checklist_lifecycle(&s, &t1.key);
        assert_comments_watchers_and_history(&s, &t1.key);

        if engine == EngineChoice::Sqlite {
            assert_sqlite_rebuild_parity(s);
        }
    }
}

fn assert_dependencies_dag(
    s: &Seed,
    a: &org_proto::tasks::TaskView,
    b: &org_proto::tasks::TaskView,
    c: &org_proto::tasks::TaskView,
) {
    for (task, depends_on_task) in [(&b.key, &a.key), (&c.key, &b.key)] {
        s.store
            .run(TaskAddDependencyOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksAddDependencyParams {
                    command_id: next_uuid(),
                    task: task.clone(),
                    depends_on_task: depends_on_task.clone(),
                },
            })
            .unwrap()
            .unwrap();
    }

    let self_err = s
        .store
        .run(TaskAddDependencyOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksAddDependencyParams {
                command_id: next_uuid(),
                task: a.key.clone(),
                depends_on_task: a.key.clone(),
            },
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(self_err.code, ErrorCode::InvalidRequest);

    let cycle_err = s
        .store
        .run(TaskAddDependencyOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksAddDependencyParams {
                command_id: next_uuid(),
                task: a.key.clone(),
                depends_on_task: c.key.clone(),
            },
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(cycle_err.code, ErrorCode::Conflict);

    let (deps_b, _) = s
        .store
        .read(TaskDependenciesRead {
            task: b.key.clone(),
        })
        .unwrap();
    let deps_b = deps_b.unwrap();
    assert_eq!(deps_b.direct_depends_on.len(), 1);
    assert_eq!(deps_b.direct_depends_on[0].key, "T-A1");
    assert_eq!(deps_b.direct_dependents.len(), 1);
    assert_eq!(deps_b.direct_dependents[0].key, "T-A3");

    let (deps_c, _) = s
        .store
        .read(TaskDependenciesRead {
            task: c.key.clone(),
        })
        .unwrap();
    let deps_c = deps_c.unwrap();
    assert_eq!(deps_c.transitive_depends_on.len(), 2);
    assert_eq!(deps_c.edges.len(), 2);
}

fn assert_labels_and_ordering(s: &Seed) {
    let (and_res, _) = s
        .store
        .read(TaskListRead {
            params: TasksListParams {
                labels: vec!["backend".into(), "security".into()],
                label_match: LabelMatchMode::All,
                ..Default::default()
            },
        })
        .unwrap();
    let and_res = and_res.unwrap();
    assert_eq!(and_res.tasks.len(), 1);
    assert_eq!(and_res.tasks[0].key, "T-A1");

    let (or_res, _) = s
        .store
        .read(TaskListRead {
            params: TasksListParams {
                labels: vec!["storage".into(), "api".into()],
                label_match: LabelMatchMode::Any,
                ..Default::default()
            },
        })
        .unwrap();
    assert_eq!(or_res.unwrap().tasks.len(), 2);

    let (catalog, _) = s
        .store
        .read(TaskLabelsRead {
            params: TasksLabelsParams {
                query: Some("sec".into()),
            },
        })
        .unwrap();
    let catalog = catalog.unwrap();
    assert_eq!(catalog.labels.len(), 1);
    assert_eq!(catalog.labels[0].label, "security");
    assert_eq!(catalog.labels[0].task_count, 2);

    let (ordered, _) = s
        .store
        .read(TaskListRead {
            params: TasksListParams {
                sort: TaskSortMode::Order,
                ..Default::default()
            },
        })
        .unwrap();
    let ordered = ordered.unwrap();
    let keys: Vec<&str> = ordered.tasks.iter().map(|t| t.key.as_str()).collect();
    assert_eq!(keys, vec!["T-A2", "T-A1", "T-A3"]);
}

fn assert_files_and_dependency_removal(
    s: &Seed,
    a: &org_proto::tasks::TaskView,
    b: &org_proto::tasks::TaskView,
) {
    let bad_path = s
        .store
        .run(TaskUpdateOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksUpdateParams {
                command_id: next_uuid(),
                task: a.key.clone(),
                expected_revision: a.revision,
                title: None,
                body_markdown: None,
                task_type: None,
                work_type: PatchField::Unchanged,
                priority: None,
                effort: PatchField::Unchanged,
                severity: PatchField::Unchanged,
                rank_order: PatchField::Unchanged,
                labels: None,
                add_labels: vec![],
                remove_labels: vec![],
                reference_files: None,
                add_reference_files: vec!["../etc/passwd".into()],
                remove_reference_files: vec![],
                modified_files: None,
                add_modified_files: vec![],
                remove_modified_files: vec![],
                details: None,
            },
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(bad_path.code, ErrorCode::InvalidRequest);

    let updated = s
        .store
        .run(TaskUpdateOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksUpdateParams {
                command_id: next_uuid(),
                task: a.key.clone(),
                expected_revision: a.revision,
                title: None,
                body_markdown: None,
                task_type: None,
                work_type: PatchField::Unchanged,
                priority: None,
                effort: PatchField::Unchanged,
                severity: PatchField::Unchanged,
                rank_order: PatchField::Set(Some(0)),
                labels: None,
                add_labels: vec!["p0".into()],
                remove_labels: vec![],
                reference_files: None,
                add_reference_files: vec!["docs/adr-0019.md".into()],
                remove_reference_files: vec![],
                modified_files: None,
                add_modified_files: vec!["crates/org-office/src/tasks/mod.rs".into()],
                remove_modified_files: vec![],
                details: None,
            },
        })
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(updated.rank_order, Some(0));
    assert_eq!(updated.labels, vec!["backend", "p0", "security"]);
    assert_eq!(
        updated.reference_files,
        vec!["docs/adr-0019.md", "docs/schema.md"]
    );
    assert_eq!(
        updated.modified_files,
        vec![
            "crates/org-office/src/tasks/mod.rs",
            "crates/org-office/src/tasks/records.rs"
        ]
    );

    s.store
        .run(TaskRemoveDependencyOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksRemoveDependencyParams {
                command_id: next_uuid(),
                task: b.key.clone(),
                depends_on_task: a.key.clone(),
            },
        })
        .unwrap()
        .unwrap();
    let (after_rm, _) = s
        .store
        .read(TaskInspectRead {
            task: a.key.clone(),
        })
        .unwrap();
    assert!(after_rm.unwrap().task.dependents.is_empty());
}

#[test]
fn engine_independent_task_77_dependencies_searchable_labels_files_and_ordering() {
    for engine in ENGINES {
        let s = seed_store(engine);

        let a = create_basic(
            &s.store,
            &s.here,
            BasicTaskSpec {
                actor: &s.owner,
                team: &s.team_a,
                title: "Root schema",
                task_type: "task",
                rank_order: Some(10),
                labels: vec!["backend", "security"],
                ref_files: vec!["docs/schema.md"],
                mod_files: vec!["crates/org-office/src/tasks/records.rs"],
            },
        );
        let b = create_basic(
            &s.store,
            &s.here,
            BasicTaskSpec {
                actor: &s.owner,
                team: &s.team_a,
                title: "Store adapter",
                task_type: "task",
                rank_order: Some(2),
                labels: vec!["backend", "storage"],
                ref_files: vec!["crates/org-store/src/lib.rs"],
                mod_files: vec!["crates/org-office/src/store/sqlite/tasks.rs"],
            },
        );
        let c = create_basic(
            &s.store,
            &s.here,
            BasicTaskSpec {
                actor: &s.owner,
                team: &s.team_a,
                title: "API endpoint",
                task_type: "task",
                rank_order: None,
                labels: vec!["security", "api"],
                ref_files: vec![],
                mod_files: vec![],
            },
        );

        assert_dependencies_dag(&s, &a, &b, &c);
        assert_labels_and_ordering(&s);
        assert_files_and_dependency_removal(&s, &a, &b);
    }
}

fn assert_idempotency_and_noop_guards(s: &Seed) {
    let cmd_id = next_uuid();
    let task_id = next_uuid();
    let params = TasksCreateParams {
        command_id: cmd_id.clone(),
        task_id: task_id.clone(),
        team_id: Some(s.team_a.as_str().to_owned()),
        project_id: None,
        assigned_team_id: None,
        assignee_id: None,
        task_type: Some("task".into()),
        work_type: None,
        title: "Idempotent task".into(),
        body_markdown: String::new(),
        priority: None,
        effort: None,
        severity: None,
        rank_order: Some(1),
        labels: vec!["backend".into()],
        reference_files: vec![],
        modified_files: vec![],
        depends_on: vec![],
        details: None,
    };
    let first = s
        .store
        .run(TaskCreateOp {
            actor: s.owner.as_str().to_owned(),
            here: s.here.clone(),
            numbering: s.store.task_numbering(),
            params: params.clone(),
        })
        .unwrap()
        .unwrap();
    assert!(!first.replayed);

    // Exact replay returns replayed=true without mutating state.
    let replay = s
        .store
        .run(TaskCreateOp {
            actor: s.owner.as_str().to_owned(),
            here: s.here.clone(),
            numbering: s.store.task_numbering(),
            params: params.clone(),
        })
        .unwrap()
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.task.id, first.task.id);

    // Reusing command_id with different params returns OperationReused (not StorageError).
    let mut mutated = params;
    mutated.title = "Different title".into();
    let reused = s
        .store
        .run(TaskCreateOp {
            actor: s.owner.as_str().to_owned(),
            here: s.here.clone(),
            numbering: s.store.task_numbering(),
            params: mutated,
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(reused.code, ErrorCode::OperationReused);

    // No-op watch / unwatch do not bump revision.
    let w1 = s
        .store
        .run(TaskWatchOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksWatchParams {
                command_id: next_uuid(),
                task: first.task.key.clone(),
            },
        })
        .unwrap()
        .unwrap()
        .task;
    let w2 = s
        .store
        .run(TaskWatchOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksWatchParams {
                command_id: next_uuid(),
                task: first.task.key.clone(),
            },
        })
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(w1.revision, w2.revision);
}

fn assert_validation_bounds_and_cap_swap(s: &Seed) {
    // u64 > i64::MAX refused consistently on Memory and SQLite.
    let overflow = s
        .store
        .run(TaskCreateOp {
            actor: s.owner.as_str().to_owned(),
            here: s.here.clone(),
            numbering: s.store.task_numbering(),
            params: TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(s.team_a.as_str().to_owned()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Bad order".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(u64::MAX),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
            },
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(overflow.code, ErrorCode::InvalidRequest);

    // Unvalidated duplicate_of_task_id and affects_feature_id refused consistently.
    let bad_dup = s
        .store
        .run(TaskCreateOp {
            actor: s.owner.as_str().to_owned(),
            here: s.here.clone(),
            numbering: s.store.task_numbering(),
            params: TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(s.team_a.as_str().to_owned()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("bug".into()),
                work_type: None,
                title: "Missing dup".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: Some(TaskDetailsWire {
                    duplicate_of_task_id: Some(next_uuid()),
                    ..Default::default()
                }),
            },
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(bad_dup.code, ErrorCode::NotFound);

    let full_labels: Vec<String> = (0..64).map(|i| format!("lbl-{i:02}")).collect();
    let capped = s
        .store
        .run(TaskCreateOp {
            actor: s.owner.as_str().to_owned(),
            here: s.here.clone(),
            numbering: s.store.task_numbering(),
            params: TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(s.team_a.as_str().to_owned()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Cap boundary".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: full_labels,
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
            },
        })
        .unwrap()
        .unwrap()
        .task;
    let swapped = s
        .store
        .run(TaskUpdateOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksUpdateParams {
                command_id: next_uuid(),
                task: capped.key.clone(),
                expected_revision: capped.revision,
                title: None,
                body_markdown: None,
                task_type: None,
                work_type: PatchField::Unchanged,
                priority: None,
                effort: PatchField::Unchanged,
                severity: PatchField::Unchanged,
                rank_order: PatchField::Unchanged,
                labels: None,
                add_labels: vec!["lbl-new".into()],
                remove_labels: vec!["lbl-00".into()],
                reference_files: None,
                add_reference_files: vec![],
                remove_reference_files: vec![],
                modified_files: None,
                add_modified_files: vec![],
                remove_modified_files: vec![],
                details: None,
            },
        })
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(swapped.labels.len(), 64);
    assert!(swapped.labels.contains(&"lbl-new".to_owned()));

    // Task key number > i64::MAX resolves to NotFound consistently on Memory and SQLite.
    let (huge_key, _) = s
        .store
        .read(TaskInspectRead {
            task: "T-A9999999999999999999".into(),
        })
        .unwrap();
    assert_eq!(huge_key.unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn engine_independent_idempotency_replay_reuse_and_validation_bounds() {
    for engine in ENGINES {
        let s = seed_store(engine);
        assert_idempotency_and_noop_guards(&s);
        assert_validation_bounds_and_cap_swap(&s);
    }
}
