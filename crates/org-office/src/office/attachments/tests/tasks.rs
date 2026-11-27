//! Task & task-comment attachments (`TASK-98.4`, ADR-0037): canonical persistence in `task.md`
//! and `comments/<id>.md`, dual-engine (`Memory` + `Sqlite`) `attachments` table projection,
//! lossless Delete+Rebuild parity, `ConflictDetail` `changed_sections`, cap enforcement (20 per
//! task, 10 per comment), and `AuthorizationPolicy` enforcement.

use org_proto::attachments::{
    ArtifactStatus, AttachmentInspectParams, AttachmentListParams, AttachmentTargetKind,
    AttachmentUpdateMetaParams, TASK_ATTACHMENTS_MAX, TASK_COMMENT_ATTACHMENTS_MAX,
};
use org_proto::tasks::{PatchField, TasksCommentParams, TasksCreateParams, TasksUpdateParams};
use org_proto::ErrorCode;

use super::*;

fn create_task_params(
    project: &str,
    title: &str,
    attachment_ids: Vec<String>,
) -> TasksCreateParams {
    TasksCreateParams {
        command_id: next_uuid(),
        task_id: next_uuid(),
        team_id: None,
        project_id: Some(project.to_owned()),
        assigned_team_id: None,
        assignee_id: None,
        task_type: Some("task".into()),
        work_type: None,
        title: title.into(),
        body_markdown: "Task with attachments.".into(),
        priority: None,
        effort: None,
        severity: None,
        rank_order: None,
        labels: vec![],
        reference_files: vec![],
        modified_files: vec![],
        depends_on: vec![],
        details: None,
        dod: None,
        attachment_ids,
    }
}

fn empty_update(task: &str, expected_revision: u64) -> TasksUpdateParams {
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
        add_attachments: vec![],
        remove_attachments: vec![],
    }
}

fn inspect_attachment(office: &Office, actor: &str, id: &str) -> Result<AttachmentView, ApiError> {
    office
        .attachment_inspect(
            actor,
            AttachmentInspectParams {
                attachment_id: id.into(),
            },
        )
        .unwrap()
        .map(|r| r.attachment)
}

fn list_attachments(
    office: &Office,
    actor: &str,
    kind: AttachmentTargetKind,
    target_id: &str,
) -> Vec<AttachmentView> {
    office
        .attachment_list(
            actor,
            AttachmentListParams {
                target_kind: Some(kind),
                target_id: Some(target_id.to_owned()),
                scope: None,
                limit: Some(200),
                cursor: None,
            },
        )
        .unwrap()
        .unwrap()
        .attachments
}

