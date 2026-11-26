//! Standalone shared files are canonical YAML under `durable/state/`, and the projection is
//! rebuilt from them losslessly (`doc-7` #1, `TASK-98.1` AC #1 and #5).

use org_proto::attachments::{
    ArtifactStatus, AttachmentInspectParams, AttachmentListParams, AttachmentTargetKind,
    AttachmentUpdateMetaParams,
};
use org_proto::ErrorCode;
use org_store::DurablePath;

use super::*;
use crate::attachments::records;

fn view(office: &Office, actor: &str, id: &str) -> Result<AttachmentView, ApiError> {
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

fn listed(
    office: &Office,
    actor: &str,
    kind: AttachmentTargetKind,
    target: Option<&str>,
) -> Vec<AttachmentView> {
    office
        .attachment_list(
            actor,
            AttachmentListParams {
                target_kind: Some(kind),
                target_id: target.map(str::to_owned),
                scope: None,
                limit: Some(200),
                cursor: None,
            },
        )
        .unwrap()
        .unwrap()
        .attachments
}

fn attachment_rows(office: &Office) -> Vec<String> {
    let mut rows: Vec<String> = office
        .dump()
        .into_iter()
        .filter(|line| line.starts_with("attachments"))
        .collect();
    rows.sort();
    rows
}

#[test]
fn standalone_files_are_canonical_yaml_and_survive_deleting_and_rebuilding_the_projection() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let staged = upload(office, &fx.agent, "plan.md", "text/markdown", b"# Plan\n").unwrap();
        let project_file = attach_upload(
            office,
            &fx.agent,
            &staged.upload_id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        let staged = upload(
            office,
            &fx.owner,
            "handbook.pdf",
            "application/pdf",
            &crate::attachments::media::tests::fixtures::pdf(2),
        )
        .unwrap();
        let mut params = attach_params(AttachmentTargetKind::Organization, None);
        params.upload_id = Some(staged.upload_id);
        params.title = Some("Employee handbook".into());
        params.description = Some("Read me first".into());
        params.artifact_status = Some(ArtifactStatus::Draft);
        let org_file = office
            .attachment_attach(&fx.owner, params)
            .unwrap()
            .unwrap()
            .attachment;
        let project_file = office
            .attachment_update_meta(
                &fx.agent,
                AttachmentUpdateMetaParams {
                    command_id: next_uuid(),
                    attachment_id: project_file.id.clone(),
                    title: Some("Launch plan".into()),
                    description: None,
                    artifact_status: Some(ArtifactStatus::Superseded),
                },
            )
            .unwrap()
            .unwrap()
            .attachment;
        let staged = upload(office, &fx.person, "mine.txt", "text/plain", b"just me").unwrap();
        let private = attach_upload(
            office,
            &fx.person,
            &staged.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        office.sync_store().unwrap();

        // The canonical files, where the contract puts them, hold exactly the records.
        let durable = office.durable_root().to_path_buf();
        let project_path = format!(
            "state/projects/{}/files/{}.yaml",
            fx.project, project_file.id
        );
        let org_path = format!("state/files/{}.yaml", org_file.id);
        for (path, expected) in [(&project_path, &project_file), (&org_path, &org_file)] {
            let bytes = std::fs::read(durable.join(path)).unwrap();
            let record = records::decode(&bytes).unwrap();
            assert_eq!(&record.to_view(), expected, "{path}");
            assert!(records::kind_of(&DurablePath::parse(path).unwrap()).is_some());
        }
        // Nothing of the private file is under `state/`.
        let state = durable.join("state");
        let mut stack = vec![state];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    stack.push(entry.path());
                } else {
                    let text = std::fs::read(entry.path()).unwrap();
                    let id = private.id.as_bytes();
                    assert!(
                        !text.windows(id.len()).any(|w| w == id),
                        "{}",
                        entry.path().display()
                    );
                }
            }
        }

        let rows_before = attachment_rows(office);
        assert_eq!(rows_before.len(), 2, "{rows_before:?}");
        let project_list = listed(
            office,
            &fx.person,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        );
        let org_list = listed(office, &fx.agent, AttachmentTargetKind::Organization, None);

        let fx = fx.rebuilt();
        let office = &fx.office;
        assert_eq!(
            attachment_rows(office),
            rows_before,
            "{engine:?}: lossless rebuild"
        );
        assert_eq!(
            view(office, &fx.person, &project_file.id).unwrap(),
            project_file
        );
        assert_eq!(view(office, &fx.agent, &org_file.id).unwrap(), org_file);
        assert_eq!(
            listed(
                office,
                &fx.person,
                AttachmentTargetKind::Project,
                Some(&fx.project)
            ),
            project_list
        );
        assert_eq!(
            listed(office, &fx.agent, AttachmentTargetKind::Organization, None),
            org_list
        );
        assert_eq!(
            read_all(office, &fx.other_person, &project_file.id, 3).unwrap(),
            b"# Plan\n"
        );
        // The private space is HQ's authoritative database, not a projection: on SQLite it
        // is untouched by the rebuild; the memory engine's starts empty with each opening.
        match engine {
            EngineChoice::Sqlite => {
                assert_eq!(view(office, &fx.person, &private.id).unwrap(), private);
                assert_eq!(
                    read_all(office, &fx.person, &private.id, 100).unwrap(),
                    b"just me"
                );
            }
            EngineChoice::Memory => {
                assert_eq!(
                    view(office, &fx.person, &private.id).unwrap_err().code,
                    ErrorCode::NotFound
                );
            }
        }
    }
}

