//! The policy on a real office: uploader-only staging, ACL decoupling from content, explicit
//! re-attach, and the human-only private space (`TASK-98.1` AC #3).

use org_proto::attachments::{
    AttachmentInspectParams, AttachmentListParams, AttachmentReadChunkParams, AttachmentScope,
    AttachmentTargetKind, AttachmentUpdateMetaParams, AttachmentUploadBeginParams,
    AttachmentUploadChunkParams, AttachmentUploadRef,
};
use org_proto::ErrorCode;

use super::*;
use org_office::storage_layout as layout;

fn inspect(office: &Office, actor: &str, id: &str) -> Result<AttachmentView, ApiError> {
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

fn list_private(
    office: &Office,
    actor: &str,
    owner: Option<&str>,
) -> Result<Vec<AttachmentView>, ApiError> {
    office
        .attachment_list(
            actor,
            AttachmentListParams {
                target_kind: None,
                target_id: owner.map(str::to_owned),
                scope: Some(AttachmentScope::UserPrivate),
                limit: None,
                cursor: None,
            },
        )
        .unwrap()
        .map(|r| r.attachments)
}

#[test]
fn a_staged_upload_is_not_found_for_everyone_but_its_uploader() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let office = &fx.office;
        let begun = office
            .attachment_upload_begin(
                &fx.person,
                AttachmentUploadBeginParams {
                    filename: "secret.md".into(),
                    mime_type: "text/markdown".into(),
                    byte_count: 6,
                },
            )
            .unwrap()
            .unwrap();
        let id = begun.upload.upload_id;
        for intruder in [&fx.owner, &fx.other_person, &fx.agent, &fx.outsider_agent] {
            let chunk = office
                .attachment_upload_chunk(
                    intruder,
                    AttachmentUploadChunkParams {
                        upload_id: id.clone(),
                        offset: 0,
                        data_base64: STANDARD.encode(b"hijack"),
                    },
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(chunk.code, ErrorCode::NotFound);
            let commit = office
                .attachment_upload_commit(
                    intruder,
                    AttachmentUploadRef {
                        upload_id: id.clone(),
                    },
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(commit.code, ErrorCode::NotFound);
            let abort = office
                .attachment_upload_abort(
                    intruder,
                    AttachmentUploadRef {
                        upload_id: id.clone(),
                    },
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(abort.code, ErrorCode::NotFound);
            let attach = attach_upload(
                office,
                intruder,
                &id,
                AttachmentTargetKind::Organization,
                None,
            )
            .unwrap_err();
            assert_eq!(attach.code, ErrorCode::NotFound);
        }
        // An unknown upload looks exactly the same as someone else's.
        let unknown = attach_upload(
            office,
            &fx.owner,
            &next_uuid(),
            AttachmentTargetKind::Organization,
            None,
        )
        .unwrap_err();
        assert_eq!(unknown.code, ErrorCode::NotFound);
        assert_eq!(
            unknown.message,
            attach_upload(
                office,
                &fx.owner,
                &id,
                AttachmentTargetKind::Organization,
                None
            )
            .unwrap_err()
            .message
        );
        // Its uploader is unaffected.
        office
            .attachment_upload_chunk(
                &fx.person,
                AttachmentUploadChunkParams {
                    upload_id: id.clone(),
                    offset: 0,
                    data_base64: STANDARD.encode(b"# mine"),
                },
            )
            .unwrap()
            .unwrap();
        office
            .attachment_upload_commit(
                &fx.person,
                AttachmentUploadRef {
                    upload_id: id.clone(),
                },
            )
            .unwrap()
            .unwrap();
        // Attaching needs a committed upload and a target the uploader may write.
        let not_mine = attach_upload(
            office,
            &fx.other_person,
            &id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap_err();
        assert_eq!(not_mine.code, ErrorCode::NotFound);
        attach_upload(
            office,
            &fx.person,
            &id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
    }
}

#[test]
fn the_same_bytes_twice_are_one_blob_and_two_attachments_with_their_own_acls() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let office = &fx.office;
        let bytes = b"quarterly,numbers\n1,2\n";
        let first = upload(office, &fx.person, "q.csv", "text/csv", bytes).unwrap();
        let second = upload(office, &fx.owner, "q-copy.csv", "text/csv", bytes).unwrap();
        let on_project = attach_upload(
            office,
            &fx.person,
            &first.upload_id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        let private = attach_upload(
            office,
            &fx.owner,
            &second.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        assert_ne!(on_project.id, private.id);
        assert_eq!(on_project.content_sha256, private.content_sha256);
        let (shared, private_blobs) = office.attachment_blob_counts().unwrap();
        assert_eq!(
            (shared, private_blobs),
            (1, 1),
            "one shared blob; private bytes kept apart"
        );
        // Same content, different ACLs: the project file is readable, the private one is not.
        inspect(office, &fx.agent, &on_project.id).unwrap();
        assert_eq!(
            inspect(office, &fx.agent, &private.id).unwrap_err().code,
            ErrorCode::NotFound
        );
        // A content hash is never a lookup key.
        assert_eq!(
            inspect(office, &fx.owner, &on_project.content_sha256)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let by_hash = office
            .attachment_read_chunk(
                &fx.owner,
                AttachmentReadChunkParams {
                    attachment_id: private.content_sha256.clone(),
                    offset: 0,
                    max_bytes: None,
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(by_hash.code, ErrorCode::NotFound);
        // Re-attaching mints a new attachment over the same blob.
        let again = reattach(
            office,
            &fx.agent,
            &on_project.id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        assert_ne!(again.id, on_project.id);
        assert_eq!(again.content_sha256, on_project.content_sha256);
        assert_eq!(again.uploaded_by_employee_id, fx.agent);
        assert_eq!(office.attachment_blob_counts().unwrap(), (1, 1));
    }
}

#[test]
fn re_attaching_needs_read_on_the_source_and_write_on_the_target() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let office = &fx.office;
        let staged = upload(office, &fx.owner, "brand.md", "text/markdown", b"# Brand").unwrap();
        let org_file = attach_upload(
            office,
            &fx.owner,
            &staged.upload_id,
            AttachmentTargetKind::Organization,
            None,
        )
        .unwrap();
        // The outsider may read the organization file but not write the project.
        let refused = reattach(
            office,
            &fx.outsider_agent,
            &org_file.id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotAuthorized);
        let refused = reattach(
            office,
            &fx.other_person,
            &org_file.id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotAuthorized);
        // Phase 1 (`doc-103` §1.B): any active employee, an agent included, publishes
        // organization-wide.
        let staged = upload(office, &fx.agent, "out.md", "text/markdown", b"# Out").unwrap();
        let published = attach_upload(
            office,
            &fx.agent,
            &staged.upload_id,
            AttachmentTargetKind::Organization,
            None,
        )
        .unwrap();
        assert_eq!(published.scope, AttachmentScope::Shared);
        inspect(office, &fx.other_person, &published.id).unwrap();
        // A project member may bring it into the project.
        reattach(
            office,
            &fx.agent,
            &org_file.id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        // A private file can leave the space only through its owner.
        let staged = upload(office, &fx.person, "draft.md", "text/markdown", b"# Draft").unwrap();
        let private = attach_upload(
            office,
            &fx.person,
            &staged.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        let refused = reattach(
            office,
            &fx.owner,
            &private.id,
            AttachmentTargetKind::Organization,
            None,
        )
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound);
        let shared = reattach(
            office,
            &fx.person,
            &private.id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        assert_eq!(shared.scope, AttachmentScope::Shared);
        inspect(office, &fx.agent, &shared.id).unwrap();
        assert_eq!(
            inspect(office, &fx.agent, &private.id).unwrap_err().code,
            ErrorCode::NotFound
        );
        // Targets `attachment.attach` does not bind directly are refused, not silently accepted.
        for kind in [
            AttachmentTargetKind::Message,
            AttachmentTargetKind::ReviewFinding,
        ] {
            let refused =
                reattach(office, &fx.person, &shared.id, kind, Some(&next_uuid())).unwrap_err();
            assert_eq!(refused.code, ErrorCode::Unsupported, "{kind:?}");
        }
        // Exactly one source.
        let mut both = attach_params(AttachmentTargetKind::Organization, None);
        both.upload_id = Some(next_uuid());
        both.source_attachment_id = Some(shared.id.clone());
        assert_eq!(
            office
                .attachment_attach(&fx.owner, both)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
}

#[test]
fn the_private_space_is_its_persons_alone_and_never_an_agents() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let office = &fx.office;
        let staged = upload(
            office,
            &fx.person,
            "diary.txt",
            "text/plain",
            b"private thoughts",
        )
        .unwrap();
        let mine = attach_upload(
            office,
            &fx.person,
            &staged.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        assert_eq!(mine.scope, AttachmentScope::UserPrivate);
        assert_eq!(mine.target_id.as_deref(), Some(fx.person.as_str()));
        assert_eq!(
            read_all(office, &fx.person, &mine.id, 4).unwrap(),
            b"private thoughts"
        );
        assert_eq!(
            list_private(office, &fx.person, None).unwrap(),
            vec![mine.clone()]
        );
        office
            .attachment_update_meta(
                &fx.person,
                AttachmentUpdateMetaParams {
                    command_id: next_uuid(),
                    attachment_id: mine.id.clone(),
                    title: Some("Diary".into()),
                    description: None,
                    artifact_status: None,
                },
            )
            .unwrap()
            .unwrap();
        for intruder in [&fx.owner, &fx.other_person, &fx.agent, &fx.outsider_agent] {
            assert_eq!(
                inspect(office, intruder, &mine.id).unwrap_err().code,
                ErrorCode::NotFound
            );
            let read = office
                .attachment_read_chunk(
                    intruder,
                    AttachmentReadChunkParams {
                        attachment_id: mine.id.clone(),
                        offset: 0,
                        max_bytes: None,
                    },
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(read.code, ErrorCode::NotFound);
            let edit = office
                .attachment_update_meta(
                    intruder,
                    AttachmentUpdateMetaParams {
                        command_id: next_uuid(),
                        attachment_id: mine.id.clone(),
                        title: Some("pwned".into()),
                        description: None,
                        artifact_status: None,
                    },
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(edit.code, ErrorCode::NotFound);
            assert_eq!(
                list_private(office, intruder, Some(&fx.person))
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }
        // An agent has no private space of its own, and is told so, with where to go instead;
        // a person's space stays one it cannot learn exists.
        let own = list_private(office, &fx.agent, None).unwrap_err();
        assert_eq!(own.code, ErrorCode::NotAuthorized);
        assert!(own.message.contains("1:1 direct message"), "{own:?}");
        let staged = upload(office, &fx.agent, "notes.txt", "text/plain", b"agent notes").unwrap();
        for (target, expected) in [
            (None, ErrorCode::NotAuthorized),
            (Some(fx.agent.as_str()), ErrorCode::NotAuthorized),
            (Some(fx.person.as_str()), ErrorCode::NotFound),
        ] {
            let refused = attach_upload(
                office,
                &fx.agent,
                &staged.upload_id,
                AttachmentTargetKind::UserPrivate,
                target,
            )
            .unwrap_err();
            assert_eq!(refused.code, expected, "{target:?}");
        }
        assert_eq!(
            list_private(office, &fx.agent, Some(&fx.person))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        // Not even a person may write another person's space.
        let other = upload(
            office,
            &fx.other_person,
            "gift.txt",
            "text/plain",
            b"for you",
        )
        .unwrap();
        let refused = attach_upload(
            office,
            &fx.other_person,
            &other.upload_id,
            AttachmentTargetKind::UserPrivate,
            Some(&fx.person),
        )
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound);
        // Bytes under `private/blobs/`, never under the shared root or `state/`.
        let durable = office.durable_root();
        assert!(layout::private_blobs(durable).join("sha256").is_dir());
        assert!(!layout::shared_blobs(durable).join("sha256").exists());
        assert!(!durable.join("state/files").exists());
        if engine.is_sqlite() {
            let db = durable.join("private/user-space.db");
            assert!(db.is_file());
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&db).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn a_satellite_has_no_private_space_and_writes_shared_files_only_through_its_outbox() {
    let sat = org_office::TestSatellite::new();
    let office = &sat.office;
    let durable = office.durable_root().to_path_buf();
    let anyone = next_uuid();
    let refused = list_private(office, &anyone, None).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unavailable);
    // A private binding is refused before anything else, the same for every caller.
    let mut params = attach_params(AttachmentTargetKind::UserPrivate, None);
    params.upload_id = Some(next_uuid());
    let refused = office
        .attachment_attach(&anyone, params)
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unavailable);
    // Shared uploads and bindings are a satellite's own now (`TASK-98.6`): a stranger is
    // refused as on HQ, never told the office cannot.
    let begin = office
        .attachment_upload_begin(
            &anyone,
            AttachmentUploadBeginParams {
                filename: "x.txt".into(),
                mime_type: "text/plain".into(),
                byte_count: 1,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(begin.code, ErrorCode::NotAuthorized);
    let mut params = attach_params(AttachmentTargetKind::Organization, None);
    params.upload_id = Some(next_uuid());
    let refused = office
        .attachment_attach(&anyone, params)
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotAuthorized);
    assert!(
        !durable.join("private").exists(),
        "ARCH-02: a satellite never makes private/"
    );
    assert_eq!(office.attachment_blob_counts().unwrap(), (0, 0));
    assert_eq!(office.attachment_outbox_len().unwrap(), 0);
    assert!(office.attachment_outbox_pending().unwrap().is_empty());
}

#[test]
fn an_unknown_or_inactive_caller_is_refused_before_anything_is_read() {
    let fx = Fixture::new(EngineChoice::Memory);
    let stranger = next_uuid();
    let refused = list_private(&fx.office, &stranger, None).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotAuthorized);
    let refused = fx
        .office
        .attachment_upload_begin(
            &stranger,
            AttachmentUploadBeginParams {
                filename: "x.txt".into(),
                mime_type: "text/plain".into(),
                byte_count: 1,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotAuthorized);
}

#[test]
fn a_list_whose_scope_contradicts_its_target_kind_is_refused_not_reinterpreted() {
    let fx = Fixture::new(EngineChoice::Memory);
    let staged = upload(&fx.office, &fx.person, "mine.txt", "text/plain", b"mine").unwrap();
    attach_upload(
        &fx.office,
        &fx.person,
        &staged.upload_id,
        AttachmentTargetKind::UserPrivate,
        None,
    )
    .unwrap();
    let list = |scope, kind, target: Option<&str>| {
        fx.office
            .attachment_list(
                &fx.person,
                AttachmentListParams {
                    target_kind: kind,
                    target_id: target.map(str::to_owned),
                    scope,
                    limit: None,
                    cursor: None,
                },
            )
            .unwrap()
            .map(|r| r.attachments.len())
    };
    for (scope, kind, target) in [
        (
            AttachmentScope::UserPrivate,
            AttachmentTargetKind::Project,
            Some(fx.project.as_str()),
        ),
        (
            AttachmentScope::UserPrivate,
            AttachmentTargetKind::Organization,
            None,
        ),
        (
            AttachmentScope::Shared,
            AttachmentTargetKind::UserPrivate,
            None,
        ),
    ] {
        assert_eq!(
            list(Some(scope), Some(kind), target).unwrap_err().code,
            ErrorCode::InvalidRequest,
            "{scope:?} {kind:?}"
        );
    }
    // Agreeing or absent, the scope lists as before.
    assert_eq!(list(Some(AttachmentScope::UserPrivate), None, None), Ok(1));
    assert_eq!(
        list(
            Some(AttachmentScope::UserPrivate),
            Some(AttachmentTargetKind::UserPrivate),
            None
        ),
        Ok(1)
    );
    assert_eq!(
        list(None, Some(AttachmentTargetKind::UserPrivate), None),
        Ok(1)
    );
    assert_eq!(
        list(
            Some(AttachmentScope::Shared),
            Some(AttachmentTargetKind::Organization),
            None
        ),
        Ok(0)
    );
}

#[test]
fn delivery_read_refuses_user_private_unauthorized_dm_and_enforces_chief_of_staff_caller_intersection(
) {
    use org_office::attachments::{DeliveryAttachmentPayload, DeliveryScope};
    use org_proto::chat::{ChatSendParams, DmOpenParams};
    use org_proto::org::MemberRemoveParams;

    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let office = &fx.office;
        let tess = office
            .chief_of_staff()
            .unwrap()
            .expect("built-in Chief of Staff")
            .id
            .to_string();

        // 1. `user_private` attachment: refused for ordinary agents AND for Tess (even acting for Owner).
        let priv_up = upload(office, &fx.owner, "diary.txt", "text/plain", b"owner-only").unwrap();
        let priv_att = attach_upload(
            office,
            &fx.owner,
            &priv_up.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &fx.agent,
                    &priv_att.id,
                    &DeliveryScope::None,
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "agent must never read user_private via delivery read"
        );
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &priv_att.id,
                    &DeliveryScope::ActingForPerson(fx.owner.clone()),
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "Chief of Staff must never read owner user_private even when acting for owner"
        );

        // 2. Bob (`fx.person`) <-> Tess DM with a document attachment.
        let _ = office
            .dm_open(
                &fx.person,
                DmOpenParams {
                    participants: vec![tess.clone()],
                    name: None,
                    conversation_id: None,
                },
            )
            .unwrap()
            .unwrap();
        let bob_up = upload(
            office,
            &fx.person,
            "bob-secret.txt",
            "text/plain",
            b"private bob notes",
        )
        .unwrap();
        let bob_msg = office
            .chat_send(
                &fx.person,
                ChatSendParams {
                    id: next_uuid(),
                    to: tess.clone(),
                    kind: org_proto::msg::KIND_MSG.into(),
                    priority: None,
                    subject: None,
                    body: "Strictly between Bob and Tess".into(),
                    attachment_ids: vec![bob_up.upload_id],
                    meta: Default::default(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap();
        let bob_dm_att_id = bob_msg.message.attachments[0].id.clone();

        // Ordinary agent outside Bob<->Tess DM is refused.
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &fx.outsider_agent,
                    &bob_dm_att_id,
                    &DeliveryScope::None,
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "non-participant agent must be refused DM attachment"
        );

        // Chief of Staff acting for Bob (active Person participant) succeeds!
        let ok_for_bob = office
            .attachment_read_for_delivery_with(
                &tess,
                &bob_dm_att_id,
                &DeliveryScope::ActingForPerson(fx.person.clone()),
                u64::MAX,
                None,
            )
            .unwrap()
            .unwrap();
        assert!(matches!(
            ok_for_bob,
            DeliveryAttachmentPayload::Document {
                bytes: Some(ref b),
                ..
            } if b == b"private bob notes"
        ));

        // Clamping check: remaining_budget smaller than file returns `bytes: None` without error.
        let clamped = office
            .attachment_read_for_delivery_with(
                &tess,
                &bob_dm_att_id,
                &DeliveryScope::ActingForPerson(fx.person.clone()),
                4,
                None,
            )
            .unwrap()
            .unwrap();
        assert!(matches!(
            clamped,
            DeliveryAttachmentPayload::Document { bytes: None, .. }
        ));

        // Chief of Staff acting for Owner MUST NOT cross into Bob<->Tess DM!
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &bob_dm_att_id,
                    &DeliveryScope::ActingForPerson(fx.owner.clone()),
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "owner must NEVER read another member's DM with Tess via caller ∩ tess"
        );

        // Chief of Staff in `tess:wake` (`DeliveryScope::None`) MUST refuse DM attachments!
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &bob_dm_att_id,
                    &DeliveryScope::None,
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "tess:wake must refuse DM attachments fail-closed"
        );

        // Chief of Staff in `tess:dm:<agent>` (`DirectSender`) can read agent's own DM, never Bob's.
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &bob_dm_att_id,
                    &DeliveryScope::DirectSender(fx.agent.clone()),
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "tess:dm:<agent> must refuse Bob<->Tess DM attachment"
        );
        let _ = office
            .dm_open(
                &fx.agent,
                DmOpenParams {
                    participants: vec![tess.clone()],
                    name: None,
                    conversation_id: None,
                },
            )
            .unwrap()
            .unwrap();
        let ag_up = upload(office, &fx.agent, "bot.txt", "text/plain", b"from agent").unwrap();
        let ag_msg = office
            .chat_send(
                &fx.agent,
                ChatSendParams {
                    id: next_uuid(),
                    to: tess.clone(),
                    kind: org_proto::msg::KIND_MSG.into(),
                    priority: None,
                    subject: None,
                    body: "Agent report".into(),
                    attachment_ids: vec![ag_up.upload_id],
                    meta: Default::default(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap();
        let ag_dm_att_id = ag_msg.message.attachments[0].id.clone();
        assert!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &ag_dm_att_id,
                    &DeliveryScope::DirectSender(fx.agent.clone()),
                    u64::MAX,
                    None
                )
                .unwrap()
                .is_ok(),
            "tess:dm:<agent> may read attachment from <agent><->Tess DM"
        );
        // Agent ID passed in `ActingForPerson` must be refused (only active Person may delegate).
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &ag_dm_att_id,
                    &DeliveryScope::ActingForPerson(fx.agent.clone()),
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "ActingForPerson must require a Person, never an Agent"
        );

        // 3. Public channel attachment: allowed in `tess:wake` (`DeliveryScope::None`)!
        let ch_up = upload(
            office,
            &fx.owner,
            "announce.txt",
            "text/plain",
            b"channel announcement",
        )
        .unwrap();
        let ch_msg = office
            .chat_send(
                &fx.owner,
                ChatSendParams {
                    id: next_uuid(),
                    to: "#general".into(),
                    kind: org_proto::msg::KIND_MSG.into(),
                    priority: None,
                    subject: None,
                    body: "Public channel file".into(),
                    attachment_ids: vec![ch_up.upload_id],
                    meta: Default::default(),
                    reply_to: None,
                    also_in_channel: false,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap();
        let ch_att_id = ch_msg.message.attachments[0].id.clone();
        assert!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &ch_att_id,
                    &DeliveryScope::None,
                    u64::MAX,
                    None
                )
                .unwrap()
                .is_ok(),
            "tess:wake may read public channel attachments"
        );

        // 4. Deactivated / offboarded caller (`MemberRemove`) is immediately refused!
        let bob_rev = office
            .load_employee(&fx.person)
            .unwrap()
            .expect("bob exists")
            .meta
            .revision;
        office
            .submit_org(
                &fx.owner,
                OrgCommand::MemberRemove(MemberRemoveParams {
                    command_id: next_uuid(),
                    member_id: fx.person.clone(),
                    expected_version: bob_rev,
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &bob_dm_att_id,
                    &DeliveryScope::ActingForPerson(fx.person.clone()),
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "deactivated/offboarded caller must be refused immediately"
        );

        // 5. Offboarded/inactive agent in `tess:dm:<agent>` is also immediately refused!
        let agent_rev = office
            .load_employee(&fx.agent)
            .unwrap()
            .expect("agent exists")
            .meta
            .revision;
        let offboard_req = org_proto::Request::new::<org_proto::hiring::AgentOffboard>(
            99,
            &org_proto::hiring::AgentOffboardParams {
                command_id: next_uuid(),
                member_id: fx.agent.clone(),
                expected_version: agent_rev,
            },
        )
        .unwrap();
        assert!(matches!(
            office
                .dispatch_hiring_rpc(Some(&fx.owner), &offboard_req)
                .unwrap()
                .outcome,
            org_proto::Outcome::Result(_)
        ));
        assert_eq!(
            office
                .attachment_read_for_delivery_with(
                    &tess,
                    &ag_dm_att_id,
                    &DeliveryScope::DirectSender(fx.agent.clone()),
                    u64::MAX,
                    None
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "offboarded/inactive DirectSender must be refused immediately"
        );
    }
}
