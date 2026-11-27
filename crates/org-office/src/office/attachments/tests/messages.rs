//! Files on chat messages and replies on a real office, on both engines (`TASK-98.2`):
//! attachment-only messages, mixed images and documents, the ten-file cap, uploader-only
//! staged uploads, exact retries, `has:image` / `has:file`, and who may read a file.

use org_proto::attachments::{AttachmentInspectParams, AttachmentReadChunkParams};
use org_proto::chat::{ChatInboxParams, ChatMessageView, ChatMessagesParams, ChatSendParams};
use org_proto::msg::MsgReplyParams;
use org_proto::ErrorCode;

use super::*;
use crate::attachments::media::tests::fixtures;
use crate::messaging::features::ChatSendCommitted;

pub(super) fn send(
    office: &Office,
    actor: &str,
    to: &str,
    body: &str,
    files: &[&str],
) -> Result<ChatSendCommitted, ApiError> {
    office
        .chat_send(actor, send_params(to, body, files))
        .unwrap()
}

pub(super) fn send_params(to: &str, body: &str, files: &[&str]) -> ChatSendParams {
    ChatSendParams {
        id: next_uuid(),
        to: to.into(),
        kind: org_proto::msg::KIND_MSG.into(),
        priority: None,
        subject: None,
        body: body.into(),
        meta: Default::default(),
        reply_to: None,
        also_in_channel: false,
        reason: None,
        attachment_ids: files.iter().map(|f| (*f).to_owned()).collect(),
    }
}

pub(super) fn messages(office: &Office, actor: &str, conversation: &str) -> Vec<ChatMessageView> {
    read_messages(
        office,
        actor,
        ChatMessagesParams {
            conversation: Some(conversation.into()),
            limit: Some(100),
            ..Default::default()
        },
    )
}

fn read_messages(office: &Office, actor: &str, params: ChatMessagesParams) -> Vec<ChatMessageView> {
    office
        .chat_messages(actor, params)
        .unwrap()
        .unwrap()
        .messages
}

