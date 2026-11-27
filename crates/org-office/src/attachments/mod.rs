//! Attachments (`TASK-98.1`, ADR-0037): files bound to tasks, comments, messages, asks,
//! projects, the organization, or a person's private space.
//!
//! - [`blobs`]: content-addressed bytes on disk, outside the office store's trees.
//! - [`media`]: the allowlist; image sanitization and document validation.
//! - [`staging`]: unattached uploads, uploader-only, with quota and expiry.
//! - [`records`], [`ports`], [`projector`]: the shared attachments family, projected from
//!   canonical YAML under `durable/state/` into the `attachments` table on every engine.
//! - [`user_space`]: HQ's human-private space (`private/user-space.db`).
//! - [`outbox`], [`cache`], [`remote`]: a satellite's shared files across offices
//!   (`TASK-98.6`): its own new files wait durably in the outbox until HQ acks them, files
//!   from HQ are pulled on demand through [`remote::BlobRemote`] into a bounded LRU cache, and
//!   HQ keeps the authoritative shared bytes in `<durable>/blobs/`. Private bytes never leave
//!   HQ: they enter neither the outbox, the cache nor any federation frame.
//!
//! Who may do what is decided only by [`crate::authz`]; the office's RPC facade is
//! `Office::attachment_*` (`office/attachments/`).

pub mod blobs;
pub mod cache;
pub mod media;
pub mod outbox;
pub mod ports;
pub mod projector;
pub mod records;
pub mod remote;
pub mod staging;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transfer_tests;
pub mod user_space;

use std::fmt;
use std::sync::Arc;

/// Chief of Staff turn partition scope for [`crate::Office::attachment_read_for_delivery_with`]
/// (`TASK-80.1`, `TASK-240`, `TASK-98.3`).
///
/// Ordinary agents ignore this field (standard `AuthorizationPolicy::can_read` applies). For the
/// built-in Chief of Staff (fail-closed check):
/// - [`Self::None`] (`tess:wake`): channels only; refuses any DM/GroupDm message or answer file.
/// - [`Self::ActingForPerson`] (`tess:ask:<caller_id>`): requires an active `Person`, enforces
///   `can_read`, and requires actual conversation membership (or channel) for message/answer files
///   so even the owner never crosses into another member's DM with Tess.
/// - [`Self::DirectSender`] (`tess:dm:<sender_id>`): requires an active employee who can read the
///   file and is an actual participant (or channel reader) for message/answer files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DeliveryScope {
    #[default]
    None,
    ActingForPerson(String),
    DirectSender(String),
}

/// Policy-authorized, hash-verified attachment payload ready for model turn delivery (`TASK-98.3`).
///
/// `Debug` never prints raw image or document bytes.
#[derive(Clone)]
pub enum DeliveryAttachmentPayload {
    /// Allowlisted image with its bounded model derivative (`<= 1568px`, `<= 1 MiB`).
    Image {
        record: records::AttachmentRecord,
        derivative: media::ModelDerivative,
    },
    /// Allowlisted document (`PDF`, `DOCX`, `XLSX`, `MD`, `TXT`, `CSV`, `JSON` <= 25 MiB).
    /// `bytes` is `None` when staging this document would exceed the caller's remaining per-turn
    /// eager staging budget (so callers fall back to `agent-os file fetch --id <id>` without
    /// pulling or loading the blob into memory).
    Document {
        record: records::AttachmentRecord,
        bytes: Option<Vec<u8>>,
    },
}

impl fmt::Debug for DeliveryAttachmentPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Image { record, derivative } => f
                .debug_struct("DeliveryAttachmentPayload::Image")
                .field("id", &record.id)
                .field("media_type", &derivative.media.mime())
                .field("derivative_bytes", &derivative.bytes.len())
                .finish(),
            Self::Document { record, bytes } => f
                .debug_struct("DeliveryAttachmentPayload::Document")
                .field("id", &record.id)
                .field("mime_type", &record.mime_type)
                .field("staged_bytes", &bytes.as_ref().map(Vec::len))
                .finish(),
        }
    }
}

/// The stores an office keeps its attachments in, beside its journal.
pub struct AttachmentStores {
    /// `<durable>/blobs/`: shared bytes.
    pub shared: Arc<dyn blobs::BlobStore>,
    /// `<durable>/private/blobs/`: HQ's private bytes; `None` on a satellite.
    pub private: Option<Arc<dyn blobs::BlobStore>>,
    pub staging: staging::StagingArea,
    pub user_space: Arc<dyn user_space::UserSpaceStore>,
}
