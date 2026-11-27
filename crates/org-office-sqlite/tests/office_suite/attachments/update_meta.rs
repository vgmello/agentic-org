//! `attachment.update_meta` applies only the fields it names, onto the file as it is where the
//! write is decided (the journal's single writer, or the private space's transaction), and the
//! writer refuses an archived project's file for an edit as for a new one (`TASK-98.1`).

use org_proto::attachments::{
    ArtifactStatus, AttachmentInspectParams, AttachmentTargetKind, AttachmentUpdateMetaParams,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Id, Refusal, Store};

use super::*;
use org_office::attachments::ports::AttachmentsRead;
use org_office::attachments::records::MetaPatch;
use org_office::projects::records::{encode_project, project_path};
use org_office::tasks::ports::TasksRead;
use org_office::testkit_support::store_open::attachment_ops::{
    attachment_operation, AttachmentWrite, AttachmentWriteOp,
};
use org_office::testkit_support::store_open::{OfficeEngine, StoreOp};

fn update(
    office: &Office,
    actor: &str,
    id: &str,
    title: Option<&str>,
    description: Option<&str>,
    status: Option<ArtifactStatus>,
) -> Result<AttachmentView, ApiError> {
    office
        .attachment_update_meta(
            actor,
            AttachmentUpdateMetaParams {
                command_id: next_uuid(),
                attachment_id: id.into(),
                title: title.map(str::to_owned),
                description: description.map(str::to_owned),
                artifact_status: status,
            },
        )
        .unwrap()
        .map(|r| r.attachment)
}

fn inspect(office: &Office, actor: &str, id: &str) -> AttachmentView {
    office
        .attachment_inspect(
            actor,
            AttachmentInspectParams {
                attachment_id: id.into(),
            },
        )
        .unwrap()
        .unwrap()
        .attachment
}

/// Sets a project's `archived` flag through the journal, as an archiving command would.
pub(super) struct ArchiveProject(pub(super) Id);

fn archived<S: TasksRead + AttachmentsRead>(view: &S, id: &Id) -> Result<Change, Refusal> {
    let mut project = view.project(id)?.expect("the project");
    project.archived = true;
    Ok(Change::new(Ack::Durable).put(project_path(id), encode_project(&project)))
}

impl StoreOp for ArchiveProject {
    type Out = ();

    fn run<E: OfficeEngine>(self, store: &Store<E>) {
        let id = self.0;
        store
            .submit(None, false, move |uow| archived(uow.view(), &id))
            .unwrap();
    }
}

/// Two callers edit different fields of one file at once, many times over: every edit of each
/// field lands, none undoes the other's.
fn race_two_fields(office: &Office, actor: &str, id: &str) {
    const ROUNDS: usize = 12;
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            barrier.wait();
            for i in 0..ROUNDS {
                let title = format!("Title {i}");
                update(office, actor, id, Some(&title), None, None).unwrap();
            }
        });
        scope.spawn(|| {
            barrier.wait();
            for i in 0..ROUNDS {
                let description = format!("Description {i}");
                update(office, actor, id, None, Some(&description), None).unwrap();
            }
        });
    });
    let last = inspect(office, actor, id);
    assert_eq!(last.title, format!("Title {}", ROUNDS - 1));
    assert_eq!(
        last.description.as_deref(),
        Some(format!("Description {}", ROUNDS - 1).as_str())
    );
}

#[test]
fn an_edit_names_only_its_fields_and_concurrent_edits_of_other_fields_all_survive() {
    org_office_sqlite::install();
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let staged = upload(office, &fx.agent, "plan.md", "text/markdown", b"# Plan").unwrap();
        let shared = attach_upload(
            office,
            &fx.agent,
            &staged.upload_id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        let staged = upload(office, &fx.person, "mine.txt", "text/plain", b"mine").unwrap();
        let private = attach_upload(
            office,
            &fx.person,
            &staged.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        for (actor, file) in [(&fx.agent, &shared), (&fx.person, &private)] {
            let titled = update(
                office,
                actor,
                &file.id,
                Some("Kept"),
                Some("Kept too"),
                None,
            )
            .unwrap();
            // Only the status is named: the title and description stay as they are now.
            let status = update(
                office,
                actor,
                &file.id,
                None,
                None,
                Some(ArtifactStatus::Review),
            )
            .unwrap();
            assert_eq!(status.title, "Kept", "{engine:?}");
            assert_eq!(status.description.as_deref(), Some("Kept too"));
            assert_eq!(status.artifact_status, ArtifactStatus::Review);
            assert_eq!(titled.artifact_status, file.artifact_status);
            // An empty description clears it, and nothing else.
            let cleared = update(office, actor, &file.id, None, Some(""), None).unwrap();
            assert_eq!(cleared.description, None);
            assert_eq!(cleared.title, "Kept");
            race_two_fields(office, actor, &file.id);
            let last = inspect(office, actor, &file.id);
            assert_eq!(last.artifact_status, ArtifactStatus::Review);
            assert_eq!(last.byte_count, file.byte_count);
            assert_eq!(last.created_at, file.created_at);
        }
    }
}

#[test]
fn an_archived_projects_file_is_refused_an_edit_where_the_write_is_decided() {
    org_office_sqlite::install();
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let staged = upload(office, &fx.agent, "plan.md", "text/markdown", b"# Plan").unwrap();
        let file = attach_upload(
            office,
            &fx.agent,
            &staged.upload_id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        office
            .store_for_test()
            .run(ArchiveProject(Id::parse(&fx.project).unwrap()));
        // Through the facade, the policy refuses it.
        let refused = update(office, &fx.agent, &file.id, Some("Late"), None, None).unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotAuthorized, "{engine:?}");
        // An edit the policy allowed on an earlier snapshot, reaching the writer only now, is
        // refused there too.
        let operation =
            attachment_operation(&next_uuid(), "attachment.update_meta", &fx.agent, b"late")
                .unwrap();
        let written = office
            .store_for_test()
            .run(AttachmentWriteOp {
                operation,
                write: AttachmentWrite::Update {
                    id: Id::parse(&file.id).unwrap(),
                    patch: MetaPatch {
                        title: Some("Late".into()),
                        description: None,
                        artifact_status: None,
                        updated_at: file.updated_at.clone(),
                    },
                },
            })
            .unwrap();
        assert_eq!(written.unwrap_err().code, ErrorCode::NotAuthorized);
        assert_eq!(inspect(office, &fx.agent, &file.id).title, file.title);
    }
}