pub(super) fn inspect(office: &Office, actor: &str, id: &str) -> Result<AttachmentView, ApiError> {
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

fn read_first_chunk(office: &Office, actor: &str, id: &str) -> Result<u64, ApiError> {
    office
        .attachment_read_chunk(
            actor,
            AttachmentReadChunkParams {
                attachment_id: id.into(),
                offset: 0,
                max_bytes: None,
            },
        )
        .unwrap()
        .map(|r| r.total_bytes)
}

pub(super) fn png(office: &Office, actor: &str, name: &str) -> String {
    upload(office, actor, name, "image/png", &fixtures::png(8, 6))
        .unwrap()
        .upload_id
}

pub(super) fn pdf(office: &Office, actor: &str, name: &str) -> String {
    upload(office, actor, name, "application/pdf", &fixtures::pdf(3))
        .unwrap()
        .upload_id
}

/// Whether `upload_id` is still in the caller's staging area.
pub(super) fn still_staged(office: &Office, actor: &str, upload_id: &str) -> bool {
    let probe = office.attachment_upload_commit(
        actor,
        AttachmentUploadRef {
            upload_id: upload_id.into(),
        },
    );
    match probe.unwrap() {
        Ok(_) => true,
        Err(error) => {
            assert_eq!(error.code, ErrorCode::NotFound, "{error:?}");
            false
        }
    }
}

#[test]
fn attachment_only_messages_and_replies_carry_images_and_documents_on_both_engines() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let image = png(office, &fx.agent, "chart.png");
        let report = pdf(office, &fx.agent, "report.pdf");
        let (shared_before, _) = office.attachment_blob_counts().unwrap();
        let sent = send(office, &fx.agent, &fx.person, "", &[&image, &report]).unwrap();
        assert!(sent.inserted);
        // Promoted into the shared blob store, then gone from staging.
        assert_eq!(
            office.attachment_blob_counts().unwrap().0,
            shared_before + 2
        );
        assert!(!still_staged(office, &fx.agent, &image));
        assert!(!still_staged(office, &fx.agent, &report));

        let conversation = sent.result.conversation_id.clone();
        let views = messages(office, &fx.person, &conversation);
        let view = views.iter().find(|m| m.id == sent.result.id).unwrap();
        assert_eq!(view.body.as_deref(), Some(""), "{engine:?}");
        assert_eq!(view.attachment_count, 2);
        let [first, second] = view.attachments.as_slice() else {
            panic!("two files: {:?}", view.attachments);
        };
        assert_eq!(
            (
                first.filename.as_str(),
                first.title.as_str(),
                first.mime_type.as_str()
            ),
            ("chart.png", "chart.png", "image/png")
        );
        assert_eq!((first.width, first.height), (Some(8), Some(6)));
        assert_eq!(second.mime_type, "application/pdf");
        assert_eq!(second.page_count, Some(3));
        for file in [first, second] {
            assert_eq!(file.uploaded_by_employee_id, fx.agent);
            assert_eq!(file.created_at, view.created_at);
            assert!(org_proto::is_uuid_v7(&file.id), "{}", file.id);
        }
        let bytes = read_all(office, &fx.person, &second.id, 1024).unwrap();
        assert_eq!(bytes, fixtures::pdf(3));
        let shown = inspect(office, &fx.person, &first.id).unwrap();
        assert_eq!(shown.target_kind, AttachmentTargetKind::Message);
        assert_eq!(shown.target_id.as_deref(), Some(sent.result.id.as_str()));

        // An attachment-only reply, and a reply with text and a document.
        let notes = upload(
            office,
            &fx.person,
            "notes.md",
            "text/markdown",
            b"# Notes\n",
        )
        .unwrap();
        let reply = office
            .chat_reply(
                &fx.person,
                MsgReplyParams {
                    id: next_uuid(),
                    to_id: sent.result.id.clone(),
                    priority: None,
                    subject: None,
                    body: String::new(),
                    meta: Default::default(),
                    attachment_ids: vec![notes.upload_id],
                },
            )
            .unwrap()
            .unwrap();
        let text_only = send(office, &fx.person, &fx.agent, "thanks", &[]).unwrap();

        let ids = |views: Vec<ChatMessageView>| -> Vec<String> {
            views.into_iter().map(|m| m.id).collect()
        };
        let filtered = |has_image, has_file| {
            ids(read_messages(
                office,
                &fx.agent,
                ChatMessagesParams {
                    conversation: Some(conversation.clone()),
                    has_image,
                    has_file,
                    ..Default::default()
                },
            ))
        };
        assert_eq!(filtered(Some(true), None), vec![sent.result.id.clone()]);
        assert_eq!(
            filtered(None, Some(true)),
            vec![sent.result.id.clone(), reply.result.id.clone()]
        );
        assert_eq!(
            filtered(Some(false), Some(true)),
            vec![reply.result.id.clone()]
        );
        assert_eq!(
            filtered(Some(false), Some(false)),
            vec![text_only.result.id.clone()]
        );

        // The inbox shows the files and filters the same way.
        let inbox = |actor: &str, has_image, has_file| {
            office
                .chat_inbox(
                    actor,
                    ChatInboxParams {
                        include_all: true,
                        has_image,
                        has_file,
                        ..Default::default()
                    },
                )
                .unwrap()
                .unwrap()
                .items
        };
        let items = inbox(&fx.person, Some(true), None);
        assert_eq!(items.len(), 1, "{engine:?}");
        assert_eq!(items[0].attachments, view.attachments);
        assert!(!inbox(&fx.person, None, None).is_empty());
        let agent_files = inbox(&fx.agent, None, Some(true));
        assert_eq!(agent_files.len(), 1);
        assert_eq!(agent_files[0].id, reply.result.id);
        assert_eq!(agent_files[0].attachments[0].mime_type, "text/markdown");
        assert!(inbox(&fx.agent, Some(true), None).is_empty());
    }
}

