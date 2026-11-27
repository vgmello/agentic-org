//! End-to-end multimodal image and allowlisted document turn delivery (`TASK-98.3`).
//!
//! Leaf module: imports only `wake` sanitization helpers and workspace domain/runtime crates,
//! introducing zero intra-crate dependency cycles (`vsa_structure`).

mod prepare;
mod staging;

use std::sync::Arc;

use org_office::attachments::DeliveryAttachmentPayload;
use org_office::{Office, OfficeError};
use org_proto::ApiError;
use serde::{Deserialize, Serialize};

pub use prepare::{
    prepare_turn_attachments, PrepareTurnAttachmentsRequest, PreparedTurnInput,
    UNTRUSTED_ATTACHMENTS_CLOSE, UNTRUSTED_ATTACHMENTS_OPEN,
};
pub use staging::{
    attachments_root, ensure_attachments_root, purge_all_stale_turn_dirs,
    purge_office_stale_attachments, purge_turn_dir, stage_turn_file, ATTACHMENTS_DIR_NAME,
};

/// Maximum inline/protocol images delivered in one turn (`TASK-98.3` AC #2 & AC #4).
pub const MAX_INLINE_IMAGES_PER_TURN: usize = 4;
/// Maximum byte length of one model image derivative (1 MiB, `TASK-98.3` AC #2).
pub const MAX_IMAGE_DERIVATIVE_BYTES: usize = 1024 * 1024;
/// Maximum total eager staging budget for allowlisted documents in one turn (50 MiB, AC #6).
pub const MAX_DOC_STAGING_BYTES_PER_TURN: u64 = 50 * 1024 * 1024;

/// Lightweight reference to one bound attachment threaded through wakes, turns, and failovers
/// (`TASK-98.3` AC #4). Never holds raw bytes or base64 data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnAttachmentRef {
    pub attachment_id: String,
}

impl TurnAttachmentRef {
    pub fn new(attachment_id: impl Into<String>) -> Self {
        Self {
            attachment_id: attachment_id.into(),
        }
    }
}

/// Consumer-owned port resolving policy-authorized attachment payloads for a managed turn
/// (`TASK-98.3`, `AGENTS.md` Rule 1).
///
/// HQ uses [`LocalOfficeAttachmentResolver`]; satellites wire a federation-backed implementation
/// in `org-office-server` that pulls missing shared blobs from HQ via `BlobPull` after the
/// office's `AuthorizationPolicy` approves the read.
pub trait TurnAttachmentResolver: Send + Sync {
    fn resolve_for_turn(
        &self,
        employee_id: &str,
        attachment_id: &str,
        remaining_doc_budget: u64,
    ) -> Result<Result<DeliveryAttachmentPayload, ApiError>, OfficeError>;
}

/// Default local-office implementation of [`TurnAttachmentResolver`].
pub struct LocalOfficeAttachmentResolver {
    office: Arc<Office>,
}

impl LocalOfficeAttachmentResolver {
    pub fn new(office: Arc<Office>) -> Self {
        Self { office }
    }
}

impl TurnAttachmentResolver for LocalOfficeAttachmentResolver {
    fn resolve_for_turn(
        &self,
        employee_id: &str,
        attachment_id: &str,
        remaining_doc_budget: u64,
    ) -> Result<Result<DeliveryAttachmentPayload, ApiError>, OfficeError> {
        self.office.attachment_read_for_delivery_with(
            employee_id,
            attachment_id,
            remaining_doc_budget,
            None,
        )
    }
}

/// Determines from discovered `capabilities_json` whether a model supports vision/image inputs.
///
/// Defaults to `true` when unspecified (`"{}"`) so models without explicit capability metadata
/// still receive multimodal inputs; returns `false` when explicitly marked non-vision (e.g.
/// `{"vision": false}`, `{"supports_vision": false}`, `{"supports_images": false}`, or
/// `{"input_modalities": ["text"]}`).
pub fn model_supports_vision(capabilities_json: &str) -> bool {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(capabilities_json) else {
        return true;
    };
    let Some(obj) = val.as_object() else {
        return true;
    };
    for key in [
        "vision",
        "supports_vision",
        "supports_images",
        "image_input",
    ] {
        if let Some(flag) = obj.get(key).and_then(serde_json::Value::as_bool) {
            return flag;
        }
    }
    for key in ["input_modalities", "modalities"] {
        if let Some(arr) = obj.get(key).and_then(serde_json::Value::as_array) {
            if !arr.is_empty() {
                return arr.iter().filter_map(serde_json::Value::as_str).any(|m| {
                    matches!(
                        m.to_ascii_lowercase().as_str(),
                        "image" | "images" | "vision" | "multimodal"
                    )
                });
            }
        }
    }
    true
}

/// Classifies whether a pre-side-effect turn failure indicates the provider/model rejected image
/// inputs, so the pump can degrade fail-soft to a text-only staged-path retry (`TASK-98.3` AC #4).
pub fn is_provider_image_rejection_error(errors: &[String], turn_reply: Option<&str>) -> bool {
    const PATTERNS: &[&str] = &[
        "does not support image",
        "does not support vision",
        "images are not supported",
        "image input is not supported",
        "image inputs are not supported",
        "unsupported image",
        "multimodal is not supported",
        "cannot process image",
        "invalid_image",
        "image_not_supported",
    ];
    let matches_text = |raw: &str| {
        let lower = raw.to_ascii_lowercase();
        PATTERNS.iter().any(|p| lower.contains(p))
    };
    errors.iter().any(|e| matches_text(e)) || turn_reply.is_some_and(matches_text)
}
