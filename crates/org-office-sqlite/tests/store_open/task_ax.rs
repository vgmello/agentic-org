//! `TASK-90` engine-independent conformance tests: server-side `--fields` pruning, surgical
//! checklist batch/1-based ordinal ops with mandatory `--if-match`, `append_body` vs full
//! `body_markdown` mutual exclusion, bounded hybrid `ConflictDetail` (`events_since <= 20` /
//! `<= 8 KiB`) surviving cache-loss rebuilds, and strict ADR-0031 security ordering.
//! Runs identically on BOTH `EngineChoice::Sqlite` and `EngineChoice::Memory`.

use std::collections::BTreeSet;

use org_proto::tasks::{
    ChecklistAddItem, ChecklistSection, PatchField, TaskPriority, TaskStatus,
    TasksChecklistAddParams, TasksChecklistCheckParams, TasksChecklistEditParams,
    TasksChecklistMoveParams, TasksChecklistRemoveParams, TasksCommentParams, TasksCreateParams,
    TasksTransitionParams, TasksUpdateParams, TasksWatchParams, WorkType, TASK_BODY_MAX_CHARS,
};
use org_proto::{ErrorCode, CONFLICT_MAX_EVENTS, CONFLICT_MAX_EVENTS_BYTES};
use org_store::Id;

use org_office::organization_family::features::{AddPerson, DeactivatePerson};
use org_office::organization_family::records::OfficeRole;
use org_office::tasks::ops::TaskWriteAuthority;
use org_office::testkit_support::store_open::task_ops::{
    TaskAddDependencyOp, TaskAssignOp, TaskChecklistAddOp, TaskChecklistCheckOp,
    TaskChecklistEditOp, TaskChecklistMoveOp, TaskChecklistRemoveOp, TaskCommentOp, TaskCreateOp,
    TaskInspectRead, TaskTransitionOp, TaskUpdateOp, TaskWatchOp,
};
use org_office::testkit_support::store_open::{
    AddPersonOp, CreateOrganizationOp, DeactivatePersonOp,
};

use super::*;

fn next_uuid() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(90_000);
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

struct AxSeed {
    place: Place,
    store: OfficeStore,
    here: org_store::OfficeIdentity,
    owner: Id,
    inactive: Id,
    team: Id,
}

fn seed(engine: EngineChoice) -> AxSeed {
    let place = Place::new();
    let store = place.create(engine.clone());
    let here = identity();
    let cmd = creation_command();
    let owner = cmd.creator.employee_id.clone();
    let team = cmd.team_id.clone();
    let create = CreateOrganizationOp {
        operation_id: id(1),
        here: here.clone(),
        command: cmd,
        ca: ca_material(&here.organization_id, 1),
    };
    assert_eq!(store.run(create).unwrap().outcome, Outcome::Completed);

    let inactive = id(202);
    assert_eq!(
        store
            .run(AddPersonOp {
                operation_id: id(2),
                here: here.clone(),
                owner: owner.clone(),
                by: Actor::Employee(owner.clone()),
                command: AddPerson::new(inactive.clone(), "former", "Former", team.clone()),
            })
            .unwrap()
            .outcome,
        Outcome::Completed
    );
    assert_eq!(
        store
            .run(DeactivatePersonOp {
                operation_id: id(3),
                here: here.clone(),
                owner: owner.clone(),
                by: Actor::Employee(owner.clone()),
                command: DeactivatePerson {
                    id: inactive.clone(),
                    expected_revision: 1,
                },
            })
            .unwrap()
            .outcome,
        Outcome::Completed
    );
    AxSeed {
        place,
        store,
        here,
        owner,
        inactive,
        team,
    }
}

fn create_task(s: &AxSeed, title: &str, body: &str) -> org_proto::tasks::TaskView {
    s.store
        .run(TaskCreateOp {
            actor: s.owner.as_str().to_owned(),
            here: s.here.clone(),
            params: TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(s.team.as_str().to_owned()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: Some(WorkType::Build),
                title: title.into(),
                body_markdown: body.into(),
                priority: Some(TaskPriority::Normal),
                effort: Some(3),
                severity: None,
                rank_order: Some(10),
                labels: vec!["ax".into()],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                attachment_ids: vec![],
                details: None,
                dod: None,
            },
            prepared_files: Default::default(),
            numbering: s.store.task_numbering(),
        })
        .unwrap()
        .unwrap()
        .task
}

