//! Attachments (`TASK-98.1`, ADR-0037): files bound to tasks, comments, messages, asks,
//! projects, the organization, or a person's private space.
//!
//! - [`blobs`]: content-addressed bytes on disk, outside the office store's trees.
//! - [`media`]: the allowlist; image sanitization and document validation.
//! - [`staging`]: unattached uploads, uploader-only, with quota and expiry.
//! - [`records`], [`ports`], [`projector`]: the shared attachments family, projected from
//!   canonical YAML under `durable/state/` into the `attachments` table on every engine.
//! - [`user_space`]: HQ's human-private space (`private/user-space.db`).
//!
//! Who may do what is decided only by [`crate::authz`]; the office's RPC facade is
//! `Office::attachment_*` (`office/attachments/`).

pub mod blobs;
pub mod media;
pub mod ports;
pub mod projector;
pub mod records;
pub mod staging;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
#[cfg(test)]
mod tests;
pub mod user_space;

use std::sync::Arc;

/// The stores an office keeps its attachments in, beside its journal.
pub(crate) struct AttachmentStores {
    /// `<durable>/blobs/`: shared bytes.
    pub shared: Arc<dyn blobs::BlobStore>,
    /// `<durable>/private/blobs/`: HQ's private bytes; `None` on a satellite.
    pub private: Option<Arc<dyn blobs::BlobStore>>,
    pub staging: staging::StagingArea,
    pub user_space: Arc<dyn user_space::UserSpaceStore>,
}
