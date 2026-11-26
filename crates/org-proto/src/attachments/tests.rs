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