#[test]
fn a_retried_attach_is_answered_once_even_after_its_upload_is_consumed() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let staged = upload(office, &fx.person, "a.txt", "text/plain", b"a").unwrap();
        let mut params = attach_params(AttachmentTargetKind::Project, Some(&fx.project));
        params.upload_id = Some(staged.upload_id.clone());
        let first = office
            .attachment_attach(&fx.person, params.clone())
            .unwrap()
            .unwrap();
        let again = office
            .attachment_attach(&fx.person, params.clone())
            .unwrap()
            .unwrap();
        assert_eq!(first, again);
        assert_eq!(
            listed(
                office,
                &fx.person,
                AttachmentTargetKind::Project,
                Some(&fx.project)
            )
            .len(),
            1
        );
        // The same command id with another request, or from another caller, is refused.
        let mut changed = params.clone();
        changed.title = Some("Other".into());
        assert_eq!(
            office
                .attachment_attach(&fx.person, changed)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::OperationReused
        );
        assert_eq!(
            office
                .attachment_attach(&fx.agent, params)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::OperationReused
        );
        // Private bindings retry the same way.
        let staged = upload(office, &fx.person, "b.txt", "text/plain", b"b").unwrap();
        let mut params = attach_params(AttachmentTargetKind::UserPrivate, None);
        params.upload_id = Some(staged.upload_id);
        let first = office
            .attachment_attach(&fx.person, params.clone())
            .unwrap()
            .unwrap();
        assert_eq!(
            office
                .attachment_attach(&fx.person, params)
                .unwrap()
                .unwrap(),
            first
        );
    }
}

fn attach_private(
    office: &Office,
    actor: &str,
    params: &AttachmentAttachParams,
) -> Result<AttachmentView, ApiError> {
    office
        .attachment_attach(actor, params.clone())
        .unwrap()
        .map(|r| r.attachment)
}

fn staged_upload_exists(office: &Office, actor: &str, upload_id: &str) -> bool {
    office
        .attachment_upload_commit(
            actor,
            org_proto::attachments::AttachmentUploadRef {
                upload_id: upload_id.into(),
            },
        )
        .unwrap()
        .is_ok()
}

