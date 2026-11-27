//! End-to-end multimodal image and allowlisted document turn delivery (`TASK-98.3`, `TASK-255`).
//!
//! Cohesive domain crate decoupled from `org-office`: owns turn-scoped `0400` attachment staging,
//! dual-delivery normalization, per-turn budgets, and consumer-owned attachment delivery DTOs,
//! introducing zero intra-crate dependency cycles (`vsa_structure`).

#![forbid(unsafe_code)]

mod prepare;
mod staging;
#[cfg(test)]
mod tests;

use std::fmt;

use org_proto::ApiError;
use serde::{Deserialize, Serialize};

pub use org_redact::sanitize;
pub use prepare::{
    prepare_turn_attachments, PrepareTurnAttachmentsRequest, PreparedTurnInput,
    UNTRUSTED_ATTACHMENTS_CLOSE, UNTRUSTED_ATTACHMENTS_OPEN,
};
pub use staging::{
    attachments_root, ensure_attachments_root, purge_all_stale_turn_dirs,
    purge_office_stale_attachments, purge_turn_dir, stage_turn_file, tighten_turn_dir,
    ATTACHMENTS_DIR_NAME,
};

/// Maximum inline/protocol images delivered in one turn (`TASK-98.3` AC #2 & AC #4).
pub const MAX_INLINE_IMAGES_PER_TURN: usize = 4;
/// Maximum total attachments resolved/decoded/staged in one turn (`TASK-98.3` DoS guardrail).
pub const MAX_ATTACHMENTS_PER_TURN: usize = 32;
/// Maximum byte length of one model image derivative (1 MiB, `TASK-98.3` AC #2).
pub const MAX_IMAGE_DERIVATIVE_BYTES: usize = 1024 * 1024;
/// Maximum total eager staging budget across all files in one turn (50 MiB, AC #6).
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

/// Chief of Staff turn partition scope passed to [`TurnAttachmentResolver::resolve_for_turn`]
/// (`TASK-80.1`, `TASK-240`, `TASK-98.3`).
///
/// Ordinary agents ignore this field (standard authorization applies). For the built-in Chief of
/// Staff:
/// - [`Self::None`] (`tess:wake`): channels only; refuses any DM/GroupDm message or answer file.
/// - [`Self::ActingForPerson`] (`tess:ask:<caller_id>`): enforces caller visibility and
///   conversation membership.
/// - [`Self::DirectSender`] (`tess:dm:<sender_id>`): enforces direct sender participation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TurnDeliveryScope {
    #[default]
    None,
    ActingForPerson(String),
    DirectSender(String),
}

/// Attachment metadata needed to stage files and render the untrusted attachments manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnAttachmentMeta {
    pub id: String,
    pub filename: String,
    pub title: String,
    pub mime_type: String,
    pub byte_count: u64,
    pub description: Option<String>,
    pub page_count: Option<u32>,
}

/// Bounded model image derivative ready for inline or staged-path turn delivery.
///
/// `Debug` never prints raw image bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct TurnImageDerivative {
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl fmt::Debug for TurnImageDerivative {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TurnImageDerivative")
            .field("mime_type", &self.mime_type)
            .field("bytes_len", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

/// Policy-authorized attachment payload ready for model turn delivery (`TASK-98.3`).
///
/// `Debug` never prints raw image or document bytes.
#[derive(Clone)]
pub enum TurnDeliveryAttachment {
    /// Allowlisted image with its bounded model derivative (`<= 1568px`, `<= 1 MiB`).
    Image {
        record: TurnAttachmentMeta,
        derivative: TurnImageDerivative,
    },
    /// Allowlisted document (`PDF`, `DOCX`, `XLSX`, `MD`, `TXT`, `CSV`, `JSON` <= 25 MiB).
    /// `bytes` is `None` when staging this document would exceed the caller's remaining per-turn
    /// eager staging budget.
    Document {
        record: TurnAttachmentMeta,
        bytes: Option<Vec<u8>>,
    },
}

impl fmt::Debug for TurnDeliveryAttachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Image { record, derivative } => f
                .debug_struct("TurnDeliveryAttachment::Image")
                .field("id", &record.id)
                .field("mime_type", &derivative.mime_type)
                .field("derivative_bytes", &derivative.bytes.len())
                .finish(),
            Self::Document { record, bytes } => f
                .debug_struct("TurnDeliveryAttachment::Document")
                .field("id", &record.id)
                .field("mime_type", &record.mime_type)
                .field("staged_bytes", &bytes.as_ref().map(Vec::len))
                .finish(),
        }
    }
}

/// Infrastructure/storage error raised by a [`TurnAttachmentResolver`] implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnAttachmentResolveError(String);

impl TurnAttachmentResolveError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TurnAttachmentResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TurnAttachmentResolveError {}

/// Consumer-owned port resolving policy-authorized attachment payloads for a managed turn
/// (`TASK-98.3`, `AGENTS.md` Rule 1).
///
/// HQ uses `org_office_sessions::LocalOfficeAttachmentResolver`; satellites wire a
/// federation-backed implementation in `org-office-server` that pulls missing shared blobs from
/// HQ via `BlobPull` after the office's `AuthorizationPolicy` approves the read.
pub trait TurnAttachmentResolver: Send + Sync {
    fn resolve_for_turn(
        &self,
        employee_id: &str,
        delivery_scope: &TurnDeliveryScope,
        attachment_id: &str,
        remaining_staging_budget: u64,
    ) -> Result<Result<TurnDeliveryAttachment, ApiError>, TurnAttachmentResolveError>;
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
