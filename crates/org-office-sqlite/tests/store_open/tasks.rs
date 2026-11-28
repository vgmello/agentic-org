//! Engine-independent conformance tests for unified typed tasks (`TASK-49` B4/B5) and first-class
//! dependencies, searchable labels, reference/modified files, and ordering (`TASK-77` B4b).
//! Runs identically on BOTH `EngineChoice::Sqlite` and `EngineChoice::Memory` plus cache-loss rebuild.

use std::collections::{BTreeMap, BTreeSet};

use org_proto::attachments::{ArtifactStatus, AttachmentTargetKind};
use org_proto::tasks::{
    LabelMatchMode, PatchField, TaskDetailsWire, TaskPriority, TaskSortMode, TaskStatus,
    TasksAddDependencyParams, TasksAssignParams, TasksChecklistAddParams,
    TasksChecklistCheckParams, TasksChecklistEditParams, TasksChecklistMoveParams,
    TasksChecklistRemoveParams, TasksCommentParams, TasksCreateParams, TasksFinalSummaryParams,
    TasksHistoryParams, TasksLabelsParams, TasksListParams, TasksRemoveDependencyParams,
    TasksTransitionParams, TasksUnwatchParams, TasksUpdateParams, TasksWatchParams, WorkType,
};
use org_proto::ErrorCode;
use org_store::{Id, Operation, Sha256};