fn base_update(task: &str, expected_revision: u64) -> TasksUpdateParams {
    TasksUpdateParams {
        command_id: next_uuid(),
        task: task.to_owned(),
        expected_revision,
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
        add_reference_files: vec![],
        remove_reference_files: vec![],
        modified_files: None,
        add_modified_files: vec![],
        remove_modified_files: vec![],
        details: None,
        feature_id: PatchField::Unchanged,
        milestone_id: PatchField::Unchanged,
        add_attachments: vec![],
        remove_attachments: vec![],
    }
}

#[test]
fn task_90_server_fields_pruning_and_append_body_parity() {
    for engine in engines() {
        let s = seed(engine.clone());
        let dep = create_task(&s, "Dependency", "dep body");
        let task = create_task(&s, "Main task", "SECRET_LARGE_BODY_MARKER");
        s.store
            .run(TaskAddDependencyOp {
                actor: s.owner.as_str().to_owned(),
                params: org_proto::tasks::TasksAddDependencyParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    depends_on_task: dep.key.clone(),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        s.store
            .run(TaskChecklistAddOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistAddParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: next_uuid(),
                    text: "Step one".into(),
                    required: None,
                    section: Some(ChecklistSection::Acceptance),
                    steps: vec![],
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        s.store
            .run(TaskCommentOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksCommentParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    comment_id: next_uuid(),
                    body_markdown: "Public comment text".into(),
                    attachment_ids: vec![],
                },
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        s.store
            .run(TaskWatchOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksWatchParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();

        // Pruned inspect: requests metadata + checklist only; skips body, comments, watchers, deps, history.
        let pruned = s
            .store
            .read(TaskInspectRead {
                task: task.key.clone(),
                fields: Some(vec![
                    "key".into(),
                    "title".into(),
                    "status".into(),
                    "revision".into(),
                    "checklist".into(),
                ]),
            })
            .unwrap()
            .0
            .unwrap();
        assert_eq!(pruned.task.key, task.key, "{engine:?}");
        assert!(pruned.task.body_markdown.is_empty(), "{engine:?}");
        assert_eq!(pruned.checklist.len(), 1, "{engine:?}");
        assert!(pruned.comments.is_empty(), "{engine:?}");
        assert!(pruned.watchers.is_empty(), "{engine:?}");
        assert!(pruned.depends_on_tasks.is_empty(), "{engine:?}");
        assert!(pruned.dependent_tasks.is_empty(), "{engine:?}");
        assert!(pruned.history.is_empty(), "{engine:?}");

        // Requesting body_markdown includes body while leaving other child collections empty.
        let body_only = s
            .store
            .read(TaskInspectRead {
                task: task.key.clone(),
                fields: Some(vec!["body_markdown".into()]),
            })
            .unwrap()
            .0
            .unwrap();
        assert_eq!(body_only.task.body_markdown, "SECRET_LARGE_BODY_MARKER");
        assert!(body_only.checklist.is_empty());
        assert!(body_only.history.is_empty());

        // Full inspect (`fields: None`) returns everything.
        let full = s
            .store
            .read(TaskInspectRead {
                task: task.key.clone(),
                fields: None,
            })
            .unwrap()
            .0
            .unwrap();
        assert_eq!(full.task.body_markdown, "SECRET_LARGE_BODY_MARKER");
        assert_eq!(full.checklist.len(), 1);
        assert_eq!(full.comments.len(), 1);
        assert_eq!(full.watchers.len(), 1);
        assert_eq!(full.depends_on_tasks.len(), 1);
        assert!(!full.history.is_empty());

        // `append_body` vs `body_markdown` mutual exclusion + bounds.
        let mut both = base_update(&task.key, full.task.revision);
        both.body_markdown = Some("replace".into());
        both.append_body = Some("append".into());
        let err = s
            .store
            .run(TaskUpdateOp {
                actor: s.owner.as_str().to_owned(),
                params: both,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "{engine:?}");

        let mut empty_append = base_update(&task.key, full.task.revision);
        empty_append.append_body = Some("   \n".into());
        let err = s
            .store
            .run(TaskUpdateOp {
                actor: s.owner.as_str().to_owned(),
                params: empty_append,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "{engine:?}");

        let mut too_big = base_update(&task.key, full.task.revision);
        too_big.append_body = Some("x".repeat(TASK_BODY_MAX_CHARS));
        let err = s
            .store
            .run(TaskUpdateOp {
                actor: s.owner.as_str().to_owned(),
                params: too_big,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::TooLarge, "{engine:?}");

        let mut ok_append = base_update(&task.key, full.task.revision);
        ok_append.append_body = Some("## Follow-up\nDone.".into());
        let updated = s
            .store
            .run(TaskUpdateOp {
                actor: s.owner.as_str().to_owned(),
                params: ok_append,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap()
            .task;
        assert_eq!(
            updated.body_markdown, "SECRET_LARGE_BODY_MARKER\n\n## Follow-up\nDone.",
            "{engine:?}"
        );
    }
}

fn seed_three_ac_steps(
    s: &AxSeed,
    task_key: &str,
) -> (org_proto::tasks::TasksChecklistResult, [String; 3]) {
    let ids = [next_uuid(), next_uuid(), next_uuid()];
    let (added, _) = s
        .store
        .run(TaskChecklistAddOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksChecklistAddParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                step_id: String::new(),
                text: String::new(),
                required: None,
                section: Some(ChecklistSection::Acceptance),
                steps: vec![
                    ChecklistAddItem {
                        step_id: ids[0].clone(),
                        text: "AC one".into(),
                        required: None,
                        section: Some(ChecklistSection::Acceptance),
                    },
                    ChecklistAddItem {
                        step_id: ids[1].clone(),
                        text: "AC two".into(),
                        required: None,
                        section: Some(ChecklistSection::Acceptance),
                    },
                    ChecklistAddItem {
                        step_id: ids[2].clone(),
                        text: "AC three".into(),
                        required: Some(false),
                        section: Some(ChecklistSection::Acceptance),
                    },
                ],
            },
            authority: TaskWriteAuthority::Unrestricted,
        })
        .unwrap()
        .unwrap();
    (added, ids)
}

#[test]
fn task_90_surgical_checklist_batch_add_and_validation() {
    for engine in engines() {
        let s = seed(engine.clone());
        let task = create_task(&s, "Checklist target", "Body");
        let (added, [id1, id2, id3]) = seed_three_ac_steps(&s, &task.key);
        assert_eq!(added.checklist.len(), 3, "{engine:?}");
        assert_eq!(added.mutated_step_ids, vec![id1, id2, id3]);
        assert!(added.checklist[0].required && added.checklist[1].required);
        assert!(!added.checklist[2].required);

        // Ordinal addressing without `expected_revision` is refused fail-closed.
        let no_if_match = s
            .store
            .run(TaskChecklistCheckOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistCheckParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: String::new(),
                    section: Some(ChecklistSection::Acceptance),
                    indices: vec![1, 3],
                    expected_revision: None,
                    done: true,
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(no_if_match.code, ErrorCode::InvalidRequest, "{engine:?}");

        // 0-based index and duplicate indices are refused.
        for bad in [vec![0], vec![1, 1]] {
            let err = s
                .store
                .run(TaskChecklistCheckOp {
                    actor: s.owner.as_str().to_owned(),
                    params: TasksChecklistCheckParams {
                        command_id: next_uuid(),
                        task: task.key.clone(),
                        step_id: String::new(),
                        section: Some(ChecklistSection::Acceptance),
                        indices: bad,
                        expected_revision: Some(added.task.revision),
                        done: true,
                    },
                    authority: TaskWriteAuthority::Unrestricted,
                })
                .unwrap()
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidRequest, "{engine:?}");
        }
    }
}

#[test]
fn task_90_surgical_checklist_ordinals_and_stale_conflicts() {
    for engine in engines() {
        let s = seed(engine.clone());
        let task = create_task(&s, "Checklist target", "Body");
        let (added, [id1, id2, id3]) = seed_three_ac_steps(&s, &task.key);

        let (checked, _) = s
            .store
            .run(TaskChecklistCheckOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistCheckParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: String::new(),
                    section: Some(ChecklistSection::Acceptance),
                    indices: vec![1, 3],
                    expected_revision: Some(added.task.revision),
                    done: true,
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        assert_eq!(checked.mutated_step_ids, vec![id1.clone(), id3]);
        assert!(
            checked.checklist[0].done && !checked.checklist[1].done && checked.checklist[2].done
        );

        let (edited, _) = s
            .store
            .run(TaskChecklistEditOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistEditParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: String::new(),
                    section: Some(ChecklistSection::Acceptance),
                    index: Some(2),
                    expected_revision: Some(checked.task.revision),
                    text: Some("AC two optional".into()),
                    required: Some(false),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        assert_eq!(edited.mutated_step_ids, vec![id2.clone()]);
        assert!(!edited.checklist[1].required);

        let (moved, _) = s
            .store
            .run(TaskChecklistMoveOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistMoveParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: String::new(),
                    from_section: Some(ChecklistSection::Acceptance),
                    index: Some(2),
                    expected_revision: Some(edited.task.revision),
                    position: 0,
                    section: Some(ChecklistSection::Dod),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        assert!(moved
            .checklist
            .iter()
            .any(|step| step.id == id2 && step.section == ChecklistSection::Dod));

        let (removed, _) = s
            .store
            .run(TaskChecklistRemoveOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistRemoveParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: String::new(),
                    section: Some(ChecklistSection::Dod),
                    index: Some(1),
                    expected_revision: Some(moved.task.revision),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        assert_eq!(removed.mutated_step_ids, vec![id2]);
        assert_eq!(removed.checklist.len(), 2);

        let stale = s
            .store
            .run(TaskChecklistCheckOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistCheckParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: String::new(),
                    section: Some(ChecklistSection::Dod),
                    indices: vec![1],
                    expected_revision: Some(added.task.revision),
                    done: true,
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(stale.code, ErrorCode::Conflict, "{engine:?}");
        let detail = stale.conflict_detail().expect("conflict detail");
        assert_eq!(detail.expected_revision, added.task.revision);
        assert_eq!(detail.current_revision, removed.task.revision);
        assert!(detail.changed_sections.iter().any(|c| c == "checklist"));

        let (by_id, _) = s
            .store
            .run(TaskChecklistCheckOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistCheckParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: id1.clone(),
                    section: None,
                    indices: vec![],
                    expected_revision: None,
                    done: false,
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        assert!(!by_id.checklist.iter().find(|x| x.id == id1).unwrap().done);
    }
}

fn assert_unauthorized_stale_ops_hide_conflict(
    s: &AxSeed,
    engine: EngineChoice,
    task_key: &str,
    base_rev: u64,
    actor: String,
    auth: TaskWriteAuthority,
) {
    let mut stale = base_update(task_key, base_rev);
    stale.title = Some("Unauthorized stale".into());
    let err = s
        .store
        .run(TaskUpdateOp {
            actor: actor.clone(),
            params: stale,
            prepared_files: Default::default(),
            authority: auth.clone(),
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized, "{engine:?}");
    assert!(err.conflict_detail().is_none(), "{engine:?}: {err:?}");

    let err_chk = s
        .store
        .run(TaskChecklistCheckOp {
            actor: actor.clone(),
            params: TasksChecklistCheckParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                step_id: String::new(),
                section: Some(ChecklistSection::Dod),
                indices: vec![1],
                expected_revision: Some(base_rev),
                done: true,
            },
            authority: auth.clone(),
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(err_chk.code, ErrorCode::NotAuthorized, "{engine:?}");
    assert!(err_chk.conflict_detail().is_none(), "{engine:?}");

    let err_tr = s
        .store
        .run(TaskTransitionOp {
            actor: actor.clone(),
            params: TasksTransitionParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                expected_revision: base_rev,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: None,
                done_anyway_reason: None,
                final_summary: None,
            },
            authority: auth.clone(),
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(err_tr.code, ErrorCode::NotAuthorized, "{engine:?}");
    assert!(err_tr.conflict_detail().is_none(), "{engine:?}");

    let err_asg = s
        .store
        .run(TaskAssignOp {
            actor,
            params: org_proto::tasks::TasksAssignParams {
                command_id: next_uuid(),
                task: task_key.to_owned(),
                expected_revision: base_rev,
                assigned_team_id: PatchField::Unchanged,
                assignee_id: PatchField::Set(None),
            },
            authority: auth,
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(err_asg.code, ErrorCode::NotAuthorized, "{engine:?}");
    assert!(err_asg.conflict_detail().is_none(), "{engine:?}");
}

#[test]
fn task_90_conflict_detail_rebuild_parity_and_adr0031_ordering() {
    for engine in engines() {
        let s = seed(engine.clone());
        let task = create_task(&s, "Initial", "SENSITIVE_BODY_CONTENT_NEVER_INLINED");
        let base_rev = task.revision;

        let mut meta_upd = base_update(&task.key, base_rev);
        meta_upd.title = Some("Renamed title".into());
        meta_upd.priority = Some(TaskPriority::Urgent);
        meta_upd.work_type = PatchField::Set(Some(WorkType::Run));
        let rev2 = s
            .store
            .run(TaskUpdateOp {
                actor: s.owner.as_str().to_owned(),
                params: meta_upd,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap()
            .task
            .revision;
        let rev3 = s
            .store
            .run(TaskChecklistAddOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksChecklistAddParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    step_id: next_uuid(),
                    text: "Verify conflict rebuild".into(),
                    required: Some(true),
                    section: Some(ChecklistSection::Dod),
                    steps: vec![],
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap()
            .0
            .task
            .revision;
        assert_eq!((rev2, rev3), (2, 3));

        let mut stale_upd = base_update(&task.key, base_rev);
        stale_upd.title = Some("Stale".into());
        let conflict_before = s
            .store
            .run(TaskUpdateOp {
                actor: s.owner.as_str().to_owned(),
                params: stale_upd,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(conflict_before.code, ErrorCode::Conflict);
        let detail_before = conflict_before.conflict_detail().expect("detail").clone();
        assert_eq!(
            (
                detail_before.expected_revision,
                detail_before.current_revision
            ),
            (base_rev, rev3)
        );
        assert!(!detail_before.reread_recommended, "{detail_before:?}");
        assert_eq!(detail_before.events_since.len(), 2);
        assert!(!serde_json::to_string(&detail_before)
            .unwrap()
            .contains("SENSITIVE_BODY_CONTENT_NEVER_INLINED"));

        let sat_auth = TaskWriteAuthority::Local {
            office_id: id(901),
            role: OfficeRole::Satellite,
            removed_offices: BTreeSet::new(),
        };
        for (actor, auth) in [
            (
                s.inactive.as_str().to_owned(),
                TaskWriteAuthority::Unrestricted,
            ),
            (next_uuid(), TaskWriteAuthority::Unrestricted),
            (s.owner.as_str().to_owned(), sat_auth),
        ] {
            assert_unauthorized_stale_ops_hide_conflict(
                &s,
                engine.clone(),
                &task.key,
                base_rev,
                actor,
                auth,
            );
        }

        let (place, owner, task_key) = (s.place, s.owner, task.key.clone());
        s.store.close().expect("close");
        if engine.is_sqlite() {
            let db_path = place
                .runtime()
                .join(s.here.office_id.as_str())
                .join("office.db");
            let _ = std::fs::remove_file(&db_path);
        }
        let reopened = place.reopen(engine.clone());
        let mut stale_after = base_update(&task_key, base_rev);
        stale_after.title = Some("Stale after rebuild".into());
        let conflict_after = reopened
            .run(TaskUpdateOp {
                actor: owner.as_str().to_owned(),
                params: stale_after,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(
            conflict_after.conflict_detail(),
            Some(&detail_before),
            "{engine:?}"
        );
        reopened.close().expect("close reopened");
    }
}

#[test]
fn task_90_conflict_detail_caps_events_total_bytes_and_hides_comments() {
    for engine in engines() {
        let s = seed(engine.clone());
        let task = create_task(&s, "Initial", "SENSITIVE_BODY_CONTENT_NEVER_INLINED");
        let base_rev = task.revision;
        s.store
            .run(TaskCommentOp {
                actor: s.owner.as_str().to_owned(),
                params: TasksCommentParams {
                    command_id: next_uuid(),
                    task: task.key.clone(),
                    comment_id: next_uuid(),
                    body_markdown: "SECRET_COMMENT_BODY_NEVER_INLINED".into(),
                    attachment_ids: vec![],
                },
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap();
        let long_step_text = "step-detail-".repeat(40);
        for i in 0..25u32 {
            s.store
                .run(TaskChecklistAddOp {
                    actor: s.owner.as_str().to_owned(),
                    params: TasksChecklistAddParams {
                        command_id: next_uuid(),
                        task: task.key.clone(),
                        step_id: next_uuid(),
                        text: format!("Item {i}: {long_step_text}"),
                        required: Some(false),
                        section: Some(ChecklistSection::Work),
                        steps: vec![],
                    },
                    authority: TaskWriteAuthority::Unrestricted,
                })
                .unwrap()
                .unwrap();
        }
        let mut stale_many = base_update(&task.key, base_rev);
        stale_many.title = Some("Stale after many".into());
        let many_err = s
            .store
            .run(TaskUpdateOp {
                actor: s.owner.as_str().to_owned(),
                params: stale_many,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(many_err.code, ErrorCode::Conflict, "{engine:?}");
        let capped = many_err.conflict_detail().expect("capped detail");
        assert!(
            capped.events_since.len() <= CONFLICT_MAX_EVENTS,
            "{engine:?}"
        );
        let total_bytes = serde_json::to_vec(capped).unwrap().len();
        assert!(
            total_bytes <= CONFLICT_MAX_EVENTS_BYTES,
            "{engine:?}: total_bytes={total_bytes}"
        );
        assert!(capped.events_truncated && capped.omitted_events >= 6 && capped.reread_recommended);
        assert!(capped.changed_sections.iter().any(|c| c == "comments"));
        let raw_capped = serde_json::to_string(capped).unwrap();
        assert!(!raw_capped.contains("SECRET_COMMENT_BODY_NEVER_INLINED"));
        assert!(!raw_capped.contains("SENSITIVE_BODY_CONTENT_NEVER_INLINED"));
    }
}
