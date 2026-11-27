//! `msg.send` and `msg.reply` with files through the office's full dispatcher (`TASK-98.2`):
//! a `msg.send` naming files goes as `chat.send` sends it (the bare mailbox binds none), and
//! its files are read with `attachment.*` by the members of its conversation.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_proto::attachments::{
    AttachmentInspect, AttachmentInspectParams, AttachmentReadChunk, AttachmentReadChunkParams,
    AttachmentUploadBeginParams, AttachmentUploadChunkParams, AttachmentUploadRef,
};
use org_proto::chat::{ChatInboxItem, ChatInboxParams};
use org_proto::msg::{MsgReply, MsgReplyParams, MsgSend, MsgSendParams, SendResult, KIND_MSG};
use org_proto::{ApiError, Call};

fn upload_csv(office: &Office, member: &str, rows: &str) -> String {
    let bytes = format!("name,amount\n{rows}\n").into_bytes();
    let begun = office
        .attachment_upload_begin(
            member,
            AttachmentUploadBeginParams {
                filename: "ledger.csv".into(),
                mime_type: "text/csv".into(),
                byte_count: bytes.len() as u64,
            },
        )
        .unwrap()
        .unwrap();
    let id = begun.upload.upload_id;
    office
        .attachment_upload_chunk(
            member,
            AttachmentUploadChunkParams {
                upload_id: id.clone(),
                offset: 0,
                data_base64: STANDARD.encode(&bytes),
            },
        )
        .unwrap()
        .unwrap();
    office
        .attachment_upload_commit(
            member,
            AttachmentUploadRef {
                upload_id: id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    id
}

fn answer<C: Call>(
    fx: &LiveOfficeFixture,
    member: &str,
    via_grant: bool,
    params: &C::Params,
) -> Result<C::Result, ApiError> {
    let request = Request::new::<C>(7, params).unwrap();
    match fx
        .shared
        .answer_for_test(&request, member, via_grant)
        .outcome
    {
        Outcome::Result(value) => Ok(serde_json::from_value(value).unwrap()),
        Outcome::Error(error) => Err(error),
    }
}

fn send_params(id: String, body: &str, files: Vec<String>) -> MsgSendParams {
    MsgSendParams {
        id,
        to: "owner".into(),
        kind: KIND_MSG.into(),
        priority: None,
        subject: None,
        body: body.into(),
        meta: Meta::new(),
        reply_to: None,
        attachment_ids: files,
    }
}

fn files_in_inbox(fx: &LiveOfficeFixture, member: &str) -> Vec<ChatInboxItem> {
    let params = ChatInboxParams {
        include_all: true,
        has_file: Some(true),
        ..Default::default()
    };
    fx.office.chat_inbox(member, params).unwrap().unwrap().items
}

#[test]
fn msg_send_and_reply_with_attachments_go_as_chat_sends_through_the_dispatcher() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fx = LiveOfficeFixture::start();
    let file = upload_csv(&fx.office, &fx.hr_id, "rent,1200");
    let params = send_params(uuidv7(3001), "", vec![file]);
    let sent: SendResult = answer::<MsgSend>(&fx, &fx.hr_id, false, &params).unwrap();
    // The exact retry is answered once, with the same result.
    let again = answer::<MsgSend>(&fx, &fx.hr_id, false, &params).unwrap();
    assert_eq!(again, sent);
    // Without files `msg.send` still goes through the mailbox, which refuses saying nothing.
    let silent = send_params(uuidv7(3002), " \n", Vec::new());
    let refused = answer::<MsgSend>(&fx, &fx.hr_id, false, &silent).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidRequest);
    let spoken = send_params(uuidv7(3003), "plain words", Vec::new());
    answer::<MsgSend>(&fx, &fx.hr_id, false, &spoken).unwrap();

    let inbox = files_in_inbox(&fx, &fx.owner_id);
    assert_eq!(inbox.len(), 1, "only the message with a file: {inbox:?}");
    assert_eq!(inbox[0].id, sent.id);
    let file_id = inbox[0].attachments[0].id.clone();
    assert_eq!(inbox[0].attachments[0].mime_type, "text/csv");
    for (member, via_grant) in [(&fx.owner_id, true), (&fx.hr_id, false)] {
        let inspected = answer::<AttachmentInspect>(
            &fx,
            member,
            via_grant,
            &AttachmentInspectParams {
                attachment_id: file_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(inspected.attachment.filename, "ledger.csv");
        let chunk = answer::<AttachmentReadChunk>(
            &fx,
            member,
            via_grant,
            &AttachmentReadChunkParams {
                attachment_id: file_id.clone(),
                offset: 0,
                max_bytes: None,
            },
        )
        .unwrap();
        assert_eq!(
            STANDARD.decode(chunk.data_base64).unwrap(),
            b"name,amount\nrent,1200\n"
        );
    }

    // The owner replies with a file only, over `msg.reply` on the host channel.
    let reply_file = upload_csv(&fx.office, &fx.owner_id, "paid,1200");
    let reply = MsgReplyParams {
        id: uuidv7(3004),
        to_id: sent.id.clone(),
        priority: None,
        subject: None,
        body: String::new(),
        meta: Meta::new(),
        attachment_ids: vec![reply_file],
    };
    let replied: SendResult = answer::<MsgReply>(&fx, &fx.owner_id, true, &reply).unwrap();
    let hr_inbox = files_in_inbox(&fx, &fx.hr_id);
    let item = hr_inbox.iter().find(|i| i.id == replied.id).unwrap();
    assert_eq!(item.attachments.len(), 1);
    assert_eq!(item.attachments[0].uploaded_by_employee_id, fx.owner_id);
}
