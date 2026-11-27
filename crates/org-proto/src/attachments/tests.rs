use super::*;

#[test]
fn a_full_chunk_fits_a_frame_and_its_base64_bound_is_exact() {
    let chunk = ATTACHMENT_CHUNK_MAX_BYTES as usize;
    assert_eq!(chunk.div_ceil(3) * 4, ATTACHMENT_CHUNK_MAX_BASE64_CHARS);
    // The envelope around one chunk is small; the base64 text dominates and stays far
    // below the 1 MiB frame cap.
    const { assert!(ATTACHMENT_CHUNK_MAX_BASE64_CHARS + 16 * 1024 < crate::MAX_FRAME_BYTES) };
}

#[test]
fn target_kinds_and_statuses_round_trip_their_stored_names() {
    for kind in [
        AttachmentTargetKind::Task,
        AttachmentTargetKind::TaskComment,
        AttachmentTargetKind::ReviewFinding,
        AttachmentTargetKind::ReviewFindingReply,
        AttachmentTargetKind::Project,
        AttachmentTargetKind::Organization,
        AttachmentTargetKind::Message,
        AttachmentTargetKind::AskAnswer,
        AttachmentTargetKind::UserPrivate,
    ] {
        assert_eq!(AttachmentTargetKind::parse(kind.as_str()), Some(kind));
        let wire = serde_json::to_value(kind).unwrap();
        assert_eq!(wire, serde_json::json!(kind.as_str()));
    }
    assert_eq!(AttachmentTargetKind::parse("Project"), None);
    assert_eq!(ArtifactStatus::default(), ArtifactStatus::Final);
    for status in ["draft", "review", "final", "superseded", "archived"] {
        assert_eq!(ArtifactStatus::parse(status).unwrap().as_str(), status);
    }
    assert_eq!(ArtifactStatus::parse("FINAL"), None);
}

#[test]
fn debug_never_prints_chunk_bytes() {
    let chunk = AttachmentUploadChunkParams {
        upload_id: "u".into(),
        offset: 0,
        data_base64: "U0VDUkVUX0JZVEVT".into(),
    };
    let text = format!("{chunk:?}");
    assert!(!text.contains("U0VDUkVUX0JZVEVT"), "{text}");
    let read = AttachmentReadChunkResult {
        attachment_id: "a".into(),
        offset: 0,
        data_base64: "U0VDUkVUX0JZVEVT".into(),
        eof: true,
        total_bytes: 12,
        content_sha256: "0".repeat(64),
    };
    assert!(!format!("{read:?}").contains("U0VDUkVUX0JZVEVT"));
}

#[test]
fn every_method_is_unique_and_in_the_attachment_family() {
    let mut seen = std::collections::BTreeSet::new();
    for method in ATTACHMENT_METHODS {
        assert!(method.starts_with("attachment."), "{method}");
        assert!(seen.insert(method), "{method} twice");
    }
}

#[test]
fn optional_fields_are_omitted_and_unknown_fields_are_ignored() {
    let params: AttachmentAttachParams = serde_json::from_value(serde_json::json!({
        "command_id": "0193a5c0-0000-7000-8000-000000000001",
        "upload_id": "0193a5c0-0000-7000-8000-000000000002",
        "target_kind": "organization",
        "future_field": true,
    }))
    .unwrap();
    assert_eq!(params.target_id, None);
    assert_eq!(params.artifact_status, None);
    let wire = serde_json::to_value(&params).unwrap();
    assert!(wire.get("target_id").is_none());
    assert!(wire.get("source_attachment_id").is_none());
}

#[test]
fn message_files_are_capped_distinct_uuidv7_and_text_or_files_is_required() {
    let id = |n: u32| format!("0190f000-0000-7000-8000-{n:012x}");
    let ten: Vec<String> = (0..10).map(id).collect();
    check_attachment_ids(&ten).unwrap();
    check_attachment_ids(&[]).unwrap();
    let mut eleven = ten.clone();
    eleven.push(id(10));
    assert_eq!(
        check_attachment_ids(&eleven).unwrap_err().code,
        crate::ErrorCode::TooLarge
    );
    for bad in [
        vec![id(1), id(1)],
        vec!["not-an-id".to_owned()],
        vec![id(1).to_uppercase()],
    ] {
        assert_eq!(
            check_attachment_ids(&bad).unwrap_err().code,
            crate::ErrorCode::InvalidRequest
        );
    }
    assert!(has_content("hi", &[]));
    assert!(has_content("", &ten[..1]));
    assert!(!has_content(" \n\t", &[]));
}

