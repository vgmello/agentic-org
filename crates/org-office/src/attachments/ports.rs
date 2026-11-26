//! Read and write ports of the shared attachments family: the projection's `attachments`
//! table, whichever engine holds it.

use org_proto::attachments::AttachmentTargetKind;
use org_store::{Engine, EngineError, Id};

use super::records::AttachmentRecord;

pub trait AttachmentsRead {
    fn attachment(&self, id: &Id) -> Result<Option<AttachmentRecord>, EngineError>;
    /// One target's attachments in ascending ID order (UUIDv7: oldest first), those after
    /// `after` only, at most `limit`.
    fn attachments_for_target(
        &self,
        kind: AttachmentTargetKind,
        target_id: Option<&Id>,
        after: Option<&Id>,
        limit: usize,
    ) -> Result<Vec<AttachmentRecord>, EngineError>;
    /// Every attachment in ascending ID order.
    fn attachments(&self) -> Result<Vec<AttachmentRecord>, EngineError>;
}

pub trait AttachmentsWrite: AttachmentsRead {
    fn put_attachment(&mut self, record: &AttachmentRecord) -> Result<(), EngineError>;
    fn remove_attachment(&mut self, id: &Id) -> Result<(), EngineError>;
}

pub trait AttachmentsEngine: Engine<Tx: AttachmentsWrite, Snapshot: AttachmentsRead> {}

impl<E> AttachmentsEngine for E where E: Engine<Tx: AttachmentsWrite, Snapshot: AttachmentsRead> {}
