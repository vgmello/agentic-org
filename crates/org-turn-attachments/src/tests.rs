//! Unit tests for the decoupled `org-turn-attachments` crate (zero `org-office` dependency).

use org_agent_runtime::ImageInputMode;
use org_proto::{ApiError, ErrorCode};

use crate::{
    is_provider_image_rejection_error, model_supports_vision, prepare_turn_attachments,
    purge_turn_dir, PrepareTurnAttachmentsRequest, TurnAttachmentMeta, TurnAttachmentResolveError,
    TurnAttachmentResolver, TurnDeliveryAttachment, TurnDeliveryScope, TurnImageDerivative,
    MAX_ATTACHMENTS_PER_TURN, MAX_DOC_STAGING_BYTES_PER_TURN, UNTRUSTED_ATTACHMENTS_CLOSE,
    UNTRUSTED_ATTACHMENTS_OPEN,
};

struct FakeResolver;

impl TurnAttachmentResolver for FakeResolver {
    fn resolve_for_turn(
        &self,
        _employee_id: &str,
        _delivery_scope: &TurnDeliveryScope,
        attachment_id: &str,
        remaining_staging_budget: u64,
    ) -> Result<Result<TurnDeliveryAttachment, ApiError>, TurnAttachmentResolveError> {
        match attachment_id {
            "img-1" => Ok(Ok(TurnDeliveryAttachment::Image {
                record: TurnAttachmentMeta {
                    id: "img-1".into(),
                    filename: "shot.png".into(),
                    title: "Screenshot".into(),
                    mime_type: "image/png".into(),
                    byte_count: 16,
                    description: Some("ui capture".into()),
                    page_count: None,
                },
                derivative: TurnImageDerivative {
                    mime_type: "image/png".into(),
                    bytes: vec![0x89, b'P', b'N', b'G'],
                    width: 2,
                    height: 2,
                },
            })),
            "doc-1" => {
                let payload = b"hello world".to_vec();
                let bytes = (payload.len() as u64 <= remaining_staging_budget).then_some(payload);
                Ok(Ok(TurnDeliveryAttachment::Document {
                    record: TurnAttachmentMeta {
                        id: "doc-1".into(),
                        filename: "notes.txt".into(),
                        title: "Notes".into(),
                        mime_type: "text/plain".into(),
                        byte_count: 11,
                        description: None,
                        page_count: Some(1),
                    },
                    bytes,
                }))
            }
            "denied" => Ok(Err(ApiError::new(ErrorCode::NotAuthorized, "denied"))),
            _ => Err(TurnAttachmentResolveError::new("storage down")),
        }
    }
}

#[test]
fn prepares_decoupled_image_document_and_error_entries_and_purges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("attachments");
    let ids = vec![
        "img-1".to_owned(),
        "doc-1".to_owned(),
        "denied".to_owned(),
        "broken".to_owned(),
    ];
    let prepared = prepare_turn_attachments(PrepareTurnAttachmentsRequest {
        employee_id: "emp-1",
        delivery_scope: &TurnDeliveryScope::None,
        turn_id: "turn-1",
        base_text: "Check files".into(),
        attachment_ids: &ids,
        image_input_mode: ImageInputMode::InlineBase64,
        allow_protocol_images: true,
        model_supports_vision: true,
        force_text_only: false,
        remaining_staging_budget: MAX_DOC_STAGING_BYTES_PER_TURN,
        remaining_attachment_slots: MAX_ATTACHMENTS_PER_TURN,
        attachments_root: &root,
        resolver: &FakeResolver,
    });
    assert!(prepared.had_protocol_images);
    assert_eq!(prepared.images.len(), 1);
    assert_eq!(prepared.staged_turn_id.as_deref(), Some("turn-1"));
    assert!(prepared.text.contains(UNTRUSTED_ATTACHMENTS_OPEN));
    assert!(prepared.text.contains(UNTRUSTED_ATTACHMENTS_CLOSE));
    assert!(prepared.text.contains("kind=\"image\""));
    assert!(prepared.text.contains("kind=\"document\""));
    assert!(prepared.text.contains("page_count=1"));
    assert!(prepared.text.contains("note=\"denied\""));
    assert!(prepared
        .text
        .contains("note=\"attachment could not be loaded right now\""));

    // Debug never leaks raw bytes.
    let deriv = TurnImageDerivative {
        mime_type: "image/png".into(),
        bytes: b"SECRET_IMAGE_BYTES".to_vec(),
        width: 1,
        height: 1,
    };
    assert!(!format!("{deriv:?}").contains("SECRET_IMAGE_BYTES"));

    purge_turn_dir(&root, "turn-1");
    assert!(!root.join("turn-1").exists());
}

#[test]
fn capability_and_image_rejection_helpers_work_independently() {
    assert!(model_supports_vision("{}"));
    assert!(!model_supports_vision(r#"{"vision": false}"#));
    assert!(!model_supports_vision(r#"{"input_modalities": ["text"]}"#));
    assert!(model_supports_vision(
        r#"{"input_modalities": ["text", "image"]}"#
    ));
    assert!(is_provider_image_rejection_error(
        &["model does not support vision".into()],
        None
    ));
    assert!(!is_provider_image_rejection_error(
        &["rate limited".into()],
        None
    ));
}