#[test]
fn a_message_says_something_and_carries_at_most_ten_distinct_files() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let code = |result: Result<ChatSendCommitted, ApiError>| result.unwrap_err().code;
        assert_eq!(
            code(send(office, &fx.agent, &fx.person, "", &[])),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(send(office, &fx.agent, &fx.person, " \n\t", &[])),
            ErrorCode::InvalidRequest
        );
        let uploads: Vec<String> = (0..11)
            .map(|n| {
                upload(office, &fx.agent, &format!("f{n}.txt"), "text/plain", b"x")
                    .unwrap()
                    .upload_id
            })
            .collect();
        let eleven: Vec<&str> = uploads.iter().map(String::as_str).collect();
        let (shared_before, _) = office.attachment_blob_counts().unwrap();
        assert_eq!(
            code(send(office, &fx.agent, &fx.person, "too many", &eleven)),
            ErrorCode::TooLarge
        );
        assert_eq!(
            code(send(
                office,
                &fx.agent,
                &fx.person,
                "twice",
                &[eleven[0], eleven[0]]
            )),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(send(
                office,
                &fx.agent,
                &fx.person,
                "not an id",
                &["upload-1"]
            )),
            ErrorCode::InvalidRequest
        );
        let mut legacy = send_params(&fx.person, "legacy id", &[eleven[0]]);
        legacy.id = "01JDQXR7Y6H0Q7E5B8W4N2M3KA".into();
        assert_eq!(
            office
                .chat_send(&fx.agent, legacy)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        // Nothing moved; every upload is still staged; ten is the cap, not an error.
        assert_eq!(office.attachment_blob_counts().unwrap().0, shared_before);
        assert!(eleven.iter().all(|id| still_staged(office, &fx.agent, id)));
        let sent = send(office, &fx.agent, &fx.person, "", &eleven[..10]).unwrap();
        let views = messages(office, &fx.agent, &sent.result.conversation_id);
        assert_eq!(views[0].attachments.len(), 10);
        let names: Vec<&str> = views[0]
            .attachments
            .iter()
            .map(|a| a.filename.as_str())
            .collect();
        let expected: Vec<String> = (0..10).map(|n| format!("f{n}.txt")).collect();
        assert_eq!(names, expected, "files keep the order they were named in");
        assert!(still_staged(office, &fx.agent, eleven[10]));
    }
}

#[test]
fn someone_elses_staged_upload_is_not_found_and_stays_theirs() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let theirs = pdf(office, &fx.agent, "plan.pdf");
        let mine = png(office, &fx.person, "mine.png");
        let (shared_before, _) = office.attachment_blob_counts().unwrap();
        // A member of the very conversation, the owner, and a stranger alike: not found.
        for (thief, to) in [
            (&fx.person, &fx.agent),
            (&fx.owner, &fx.agent),
            (&fx.other_person, &fx.person),
        ] {
            let refused = send(office, thief, to, "look", &[&theirs]).unwrap_err();
            assert_eq!(refused.code, ErrorCode::NotFound, "{engine:?} {thief}");
        }
        // Mixed with the caller's own upload, nothing is bound and nothing moves.
        let refused = send(office, &fx.person, &fx.agent, "", &[&mine, &theirs]).unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound);
        assert_eq!(office.attachment_blob_counts().unwrap().0, shared_before);
        assert!(still_staged(office, &fx.person, &mine));
        assert!(still_staged(office, &fx.agent, &theirs));
        // The uploader still binds it.
        send(office, &fx.agent, &fx.person, "", &[&theirs]).unwrap();
        assert!(!still_staged(office, &fx.agent, &theirs));
    }
}