#[test]
fn task_and_comment_attachments_persist_project_and_rebuild_losslessly_on_both_engines() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;

        // 1. Create a task with one staged upload.
        let up_create = upload(office, &fx.agent, "spec.md", "text/markdown", b"# Spec\n").unwrap();
        let create_params = create_task_params(
            &fx.project,
            "Attach spec",
            vec![up_create.upload_id.clone()],
        );
        let created = office
            .task_create(&fx.agent, create_params.clone())
            .unwrap()
            .unwrap();
        // Exact idempotent replay after staged upload was cleaned up still succeeds.
        let replayed = office
            .task_create(&fx.agent, create_params)
            .unwrap()
            .unwrap();
        assert!(replayed.replayed);
        assert_eq!(replayed.task.id, created.task.id);

        let inspected = office.task_inspect(&created.task.key).unwrap().unwrap();
        assert_eq!(inspected.attachments.len(), 1, "{engine:?}");
        let initial_att_id = inspected.attachments[0].id.clone();
        assert_eq!(inspected.attachments[0].filename, "spec.md");
        assert_eq!(
            read_all(office, &fx.other_person, &initial_att_id, 4).unwrap(),
            b"# Spec\n"
        );

        // 2. Attach a second file via `tasks.update` (`add_attachments`) and a third via
        //    `attachment.attach(Task)`, then update metadata on the third via
        //    `attachment.update_meta`.
        let up_update = upload(office, &fx.person, "data.csv", "text/csv", b"a,b\n1,2\n").unwrap();
        let mut upd = empty_update(&created.task.key, created.task.revision);
        upd.add_attachments = vec![up_update.upload_id];
        let after_add = office.task_update(&fx.person, upd).unwrap().unwrap().task;

        // Stale `--if-match` returns structured `ConflictDetail` naming `"attachments"`.
        let mut stale = empty_update(&created.task.key, created.task.revision);
        stale.title = Some("Stale rename".into());
        let conflict = office.task_update(&fx.person, stale).unwrap().unwrap_err();
        assert_eq!(conflict.code, ErrorCode::Conflict, "{engine:?}");
        let detail = conflict
            .conflict_detail()
            .expect("structured ConflictDetail");
        assert_eq!(detail.expected_revision, created.task.revision);
        assert_eq!(detail.current_revision, after_add.revision);
        assert!(
            detail.changed_sections.contains(&"attachments".to_owned()),
            "{detail:?}"
        );

        // Direct `attachment.attach(Task)` + `attachment.update_meta` on task attachment.
        let up_direct =
            upload(office, &fx.owner, "notes.txt", "text/plain", b"owner note").unwrap();
        let direct = attach_upload(
            office,
            &fx.owner,
            &up_direct.upload_id,
            AttachmentTargetKind::Task,
            Some(&created.task.id),
        )
        .unwrap();
        let updated_direct = office
            .attachment_update_meta(
                &fx.owner,
                AttachmentUpdateMetaParams {
                    command_id: next_uuid(),
                    attachment_id: direct.id.clone(),
                    title: Some("Reviewed owner note".into()),
                    description: Some("Verified".into()),
                    artifact_status: Some(ArtifactStatus::Review),
                },
            )
            .unwrap()
            .unwrap()
            .attachment;
        assert_eq!(updated_direct.title, "Reviewed owner note");
        assert_eq!(updated_direct.artifact_status, ArtifactStatus::Review);

        // Remove the initial attachment via `tasks.update` (`remove_attachments`).
        let latest_rev = office
            .task_inspect(&created.task.key)
            .unwrap()
            .unwrap()
            .task
            .revision;
        let mut rm = empty_update(&created.task.key, latest_rev);
        rm.remove_attachments = vec![initial_att_id.clone()];
        office.task_update(&fx.agent, rm).unwrap().unwrap();
        assert_eq!(
            inspect_attachment(office, &fx.agent, &initial_att_id)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        // 3. Add a task comment with an attachment (and empty body!) plus re-attaching an
        //    existing readable task attachment.
        let up_comment = upload(
            office,
            &fx.agent,
            "log.txt",
            "text/plain",
            b"build passed\n",
        )
        .unwrap();
        let comment_params = TasksCommentParams {
            command_id: next_uuid(),
            task: created.task.key.clone(),
            comment_id: next_uuid(),
            body_markdown: String::new(),
            attachment_ids: vec![up_comment.upload_id, direct.id.clone()],
        };
        let (comment_res, replayed) = office
            .task_comment(&fx.agent, comment_params.clone())
            .unwrap()
            .unwrap();
        assert!(!replayed);
        assert_eq!(comment_res.comment.attachments.len(), 2, "{engine:?}");
        let comment_att_id = comment_res.comment.attachments[0].id.clone();

        // Exact retry of comment with attachments replays cleanly.
        let (comment_replay, was_replayed) = office
            .task_comment(&fx.agent, comment_params)
            .unwrap()
            .unwrap();
        assert!(was_replayed);
        assert_eq!(
            comment_replay.comment.attachments,
            comment_res.comment.attachments
        );

        // `attachment.inspect`, `attachment.list`, and `attachment.read_chunk` work on BOTH
        // `Task` and `TaskComment` attachments.
        assert_eq!(
            list_attachments(
                office,
                &fx.person,
                AttachmentTargetKind::Task,
                &created.task.id
            )
            .len(),
            2
        );
        assert_eq!(
            list_attachments(
                office,
                &fx.person,
                AttachmentTargetKind::TaskComment,
                &comment_res.comment.id,
            )
            .len(),
            2
        );
        let comment_view = inspect_attachment(office, &fx.other_person, &comment_att_id).unwrap();
        assert_eq!(comment_view.target_kind, AttachmentTargetKind::TaskComment);

        // 4. Delete runtime projection & rebuild from durable files (`task.md`, `comments/<id>.md`).
        office.sync_store().unwrap();
        let before_inspect = office.task_inspect(&created.task.key).unwrap().unwrap();
        let before_task_list = list_attachments(
            office,
            &fx.person,
            AttachmentTargetKind::Task,
            &created.task.id,
        );
        let before_comment_list = list_attachments(
            office,
            &fx.person,
            AttachmentTargetKind::TaskComment,
            &comment_res.comment.id,
        );

        let fx = fx.rebuilt();
        let office = &fx.office;
        let after_inspect = office.task_inspect(&created.task.key).unwrap().unwrap();
        assert_eq!(
            after_inspect.attachments, before_inspect.attachments,
            "{engine:?}: task attachments survive rebuild"
        );
        assert_eq!(
            after_inspect.comments, before_inspect.comments,
            "{engine:?}: comment attachments survive rebuild"
        );
        assert_eq!(
            list_attachments(
                office,
                &fx.person,
                AttachmentTargetKind::Task,
                &created.task.id
            ),
            before_task_list,
            "{engine:?}"
        );
        assert_eq!(
            list_attachments(
                office,
                &fx.person,
                AttachmentTargetKind::TaskComment,
                &comment_res.comment.id,
            ),
            before_comment_list,
            "{engine:?}"
        );
        assert_eq!(
            read_all(office, &fx.other_person, &updated_direct.id, 16).unwrap(),
            b"owner note"
        );
    }
}