#[test]
fn has_image_and_has_file_tell_images_from_documents() {
    let all = AttachmentFilter::default();
    assert!(all.is_none() && all.admits([]) && all.admits(["image/png"]));
    let images = AttachmentFilter::new(Some(true), None);
    assert!(images.admits(["text/csv", "image/webp"]));
    assert!(!images.admits(["application/pdf"]));
    assert!(!images.admits([]));
    let no_images = AttachmentFilter::new(Some(false), None);
    assert!(no_images.admits([]) && no_images.admits(["text/markdown"]));
    let documents = AttachmentFilter::new(None, Some(true));
    assert!(documents.admits(["application/pdf"]) && !documents.admits(["image/gif"]));
    let plain = AttachmentFilter::new(Some(false), Some(false));
    assert!(plain.admits([]) && !plain.admits(["text/plain"]));
    assert!(is_image_mime("image/jpeg") && !is_image_mime("application/json"));
}

#[test]
fn file_fields_are_optional_on_the_wire_and_omitted_when_empty() {
    use crate::ask::{AskAnswerRecord, ChatAnswerParams};
    use crate::chat::{ChatInboxParams, ChatMessagesParams, ChatSendParams};
    use crate::inbox::{ActionInput, InboxListParams};
    use crate::msg::{MsgReplyParams, MsgSendParams};
    use serde_json::json;

    // Older clients send no `attachment_ids`; newer ones only when they name files.
    let send: ChatSendParams =
        serde_json::from_value(json!({"id": "x", "to": "y", "body": "hi"})).unwrap();
    assert!(send.attachment_ids.is_empty());
    assert!(serde_json::to_value(&send)
        .unwrap()
        .get("attachment_ids")
        .is_none());
    let mut reply: MsgReplyParams =
        serde_json::from_value(json!({"id": "x", "to_id": "y", "body": ""})).unwrap();
    reply.attachment_ids.push("u".into());
    assert_eq!(
        serde_json::to_value(&reply).unwrap()["attachment_ids"],
        json!(["u"])
    );
    assert!(format!("{reply:?}").contains("attachments: 1"));
    let msg: MsgSendParams =
        serde_json::from_value(json!({"id": "x", "to": "y", "kind": "msg", "body": "b"})).unwrap();
    assert!(msg.attachment_ids.is_empty());
    let answer: ChatAnswerParams = serde_json::from_value(json!({"ask_id": "a"})).unwrap();
    assert!(answer.attachment_ids.is_empty());
    let record: AskAnswerRecord = serde_json::from_value(json!({"by": "e", "at": "t"})).unwrap();
    assert!(serde_json::to_value(&record)
        .unwrap()
        .get("attachments")
        .is_none());

    let filtered: ChatMessagesParams =
        serde_json::from_value(json!({"has_image": true, "has_file": false})).unwrap();
    assert_eq!(
        filtered.attachment_filter(),
        AttachmentFilter::new(Some(true), Some(false))
    );
    let inbox: ChatInboxParams = serde_json::from_value(json!({"has_file": true})).unwrap();
    assert_eq!((inbox.has_image, inbox.has_file), (None, Some(true)));
    let list: InboxListParams = serde_json::from_value(json!({"state": "open"})).unwrap();
    assert_eq!((list.has_image, list.has_file), (None, None));
    let input: ActionInput =
        serde_json::from_value(json!({"field": "body", "required": false, "max_chars": 9}))
            .unwrap();
    assert_eq!(
        (input.attachments_field.as_deref(), input.max_attachments),
        (None, 0)
    );
    assert_eq!(
        serde_json::to_value(&input).unwrap(),
        json!({"field": "body", "required": false, "max_chars": 9})
    );

    let meta: AttachmentMeta = serde_json::from_value(json!({
        "id": "f", "filename": "a.png", "title": "a.png", "mime_type": "image/png",
        "byte_count": 3, "uploaded_by_employee_id": "e", "created_at": "t"
    }))
    .unwrap();
    assert!(meta.is_image());
    assert_eq!(meta.artifact_status, ArtifactStatus::Final);
}
