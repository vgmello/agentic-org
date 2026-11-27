//! `TASK-240` H-1 and H-2 against a real office through the production [`ChiefOfStaffGate`]:
//!
//! - acting for the owner she never inspects, reads, lists, edits or re-attaches a file Bob
//!   sent her in his DM with her, nor the file of his answer to her ask there, though she
//!   herself may read both; the owner's own DM files with her still work;
//! - an upload she stages acting for the owner is never bound for Bob's turn or a wake: the
//!   gate refuses any message, ask or answer with files for anyone but the owner, and the
//!   office binds the upload only while her run acts for the member it was staged for.

use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::Office;
use org_proto::ask::{AskQuestion, AskQuestionType, ChatAnswerParams, ChatAskParams};
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentInspect, AttachmentInspectParams, AttachmentList,
    AttachmentListParams, AttachmentListResult, AttachmentReadChunk, AttachmentReadChunkParams,
    AttachmentTargetKind, AttachmentUploadBeginParams, AttachmentUploadChunkParams,
    AttachmentUploadRef, AttachmentView,
};
use org_proto::chat::{ChatMessagesParams, ChatSendParams};
use org_proto::ErrorCode;
use serde_json::json;

use super::{error_code, next_id, refused, result, world, Delegation, World};

/// Stages a committed text upload as `actor`: its upload ID.
fn staged(office: &Office, actor: &str, name: &str, body: &[u8]) -> String {
    let begun = office
        .attachment_upload_begin(
            actor,
            AttachmentUploadBeginParams {
                filename: name.into(),
                mime_type: "text/plain".into(),
                byte_count: body.len() as u64,
            },
        )
        .unwrap()
        .unwrap();
    let upload_id = begun.upload.upload_id;
    office
        .attachment_upload_chunk(
            actor,
            AttachmentUploadChunkParams {
                upload_id: upload_id.clone(),
                offset: 0,
                data_base64: STANDARD.encode(body),
            },
        )
        .unwrap()
        .unwrap();
    office
        .attachment_upload_commit(
            actor,
            AttachmentUploadRef {
                upload_id: upload_id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    upload_id
}

fn send_params(to: &str, files: Vec<String>) -> ChatSendParams {
    ChatSendParams {
        id: next_id(),
        to: to.to_owned(),
        kind: "msg".into(),
        priority: None,
        subject: None,
        body: "see attached".into(),
        attachment_ids: files,
        meta: Default::default(),
        reply_to: None,
        also_in_channel: false,
        reason: None,
    }
}

/// `actor` sends one staged file to `to`: `(message, attachment)`.
fn send_file(office: &Office, actor: &str, to: &str, name: &str, body: &[u8]) -> (String, String) {
    let upload = staged(office, actor, name, body);
    let sent = office
        .chat_send(actor, send_params(to, vec![upload]))
        .unwrap()
        .unwrap();
    (sent.result.id, sent.message.attachments[0].id.clone())
}

pub(super) fn ask_params(to: &str) -> ChatAskParams {
    ChatAskParams {
        id: next_id(),
        to: to.into(),
        body: Some("Which draft?".into()),
        subject: None,
        questions: vec![AskQuestion {
            id: "q1".into(),
            question_type: AskQuestionType::FreeText,
            prompt: "Which draft?".into(),
            options: Vec::new(),
            allow_multiple: false,
            default_value: None,
            optional: false,
        }],
        required: false,
        wait_ms: None,
        hold_ms: None,
        blocks: None,
        approval: false,
        addressed_to: None,
        meta: Default::default(),
        attachment_ids: Vec::new(),
    }
}

/// Tess asks Bob in his DM with her and he answers with a file: `(ask, answer attachment)`.
fn bobs_answer_file(w: &World) -> (String, String) {
    let ask = w
        .office
        .msg_ask(&w.tess, ask_params(&w.bob))
        .unwrap()
        .unwrap();
    let upload = staged(&w.office, &w.bob, "answer.txt", b"bob's private answer");
    let answered = w
        .office
        .chat_answer(
            &w.bob,
            ChatAnswerParams {
                ask_id: ask.result.ask_id.clone(),
                answers: Vec::new(),
                option_id: None,
                text: Some("this one".into()),
                attachment_ids: vec![upload],
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(answered.result.conversation_id, w.bob_dm);
    let file = answered.result.answer.attachments[0].id.clone();
    (ask.result.ask_id, file)
}

/// Every attachment call on `file` (of message or ask `message`, target `kind`).
fn file_calls(file: &str, message: &str, kind: &str) -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("attachment.inspect", json!({"attachment_id": file})),
        (
            "attachment.read_chunk",
            json!({"attachment_id": file, "offset": 0}),
        ),
        (
            "attachment.update_meta",
            json!({"command_id": next_id(), "attachment_id": file, "title": "Mine now"}),
        ),
        (
            "attachment.attach",
            json!({"command_id": next_id(), "source_attachment_id": file, "target_kind": "organization"}),
        ),
        (
            "attachment.list",
            json!({"target_kind": kind, "target_id": message}),
        ),
    ]
}

fn inspect_params(file: &str) -> AttachmentInspectParams {
    AttachmentInspectParams {
        attachment_id: file.to_owned(),
    }
}

#[test]
fn acting_for_the_owner_she_never_touches_a_file_another_member_sent_her() {
    let w = world();
    let for_owner = w.acting_for(&w.owner);
    let (bob_msg, bob_file) = send_file(&w.office, &w.bob, &w.tess, "worry.txt", b"BOB-SECRET");
    let (bob_ask, bob_answer) = bobs_answer_file(&w);
    let (owner_msg, owner_file) =
        send_file(&w.office, &w.owner, &w.tess, "plan.txt", b"OWNER-PLAN");
    // Tess herself is in Bob's DM, so the office lets her read both of his files.
    for file in [&bob_file, &bob_answer] {
        let own = w.office.attachment_inspect(&w.tess, inspect_params(file));
        assert!(own.unwrap().is_ok(), "{file}");
    }
    assert_eq!(
        w.office.attachment_conversation_id(&bob_answer).unwrap(),
        Some(w.bob_dm.clone())
    );

    // Acting for the owner, every call on them is refused before it runs.
    for (file, message, kind) in [
        (&bob_file, &bob_msg, "message"),
        (&bob_answer, &bob_ask, "ask_answer"),
    ] {
        for (method, params) in file_calls(file, message, kind) {
            assert!(
                refused(&w.admit(method, params, &for_owner)),
                "{method} on Bob's {kind} file for the owner"
            );
        }
        let read = w.through_gate::<AttachmentReadChunk>(
            AttachmentReadChunkParams {
                attachment_id: file.to_string(),
                offset: 0,
                max_bytes: None,
            },
            &for_owner,
            |_, _, _| panic!("read_chunk ran on Bob's file for the owner"),
        );
        assert_eq!(error_code(&read), Some(ErrorCode::NotAuthorized));
    }
    // Defense in depth: an answer that names one of his files is narrowed away.
    let inspected = w.narrowed::<AttachmentInspect>(
        inspect_params(&bob_file),
        &for_owner,
        |office, tess, p| office.attachment_inspect(tess, p).unwrap().unwrap(),
    );
    assert_eq!(error_code(&inspected), Some(ErrorCode::NotFound));
    let view = |file: &str| -> AttachmentView {
        let found = w.office.attachment_inspect(&w.tess, inspect_params(file));
        found.unwrap().unwrap().attachment
    };
    let all = AttachmentListParams {
        target_kind: Some(AttachmentTargetKind::Organization),
        target_id: None,
        scope: None,
        limit: None,
        cursor: None,
    };
    let listed = w.narrowed::<AttachmentList>(all, &for_owner, |_, _, _| AttachmentListResult {
        attachments: vec![view(&bob_file), view(&owner_file), view(&bob_answer)],
        next_cursor: None,
    });
    let ids: Vec<&str> = result(&listed)["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [owner_file.as_str()]);

    // The owner's own DM file with her: every check passes, and it reads.
    for (method, params) in file_calls(&owner_file, &owner_msg, "message") {
        assert!(
            w.admit(method, params, &for_owner).is_ok(),
            "{method} on the owner's own file"
        );
    }
    let read = w.through_gate::<AttachmentReadChunk>(
        AttachmentReadChunkParams {
            attachment_id: owner_file.clone(),
            offset: 0,
            max_bytes: None,
        },
        &for_owner,
        |office, tess, p| office.attachment_read_chunk(tess, p).unwrap(),
    );
    let bytes = STANDARD
        .decode(result(&read)["data_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(bytes, b"OWNER-PLAN");
    let inspected = w.through_gate::<AttachmentInspect>(
        inspect_params(&owner_file),
        &for_owner,
        |office, tess, p| office.attachment_inspect(tess, p).unwrap(),
    );
    assert_eq!(result(&inspected)["attachment"]["id"], owner_file.as_str());
    // A message or ask named as a list's target must be one the caller sees.
    assert!(refused(&w.admit(
        "attachment.list",
        json!({"target_kind": "message"}),
        &for_owner
    )));
    assert!(refused(&w.admit(
        "attachment.list",
        json!({"target_kind": "message", "target_id": next_id()}),
        &for_owner
    )));
}

/// Tess's run acting for `caller` (or nobody) as the office reads it.
fn acting_as(w: &World) -> Arc<Mutex<Option<String>>> {
    let acting = Arc::new(Mutex::new(None::<String>));
    let (read, tess) = (Arc::clone(&acting), w.tess.clone());
    w.office.set_attachment_acting_for(Box::new(move |member| {
        (member == tess)
            .then(|| read.lock().unwrap().clone())
            .flatten()
    }));
    acting
}

/// How many messages `reader` sees in `conversation`.
pub(super) fn messages_in(w: &World, reader: &str, conversation: &str) -> usize {
    let params = ChatMessagesParams {
        conversation: Some(conversation.to_owned()),
        ..Default::default()
    };
    let history = w.office.chat_messages(reader, params).unwrap().unwrap();
    history.messages.len()
}

#[test]
fn an_upload_staged_for_the_owner_is_never_bound_for_another_member_or_a_wake() {
    let w = world();
    let acting = acting_as(&w);
    *acting.lock().unwrap() = Some(w.owner.clone());
    let upload = staged(&w.office, &w.tess, "board.txt", b"OWNER-BOARD-NOTES");
    let meta = std::fs::read_to_string(
        w.office
            .attachment_staging_root()
            .join(&upload)
            .join("meta.json"),
    )
    .unwrap();
    assert!(
        meta.contains(&format!("\"acting_for_caller_id\":\"{}\"", w.owner)),
        "{meta}"
    );
    // An ordinary member's upload names nobody.
    let bobs = staged(&w.office, &w.bob, "b.txt", b"b");
    let bob_meta = w
        .office
        .attachment_staging_root()
        .join(&bobs)
        .join("meta.json");
    let bob_meta = std::fs::read_to_string(bob_meta).unwrap();
    assert!(!bob_meta.contains("acting_for_caller_id"), "{bob_meta}");

    // The gate: no message, ask, reply or answer with files for Bob or a wake.
    let files = json!([upload]);
    let calls = |to: &str| {
        [
            (
                "chat.send",
                json!({"id": next_id(), "to": to, "body": "x", "attachment_ids": files}),
            ),
            (
                "msg.send",
                json!({"id": next_id(), "to": to, "kind": "msg", "body": "x", "attachment_ids": files}),
            ),
            (
                "msg.reply",
                json!({"id": next_id(), "to_id": w.bob_message, "body": "x", "attachment_ids": files}),
            ),
            ("chat.ask", {
                let mut ask = serde_json::to_value(ask_params(to)).unwrap();
                ask["attachment_ids"] = files.clone();
                ask
            }),
            (
                "chat.answer",
                json!({"ask_id": w.bob_message, "text": "x", "attachment_ids": files}),
            ),
        ]
    };
    for delegation in [w.acting_for(&w.bob), Delegation::default()] {
        for (method, params) in calls(&w.bob) {
            assert!(
                refused(&w.admit(method, params, &delegation)),
                "{method} with the owner's upload for {:?}",
                delegation.caller_id
            );
        }
    }
    // Without files the same send for Bob is admitted.
    let plain = json!({"id": next_id(), "to": w.bob, "body": "x"});
    assert!(w.admit("chat.send", plain, &w.acting_for(&w.bob)).is_ok());

    // The office: while her run acts for Bob or for nobody, the upload is not hers to bind.
    let before = messages_in(&w, &w.bob, &w.bob_dm);
    for now in [Some(w.bob.clone()), None] {
        *acting.lock().unwrap() = now.clone();
        let sent = w
            .office
            .chat_send(&w.tess, send_params(&w.bob, vec![upload.clone()]))
            .unwrap();
        assert_eq!(sent.unwrap_err().code, ErrorCode::NotFound, "{now:?}");
        let attached = w.office.attachment_attach(
            &w.tess,
            AttachmentAttachParams {
                command_id: next_id(),
                upload_id: Some(upload.clone()),
                source_attachment_id: None,
                target_kind: AttachmentTargetKind::Organization,
                target_id: None,
                title: None,
                description: None,
                artifact_status: None,
            },
        );
        assert_eq!(attached.unwrap().unwrap_err().code, ErrorCode::NotFound);
    }
    assert_eq!(
        messages_in(&w, &w.bob, &w.bob_dm),
        before,
        "nothing reached Bob"
    );

    // Back on the owner's turn, it binds into the owner's DM with her.
    *acting.lock().unwrap() = Some(w.owner.clone());
    let to_owner = send_params(&w.owner, vec![upload.clone()]);
    let request = serde_json::to_value(&to_owner).unwrap();
    assert!(w
        .admit("chat.send", request, &w.acting_for(&w.owner))
        .is_ok());
    let sent = w.office.chat_send(&w.tess, to_owner).unwrap().unwrap();
    assert_eq!(sent.result.conversation_id, w.owner_dm);
    assert_eq!(sent.message.attachments.len(), 1);
}

#[test]
fn legacy_tess_upload_without_acting_for_fails_closed_even_during_wake() {
    let w = world();
    let acting = acting_as(&w);
    // Starting an upload as Tess while her run acts for nobody (wake) fails closed.
    let wake_begin = w
        .office
        .attachment_upload_begin(
            &w.tess,
            AttachmentUploadBeginParams {
                filename: "wake.txt".into(),
                mime_type: "text/plain".into(),
                byte_count: 4,
            },
        )
        .unwrap();
    assert_eq!(wake_begin.unwrap_err().code, ErrorCode::NotAuthorized);

    // Stage for Owner, then strip `acting_for_caller_id` from meta.json (legacy/tampered).
    *acting.lock().unwrap() = Some(w.owner.clone());
    let upload = staged(&w.office, &w.tess, "legacy.txt", b"LEGACY-TESS-FILE");
    let meta_path = w
        .office
        .attachment_staging_root()
        .join(&upload)
        .join("meta.json");
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    meta.as_object_mut().unwrap().remove("acting_for_caller_id");
    std::fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();

    for now in [Some(w.owner.clone()), Some(w.bob.clone()), None] {
        *acting.lock().unwrap() = now.clone();
        let sent = w
            .office
            .chat_send(&w.tess, send_params(&w.owner, vec![upload.clone()]))
            .unwrap();
        assert_eq!(sent.unwrap_err().code, ErrorCode::NotFound, "{now:?}");
        let attached = w.office.attachment_attach(
            &w.tess,
            AttachmentAttachParams {
                command_id: next_id(),
                upload_id: Some(upload.clone()),
                source_attachment_id: None,
                target_kind: AttachmentTargetKind::Organization,
                target_id: None,
                title: None,
                description: None,
                artifact_status: None,
            },
        );
        assert_eq!(
            attached.unwrap().unwrap_err().code,
            ErrorCode::NotFound,
            "{now:?}"
        );
    }
}