use org_office::attachments::blobs::ContentHash;
use org_office::attachments::records::{AttachmentRecord, MetaPatch};
use org_office::organization_family::features::offices::RegisterSatelliteOffice;
use org_office::organization_family::features::{AddPerson, CreateTeam, DeactivatePerson};
use org_office::organization_family::records::OfficeRole;
use org_office::replication::state_files::ApplyReplicatedFiles;
use org_office::tasks::ops::TaskWriteAuthority;
use org_office::tasks::records::{
    self as task_records, EventInput, Location, TaskMeta, TaskRecord,
};
use org_office::testkit_support::store_open::task_ops::{
    TaskAddDependencyOp, TaskAssignOp, TaskAttachOp, TaskChecklistAddOp, TaskChecklistCheckOp,
    TaskChecklistEditOp, TaskChecklistMoveOp, TaskChecklistRemoveOp, TaskCommentOp,
    TaskCommentsRead, TaskCreateOp, TaskDependenciesRead, TaskFinalSummaryOp, TaskHistoryRead,
    TaskInspectRead, TaskLabelsRead, TaskListRead, TaskRemoveDependencyOp, TaskTransitionOp,
    TaskUnwatchOp, TaskUpdateAttachmentMetaOp, TaskUpdateOp, TaskWatchOp,
};
use org_office::testkit_support::store_open::{
    AddPersonOp, ApplyReplicatedFilesOp, CreateOrganizationOp, CreateTeamOp, DeactivatePersonOp,
    RecordTaskRepoBranchOp, RegisterSatelliteOfficeOp,
};

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
    let store = place.create(engine.clone());
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
                dod: None,
                attachment_ids: Vec::new(),
            },
            prepared_files: Default::default(),
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
                reason: None,
                blocked_reason: None,
                waiting_on_owner: None,
                resolution: None,
                note: None,
                done_anyway_reason: None,
                final_summary: None,
            },
            authority: TaskWriteAuthority::Unrestricted,
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
            authority: TaskWriteAuthority::Unrestricted,
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
                reason: None,
                blocked_reason: None,
                waiting_on_owner: None,
                resolution: None,
                note: None,
                done_anyway_reason: None,
                final_summary: None,
            },
            authority: TaskWriteAuthority::Unrestricted,
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
                reason: None,
                blocked_reason: None,
                waiting_on_owner: None,
                resolution: None,
                note: None,
                done_anyway_reason: None,
                final_summary: None,
            },
            authority: TaskWriteAuthority::Unrestricted,
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
                reason: None,
                blocked_reason: None,
                waiting_on_owner: None,
                resolution: None,
                note: Some("Starting work".into()),
                done_anyway_reason: None,
                final_summary: None,
            },
            authority: TaskWriteAuthority::Unrestricted,
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
                    required: Some(required),
                    section: None,
                    steps: vec![],
                },
                authority: TaskWriteAuthority::Unrestricted,
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
                from_section: None,
                index: None,
                expected_revision: None,
                position: 0,
                section: None,
            },
            authority: TaskWriteAuthority::Unrestricted,
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
                section: None,
                index: None,
                expected_revision: None,
                text: Some("Run full security audit".into()),
                required: Some(true),
            },
            authority: TaskWriteAuthority::Unrestricted,
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
                section: None,
                indices: vec![],
                expected_revision: None,
                done: true,
            },
            authority: TaskWriteAuthority::Unrestricted,
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
                section: None,
                index: None,
                expected_revision: None,
            },
            authority: TaskWriteAuthority::Unrestricted,
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
                attachment_ids: vec![],
            },
            prepared_files: Default::default(),
            authority: TaskWriteAuthority::Unrestricted,
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
            authority: TaskWriteAuthority::Unrestricted,
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
            authority: TaskWriteAuthority::Unrestricted,
        })
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(unwatched.watcher_count, 0);

    let (history_res, _) = s
        .store
        .read(TaskHistoryRead {
            params: TasksHistoryParams::new(task_key),
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
    let reopened = s
        .place
        .reopen(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
    let (inspected_res, _) = reopened
        .read(TaskInspectRead {
            task: "T-A1".into(),
            fields: None,
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
    for engine in engines() {
        let s = seed_store(engine.clone());

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

        if engine.is_sqlite() {
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
                authority: TaskWriteAuthority::Unrestricted,
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
            authority: TaskWriteAuthority::Unrestricted,
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
            authority: TaskWriteAuthority::Unrestricted,
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
                append_body: None,
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
                add_attachments: vec![],
                remove_attachments: vec![],
            },
            prepared_files: Default::default(),
            authority: TaskWriteAuthority::Unrestricted,
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
                append_body: None,
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
                add_attachments: vec![],
                remove_attachments: vec![],
            },
            prepared_files: Default::default(),
            authority: TaskWriteAuthority::Unrestricted,
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
            authority: TaskWriteAuthority::Unrestricted,
        })
        .unwrap()
        .unwrap();
    let (after_rm, _) = s
        .store
        .read(TaskInspectRead {
            task: a.key.clone(),
            fields: None,
        })
        .unwrap();
    assert!(after_rm.unwrap().task.dependents.is_empty());
}

#[test]
fn engine_independent_task_77_dependencies_searchable_labels_files_and_ordering() {
    for engine in engines() {
        let s = seed_store(engine.clone());

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
        dod: None,
        attachment_ids: vec![],
    };
    let first = s
        .store
        .run(TaskCreateOp {
            actor: s.owner.as_str().to_owned(),
            here: s.here.clone(),
            numbering: s.store.task_numbering(),
            params: params.clone(),
            prepared_files: Default::default(),
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
            prepared_files: Default::default(),
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
            prepared_files: Default::default(),
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
            authority: TaskWriteAuthority::Unrestricted,
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
            authority: TaskWriteAuthority::Unrestricted,
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
                dod: None,
                attachment_ids: vec![],
            },
            prepared_files: Default::default(),
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
                dod: None,
                attachment_ids: vec![],
            },
            prepared_files: Default::default(),
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
                dod: None,
                attachment_ids: vec![],
            },
            prepared_files: Default::default(),
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
                append_body: None,
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
                add_attachments: vec![],
                remove_attachments: vec![],
            },
            prepared_files: Default::default(),
            authority: TaskWriteAuthority::Unrestricted,
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
            fields: None,
        })
        .unwrap();
    assert_eq!(huge_key.unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn engine_independent_idempotency_replay_reuse_and_validation_bounds() {
    for engine in engines() {
        let s = seed_store(engine.clone());
        assert_idempotency_and_noop_guards(&s);
        assert_validation_bounds_and_cap_swap(&s);
    }
}

fn attach_op(owner: &Id, task_id: &Id, authority: TaskWriteAuthority) -> (Id, TaskAttachOp) {
    let cmd = next_uuid();
    let attachment_id = Id::parse(&next_uuid()).unwrap();
    let op = Operation {
        id: Id::parse(&cmd).unwrap(),
        kind: "attachment.attach",
        actor: Actor::Employee(owner.clone()),
        fingerprint: Sha256::of(cmd.as_bytes()),
    };
    let record = AttachmentRecord {
        id: attachment_id.clone(),
        target_kind: AttachmentTargetKind::Task,
        target_id: Some(task_id.clone()),
        content_sha256: ContentHash::of(b"spec"),
        mime_type: "text/plain".into(),
        byte_count: 4,
        filename: "spec.txt".into(),
        title: "Spec".into(),
        description: None,
        artifact_status: ArtifactStatus::default(),
        width: None,
        height: None,
        page_count: None,
        uploaded_by_employee_id: owner.clone(),
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    };
    (
        attachment_id,
        TaskAttachOp {
            operation: op,
            actor: owner.as_str().to_owned(),
            command_id: cmd,
            record,
            authority,
        },
    )
}

fn update_title(
    s: &Seed,
    task_key: &str,
    expected_revision: u64,
    title: &str,
    authority: TaskWriteAuthority,
) -> Result<org_proto::tasks::TaskView, ErrorCode> {
    let params: TasksUpdateParams = serde_json::from_value(serde_json::json!({
        "command_id": next_uuid(),
        "task": task_key,
        "expected_revision": expected_revision,
        "title": title,
    }))
    .unwrap();
    s.store
        .run(TaskUpdateOp {
            actor: s.owner.as_str().to_owned(),
            params,
            prepared_files: Default::default(),
            authority,
        })
        .unwrap()
        .map(|o| o.task)
        .map_err(|e| e.code)
}

fn register_satellite(s: &Seed, op_num: u64, office_id: &Id, name: &str) {
    let reply = s
        .store
        .run(RegisterSatelliteOfficeOp {
            operation_id: id(op_num),
            here: s.here.clone(),
            owner: s.owner.clone(),
            by: Actor::Employee(s.owner.clone()),
            command: RegisterSatelliteOffice {
                office_id: office_id.clone(),
                office_name: name.into(),
            },
        })
        .unwrap();
    assert_eq!(reply.outcome, Outcome::Completed);
}

#[test]
fn atomic_task_write_authority_enforced_inside_unit_of_work() {
    for engine in engines() {
        let s = seed_store(engine.clone());
        let hq_task = create_basic(
            &s.store,
            &s.here,
            BasicTaskSpec {
                actor: &s.owner,
                team: &s.team_a,
                title: "HQ owned task",
                task_type: "task",
                rank_order: None,
                labels: vec![],
                ref_files: vec![],
                mod_files: vec![],
            },
        );
        let hq_task_id = Id::parse(&hq_task.id).unwrap();

        // Construct HQ's local authority BEFORE any satellite exists (single-office state).
        // A TOCTOU pre-check would have authorized writes once and missed later enrollments;
        // UnitOfWork evaluates against the live transaction view.
        let hq_auth = TaskWriteAuthority::Local {
            office_id: s.here.office_id.clone(),
            role: OfficeRole::Hq,
            removed_offices: BTreeSet::new(),
        };

        // HQ attaches a file to its own task so we can also test TaskUpdateAttachmentMetaOp.
        let (hq_att_id, hq_attach) = attach_op(&s.owner, &hq_task_id, hq_auth.clone());
        s.store
            .run(hq_attach)
            .unwrap()
            .expect("hq attaches own task");
        let rev_after_attach = s
            .store
            .read(TaskInspectRead {
                task: hq_task.key.clone(),
                fields: None,
            })
            .unwrap()
            .0
            .unwrap()
            .task
            .revision;

        // Enroll satellite office B and replicate a satellite-owned task T-B1 into HQ's store.
        let sat_office = id(901);
        register_satellite(&s, 90, &sat_office, "Munich Satellite");
        let sat_task_id = Id::parse(&next_uuid()).unwrap();
        let sat_record = TaskRecord {
            id: sat_task_id.clone(),
            office_code: "B".into(),
            number: 1,
            team_id: s.team_a.clone(),
            assigned_team_id: None,
            project_id: None,
            task_type: "task".into(),
            work_type: None,
            title: "Satellite owned task".into(),
            status: TaskStatus::Todo,
            assignee_id: None,
            body_markdown: String::new(),
            feature_id: None,
            milestone_id: None,
            priority: TaskPriority::High,
            effort: None,
            severity: None,
            resolution: None,
            blocked_reason: None,
            blocked_since: None,
            waiting_on_owner: false,
            rank_order: None,
            labels: Vec::new(),
            reference_files: Vec::new(),
            modified_files: Vec::new(),
            repositories: Vec::new(),
            details: None,
            final_summary: None,
            meta: TaskMeta::created(&s.owner, "2026-01-01T00:00:00.000Z"),
        };
        let sat_loc = Location::Team(vec![s.team_a.clone()]);
        let sat_md_path = task_records::task_md_path(&sat_loc, &sat_task_id).unwrap();
        let sat_md_bytes = task_records::encode_task_md(&sat_record, &[], &[]).unwrap();
        let sat_event_path = task_records::event_path(&sat_task_id, 1);
        let (_, sat_event_bytes) = task_records::encode_event(EventInput {
            organization_id: &s.here.organization_id,
            task_id: &sat_task_id,
            revision: 1,
            occurred_at: "2026-01-01T00:00:00.000Z",
            actor_id: &s.owner,
            operation_id: &next_uuid(),
            change: task_records::created_change(serde_json::Map::new(), ""),
        })
        .unwrap();
        let replicated = s
            .store
            .run(ApplyReplicatedFilesOp {
                operation_id: id(91),
                actor: Actor::System,
                command: ApplyReplicatedFiles {
                    files: vec![
                        (sat_md_path.as_str().to_owned(), Some(sat_md_bytes)),
                        (sat_event_path.as_str().to_owned(), Some(sat_event_bytes)),
                    ],
                    rehome_tasks: false,
                    task_revisions: BTreeMap::from([(sat_task_id.clone(), 1)]),
                },
            })
            .unwrap();
        assert_eq!(replicated.outcome, Outcome::Completed);
        let sat_task = s
            .store
            .read(TaskInspectRead {
                task: sat_task_id.as_str().to_owned(),
                fields: None,
            })
            .unwrap()
            .0
            .expect("satellite task replicated")
            .task;
        assert_eq!(sat_task.office_code, "B");
        assert_eq!(sat_task.key, "T-B1");

        let sat_auth = TaskWriteAuthority::Local {
            office_id: sat_office.clone(),
            role: OfficeRole::Satellite,
            removed_offices: BTreeSet::new(),
        };

        // Satellite is refused on HQ's task across update, transition, comment, attach, meta, git.
        assert_eq!(
            update_title(
                &s,
                &hq_task.key,
                rev_after_attach,
                "Hijack",
                sat_auth.clone()
            ),
            Err(ErrorCode::NotAuthorized)
        );
        let sat_trans = s
            .store
            .run(TaskTransitionOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksTransitionParams {
                    command_id: next_uuid(),
                    task: hq_task.key.clone(),
                    expected_revision: rev_after_attach,
                    status: TaskStatus::InProgress,
                    reason: None,
                    blocked_reason: None,
                    waiting_on_owner: None,
                    resolution: None,
                    note: None,
                    done_anyway_reason: None,
                    final_summary: None,
                },
                authority: sat_auth.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(sat_trans.code, ErrorCode::NotAuthorized);

        let sat_comment = s
            .store
            .run(TaskCommentOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksCommentParams {
                    command_id: next_uuid(),
                    task: hq_task.key.clone(),
                    comment_id: next_uuid(),
                    body_markdown: "denied".into(),
                    attachment_ids: vec![],
                },
                prepared_files: Default::default(),
                authority: sat_auth.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(sat_comment.code, ErrorCode::NotAuthorized);

        let (_, sat_attach) = attach_op(&s.owner, &hq_task_id, sat_auth.clone());
        assert_eq!(
            s.store.run(sat_attach).unwrap().unwrap_err().code,
            ErrorCode::NotAuthorized
        );

        let meta_cmd = next_uuid();
        let sat_meta = s
            .store
            .run(TaskUpdateAttachmentMetaOp {
                operation: Operation {
                    id: Id::parse(&meta_cmd).unwrap(),
                    kind: "attachment.update_meta",
                    actor: Actor::Employee(s.owner.clone()),
                    fingerprint: Sha256::of(meta_cmd.as_bytes()),
                },
                actor: s.owner.as_str().to_owned(),
                command_id: meta_cmd,
                attachment_id: hq_att_id.clone(),
                patch: MetaPatch {
                    title: Some("Hijacked".into()),
                    description: None,
                    artifact_status: None,
                    updated_at: "2026-01-01T00:01:00.000Z".into(),
                },
                authority: sat_auth.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(sat_meta.code, ErrorCode::NotAuthorized);

        let sat_git = s
            .store
            .run(RecordTaskRepoBranchOp {
                actor: s.owner.as_str().to_owned(),
                command_id: next_uuid(),
                task_id: hq_task_id.clone(),
                repository_id: id(801),
                branch: "task/T-A1".into(),
                integrated_commit: None,
                authority: sat_auth.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(sat_git.code, ErrorCode::NotAuthorized);

        assert_remaining_task_ops_refused(&s, &hq_task.key, rev_after_attach, &sat_auth);

        // HQ using `hq_auth` captured BEFORE satellite enrollment is STILL refused on T-B1
        // because UnitOfWork checks the live `offices` + `office_codes` tables atomically.
        assert_eq!(
            update_title(
                &s,
                &sat_task.key,
                sat_task.revision,
                "HQ hijack",
                hq_auth.clone()
            ),
            Err(ErrorCode::NotAuthorized)
        );
        let (_, hq_attach_sat) = attach_op(&s.owner, &sat_task_id, hq_auth.clone());
        assert_eq!(
            s.store.run(hq_attach_sat).unwrap().unwrap_err().code,
            ErrorCode::NotAuthorized
        );
        let hq_git_sat = s
            .store
            .run(RecordTaskRepoBranchOp {
                actor: s.owner.as_str().to_owned(),
                command_id: next_uuid(),
                task_id: sat_task_id.clone(),
                repository_id: id(801),
                branch: "task/T-B1".into(),
                integrated_commit: None,
                authority: hq_auth.clone(),
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(hq_git_sat.code, ErrorCode::NotAuthorized);

        // Satellite CAN mutate its own task T-B1; HQ CAN mutate T-B1 once satellite is revoked.
        let sat_updated =
            update_title(&s, &sat_task.key, sat_task.revision, "Sat ok", sat_auth).expect("sat ok");
        assert_eq!(sat_updated.revision, sat_task.revision + 1);

        let hq_recovery_auth = TaskWriteAuthority::Local {
            office_id: s.here.office_id.clone(),
            role: OfficeRole::Hq,
            removed_offices: BTreeSet::from([sat_office.as_str().to_owned()]),
        };
        let recovered = update_title(
            &s,
            &sat_task.key,
            sat_updated.revision,
            "HQ recovered",
            hq_recovery_auth,
        )
        .expect("hq recovery ok");
        assert_eq!(recovered.title, "HQ recovered");

        // HQ task revision remained untouched across all refused satellite attempts.
        let hq_after = s
            .store
            .read(TaskInspectRead {
                task: hq_task.key.clone(),
                fields: None,
            })
            .unwrap()
            .0
            .unwrap()
            .task;
        assert_eq!(hq_after.revision, rev_after_attach);
    }
}

fn assert_remaining_task_ops_refused(
    s: &Seed,
    task_key: &str,
    expected_revision: u64,
    authority: &TaskWriteAuthority,
) {
    let actor = s.owner.as_str().to_owned();
    let assign = s.store.run(TaskAssignOp {
        actor: actor.clone(),
        params: TasksAssignParams {
            command_id: next_uuid(),
            task: task_key.into(),
            expected_revision,
            assigned_team_id: PatchField::Unchanged,
            assignee_id: PatchField::Set(Some(actor.clone())),
        },
        authority: authority.clone(),
    });
    assert_eq!(assign.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let summary = s.store.run(TaskFinalSummaryOp {
        actor: actor.clone(),
        params: TasksFinalSummaryParams {
            command_id: next_uuid(),
            task: task_key.into(),
            expected_revision,
            final_summary: "denied".into(),
        },
        authority: authority.clone(),
    });
    assert_eq!(summary.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let watch = s.store.run(TaskWatchOp {
        actor: actor.clone(),
        params: TasksWatchParams {
            command_id: next_uuid(),
            task: task_key.into(),
        },
        authority: authority.clone(),
    });
    assert_eq!(watch.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let unwatch = s.store.run(TaskUnwatchOp {
        actor: actor.clone(),
        params: TasksUnwatchParams {
            command_id: next_uuid(),
            task: task_key.into(),
        },
        authority: authority.clone(),
    });
    assert_eq!(unwatch.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let dep_add = s.store.run(TaskAddDependencyOp {
        actor: actor.clone(),
        params: TasksAddDependencyParams {
            command_id: next_uuid(),
            task: task_key.into(),
            depends_on_task: task_key.into(),
        },
        authority: authority.clone(),
    });
    assert_eq!(dep_add.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let dep_rm = s.store.run(TaskRemoveDependencyOp {
        actor,
        params: TasksRemoveDependencyParams {
            command_id: next_uuid(),
            task: task_key.into(),
            depends_on_task: task_key.into(),
        },
        authority: authority.clone(),
    });
    assert_eq!(dep_rm.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    assert_checklist_ops_refused(s, task_key, authority);
}

fn assert_checklist_ops_refused(s: &Seed, task_key: &str, authority: &TaskWriteAuthority) {
    let actor = s.owner.as_str().to_owned();
    let add = s.store.run(TaskChecklistAddOp {
        actor: actor.clone(),
        params: TasksChecklistAddParams {
            command_id: next_uuid(),
            task: task_key.into(),
            step_id: next_uuid(),
            text: "denied".into(),
            required: None,
            section: None,
            steps: vec![],
        },
        authority: authority.clone(),
    });
    assert_eq!(add.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let check = s.store.run(TaskChecklistCheckOp {
        actor: actor.clone(),
        params: TasksChecklistCheckParams {
            command_id: next_uuid(),
            task: task_key.into(),
            step_id: next_uuid(),
            section: None,
            indices: vec![],
            expected_revision: None,
            done: true,
        },
        authority: authority.clone(),
    });
    assert_eq!(check.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let edit = s.store.run(TaskChecklistEditOp {
        actor: actor.clone(),
        params: TasksChecklistEditParams {
            command_id: next_uuid(),
            task: task_key.into(),
            step_id: next_uuid(),
            section: None,
            index: None,
            expected_revision: None,
            text: Some("denied".into()),
            required: None,
        },
        authority: authority.clone(),
    });
    assert_eq!(edit.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let remove = s.store.run(TaskChecklistRemoveOp {
        actor: actor.clone(),
        params: TasksChecklistRemoveParams {
            command_id: next_uuid(),
            task: task_key.into(),
            step_id: next_uuid(),
            section: None,
            index: None,
            expected_revision: None,
        },
        authority: authority.clone(),
    });
    assert_eq!(remove.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);

    let mv = s.store.run(TaskChecklistMoveOp {
        actor,
        params: TasksChecklistMoveParams {
            command_id: next_uuid(),
            task: task_key.into(),
            step_id: next_uuid(),
            from_section: None,
            index: None,
            expected_revision: None,
            position: 0,
            section: None,
        },
        authority: authority.clone(),
    });
    assert_eq!(mv.unwrap().unwrap_err().code, ErrorCode::NotAuthorized);
}