#[test]
fn a_private_command_id_reused_for_another_upload_is_refused_and_loses_nothing() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let first = upload(office, &fx.person, "a.txt", "text/plain", b"first").unwrap();
        let mut params = attach_params(AttachmentTargetKind::UserPrivate, None);
        params.upload_id = Some(first.upload_id.clone());
        let kept = attach_private(office, &fx.person, &params).unwrap();
        // The same command id with a new upload: refused, and the new upload stays staged.
        let second = upload(office, &fx.person, "b.txt", "text/plain", b"second").unwrap();
        let mut reused = params.clone();
        reused.upload_id = Some(second.upload_id.clone());
        assert_eq!(
            attach_private(office, &fx.person, &reused)
                .unwrap_err()
                .code,
            ErrorCode::OperationReused,
            "{engine:?}"
        );
        assert!(staged_upload_exists(office, &fx.person, &second.upload_id));
        // The same command id with other metadata, while the upload is still staged: refused.
        let mut retitled = reused.clone();
        retitled.title = Some("Second".into());
        assert_eq!(
            attach_private(office, &fx.person, &retitled)
                .unwrap_err()
                .code,
            ErrorCode::OperationReused
        );
        assert!(staged_upload_exists(office, &fx.person, &second.upload_id));
        // Under a command id of its own the new upload binds, and the first file is intact.
        let mut fresh = reused.clone();
        fresh.command_id = next_uuid();
        let bound = attach_private(office, &fx.person, &fresh).unwrap();
        assert_ne!(bound.id, kept.id);
        assert_eq!(
            read_all(office, &fx.person, &bound.id, 100).unwrap(),
            b"second"
        );
        assert_eq!(
            read_all(office, &fx.person, &kept.id, 100).unwrap(),
            b"first"
        );
        assert_eq!(attach_private(office, &fx.person, &params).unwrap(), kept);
    }
}

#[test]
fn another_persons_private_command_id_is_their_own_and_reveals_nothing() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let mine = upload(office, &fx.person, "a.txt", "text/plain", b"pat's").unwrap();
        let mut params = attach_params(AttachmentTargetKind::UserPrivate, None);
        params.upload_id = Some(mine.upload_id.clone());
        let pats = attach_private(office, &fx.person, &params).unwrap();
        // Dana sends the very same command id (and title) with an upload of her own.
        let hers = upload(office, &fx.other_person, "a.txt", "text/plain", b"dana's").unwrap();
        let mut same_id = params.clone();
        same_id.upload_id = Some(hers.upload_id.clone());
        let danas = attach_private(office, &fx.other_person, &same_id).unwrap();
        assert_ne!(danas.id, pats.id, "{engine:?}");
        assert_eq!(
            read_all(office, &fx.other_person, &danas.id, 100).unwrap(),
            b"dana's"
        );
        // Her retry answers her file; Pat's retry still answers his.
        assert_eq!(
            attach_private(office, &fx.other_person, &same_id).unwrap(),
            danas
        );
        assert_eq!(attach_private(office, &fx.person, &params).unwrap(), pats);
        // Someone who never used the id, replaying Pat's exact request (his consumed upload),
        // learns nothing of it: the upload is simply not theirs.
        assert_eq!(
            attach_private(office, &fx.owner, &params).unwrap_err().code,
            ErrorCode::NotFound
        );
    }
}

#[test]
fn a_private_retry_with_other_metadata_or_target_is_refused() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let staged = upload(office, &fx.person, "a.txt", "text/plain", b"a").unwrap();
        let mut params = attach_params(AttachmentTargetKind::UserPrivate, None);
        params.upload_id = Some(staged.upload_id.clone());
        params.title = Some("Mine".into());
        let first = attach_private(office, &fx.person, &params).unwrap();
        // The upload is consumed: each retry below is answered from the recorded command.
        assert_eq!(attach_private(office, &fx.person, &params).unwrap(), first);
        let mut retitled = params.clone();
        retitled.title = Some("Other".into());
        let mut described = params.clone();
        described.description = Some("more".into());
        let mut own_id = params.clone();
        own_id.target_id = Some(fx.person.clone());
        // Retargeting one's own command at another space is a reuse of one's own command id:
        // the answer says nothing about that space.
        let mut elsewhere = params.clone();
        elsewhere.target_id = Some(fx.other_person.clone());
        for (what, changed) in [
            ("title", retitled),
            ("description", described),
            ("explicit target", own_id),
            ("another space", elsewhere),
        ] {
            assert_eq!(
                attach_private(office, &fx.person, &changed)
                    .unwrap_err()
                    .code,
                ErrorCode::OperationReused,
                "{engine:?} {what}"
            );
        }
        assert_eq!(
            listed(
                office,
                &fx.person,
                AttachmentTargetKind::UserPrivate,
                Some(&fx.person)
            ),
            vec![first]
        );
    }
}