#[test]
fn task_and_comment_attachment_caps_and_acl_are_enforced() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;

        // Task cap: 20 allowed, 21st refused with `TooLarge`.
        let mut twenty = Vec::with_capacity(TASK_ATTACHMENTS_MAX);
        for i in 0..TASK_ATTACHMENTS_MAX {
            let up = upload(
                office,
                &fx.person,
                &format!("f{i}.txt"),
                "text/plain",
                format!("body-{i}").as_bytes(),
            )
            .unwrap();
            twenty.push(up.upload_id);
        }
        let created = office
            .task_create(
                &fx.person,
                create_task_params(&fx.project, "Cap check", twenty),
            )
            .unwrap()
            .unwrap()
            .task;
        let extra = upload(office, &fx.person, "extra.txt", "text/plain", b"21st").unwrap();
        let mut over = empty_update(&created.key, created.revision);
        over.add_attachments = vec![extra.upload_id.clone()];
        assert_eq!(
            office
                .task_update(&fx.person, over)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::TooLarge,
            "{engine:?}: 21st task attachment via tasks.update must fail"
        );
        assert_eq!(
            attach_upload(
                office,
                &fx.person,
                &extra.upload_id,
                AttachmentTargetKind::Task,
                Some(&created.id),
            )
            .unwrap_err()
            .code,
            ErrorCode::TooLarge,
            "{engine:?}: 21st task attachment via attachment.attach must fail"
        );

        // Comment cap: 11 attachments refused with `TooLarge`; 10 allowed.
        let mut eleven = Vec::with_capacity(TASK_COMMENT_ATTACHMENTS_MAX + 1);
        for i in 0..=TASK_COMMENT_ATTACHMENTS_MAX {
            let up = upload(
                office,
                &fx.person,
                &format!("c{i}.txt"),
                "text/plain",
                format!("c-{i}").as_bytes(),
            )
            .unwrap();
            eleven.push(up.upload_id);
        }
        let over_comment = office
            .task_comment(
                &fx.person,
                TasksCommentParams {
                    command_id: next_uuid(),
                    task: created.key.clone(),
                    comment_id: next_uuid(),
                    body_markdown: "too many".into(),
                    attachment_ids: eleven.clone(),
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(over_comment.code, ErrorCode::TooLarge, "{engine:?}");

        // ACL: an agent cannot attach another person's private file onto a task (`NotFound`).
        let secret_up = upload(office, &fx.person, "priv.txt", "text/plain", b"secret").unwrap();
        let private = attach_upload(
            office,
            &fx.person,
            &secret_up.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        let steal = office
            .task_create(
                &fx.agent,
                create_task_params(&fx.project, "Steal private", vec![private.id.clone()]),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(steal.code, ErrorCode::NotFound, "{engine:?}");

        // ACL: an agent asking for `UserPrivate` gets `NotAuthorized` + 1:1 DM `--attach` hint.
        let agent_up = upload(office, &fx.agent, "note.txt", "text/plain", b"hi").unwrap();
        let denied = attach_upload(
            office,
            &fx.agent,
            &agent_up.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap_err();
        assert_eq!(denied.code, ErrorCode::NotAuthorized, "{engine:?}");
        assert!(
            denied.message.contains("1:1 direct message") && denied.message.contains("--attach"),
            "{}",
            denied.message
        );
    }
}