#[test]
fn an_exact_retry_with_files_is_answered_once_and_a_changed_one_is_refused() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let image = png(office, &fx.agent, "a.png");
        let report = pdf(office, &fx.agent, "b.pdf");
        let params = send_params(&fx.person, "both", &[&image, &report]);
        let first = office
            .chat_send(&fx.agent, params.clone())
            .unwrap()
            .unwrap();
        assert!(first.inserted);
        // The uploads are gone by now; the retry is still recognized by its bound IDs.
        let again = office
            .chat_send(&fx.agent, params.clone())
            .unwrap()
            .unwrap();
        assert!(!again.inserted, "{engine:?}");
        assert_eq!(again.result.id, first.result.id);
        assert_eq!(again.message.attachments, first.message.attachments);
        assert_eq!(
            first.message.content_sha256,
            first.message.compute_content_sha256()
        );

        let other = png(office, &fx.agent, "c.png");
        for files in [
            vec![],
            vec![image.clone()],
            vec![report.clone(), image.clone()],
            vec![image.clone(), other.clone()],
        ] {
            let mut changed = params.clone();
            changed.attachment_ids = files;
            if changed.attachment_ids.is_empty() {
                changed.body = "both".into();
            }
            let refused = office.chat_send(&fx.agent, changed).unwrap().unwrap_err();
            assert_eq!(refused.code, ErrorCode::AlreadyExists, "{engine:?}");
        }
        assert!(still_staged(office, &fx.agent, &other));
        // Someone else replaying the ID learns nothing about its files.
        let refused = office.chat_send(&fx.person, params).unwrap().unwrap_err();
        assert_eq!(refused.code, ErrorCode::AlreadyExists);
        let views = messages(office, &fx.agent, &first.result.conversation_id);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].attachments.len(), 2);
    }
}

#[test]
fn a_direct_message_deliverable_is_read_by_its_two_members_and_hidden_from_outsiders() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let report = pdf(office, &fx.agent, "deliverable.pdf");
        let sent = send(
            office,
            &fx.agent,
            &fx.person,
            "Here is the report",
            &[&report],
        )
        .unwrap();
        let file = sent.message.attachments[0].id.clone();
        for reader in [&fx.agent, &fx.person, &fx.owner] {
            assert_eq!(
                read_first_chunk(office, reader, &file).unwrap(),
                fixtures::pdf(3).len() as u64
            );
            inspect(office, reader, &file).unwrap();
        }
        for outsider in [&fx.other_person, &fx.outsider_agent] {
            assert_eq!(
                inspect(office, outsider, &file).unwrap_err().code,
                ErrorCode::NotFound,
                "{engine:?}"
            );
            assert_eq!(
                read_first_chunk(office, outsider, &file).unwrap_err().code,
                ErrorCode::NotFound
            );
            // Nor can they carry it elsewhere.
            let refused = reattach(
                office,
                outsider,
                &file,
                AttachmentTargetKind::Organization,
                None,
            );
            assert_eq!(refused.unwrap_err().code, ErrorCode::NotFound);
        }
        // A message file is changed only with its message: never edited in place.
        let mut edit = org_proto::attachments::AttachmentUpdateMetaParams {
            command_id: next_uuid(),
            attachment_id: file.clone(),
            title: Some("Renamed".into()),
            description: None,
            artifact_status: None,
        };
        let refused = office
            .attachment_update_meta(&fx.agent, edit.clone())
            .unwrap();
        assert_eq!(refused.unwrap_err().code, ErrorCode::Unsupported);
        edit.command_id = next_uuid();
        let refused = office.attachment_update_meta(&fx.person, edit).unwrap();
        assert_eq!(refused.unwrap_err().code, ErrorCode::NotAuthorized);
    }
}

#[test]
fn a_channel_file_is_every_employees_to_read_but_only_members_post_files() {
    for engine in ENGINES {
        let fx = Fixture::new(engine);
        let office = &fx.office;
        let image = png(office, &fx.agent, "board.png");
        let sent = send(office, &fx.agent, "#general", "", &[&image]).unwrap();
        let file = sent.message.attachments[0].id.clone();
        for reader in [&fx.owner, &fx.person, &fx.other_person, &fx.outsider_agent] {
            inspect(office, reader, &file).unwrap();
        }
        let views = messages(office, &fx.other_person, &sent.result.conversation_id);
        assert_eq!(views.last().unwrap().attachments.len(), 1, "{engine:?}");
    }
}
