//! A satellite's side of shipping its own shared files to HQ (`TASK-98.6`, ADR-0037). The
//! federation adapter lists what waits ([`Office::attachment_outbox_pending`]), reads each
//! file's bytes ([`Office::attachment_outbox_bytes`]) and, only after HQ's verified `BlobAck`,
//! tells the office ([`Office::attachment_outbox_acknowledged`]), which keeps the bytes in its
//! cache and drops the entry.

use org_store::Id;

use super::Office;
use crate::attachments::blobs::ContentHash;
use crate::attachments::media::MediaType;
use crate::attachments::remote::OutboxShipment;
use crate::error::OfficeError;
use crate::office::storage;
use crate::store::open::attachment_ops::AttachmentById;

impl Office {
    /// Every outbox entry ready to ship, oldest first: those whose binding this office has
    /// committed. An entry whose binding never committed (a crash between the two writes)
    /// waits, never shipped and never removed.
    pub fn attachment_outbox_pending(&self) -> Result<Vec<OutboxShipment>, OfficeError> {
        if !self.is_satellite() {
            return Ok(Vec::new());
        }
        let mut ready = Vec::new();
        for entry in self.outbox().pending()? {
            let bound = self
                .store
                .read(AttachmentById(entry.record.id.clone()))
                .map_err(storage)?
                .0;
            if bound.is_none_or(|bound| bound.content_sha256 != entry.record.content_sha256) {
                continue;
            }
            let Some(media) = MediaType::from_mime(&entry.record.mime_type) else {
                continue;
            };
            ready.push(OutboxShipment {
                attachment_id: entry.record.id.to_string(),
                content_sha256: entry.record.content_sha256.to_string(),
                byte_count: entry.record.byte_count,
                record_yaml: entry.record_yaml,
                max_bytes: media.max_bytes(),
            });
        }
        Ok(ready)
    }

    /// The bytes of a waiting file, checked against its record; `None` when they are gone.
    pub fn attachment_outbox_bytes(
        &self,
        shipment: &OutboxShipment,
    ) -> Result<Option<Vec<u8>>, OfficeError> {
        let hash = ContentHash::parse(&shipment.content_sha256)
            .ok_or(OfficeError::Invalid("an outbox digest"))?;
        let Some(bytes) = self.outbox().blobs().read_all(&hash, shipment.max_bytes)? else {
            return Ok(None);
        };
        if bytes.len() as u64 != shipment.byte_count || ContentHash::of(&bytes) != hash {
            return Err(OfficeError::Invalid(
                "an outbox blob does not match its record",
            ));
        }
        Ok(Some(bytes))
    }

    /// HQ acknowledged `shipment` with a verified `BlobAck`: the bytes move to the cache (so
    /// they stay readable here until evicted) and the entry goes, with the outbox's copy once
    /// no other entry names it.
    pub fn attachment_outbox_acknowledged(
        &self,
        shipment: &OutboxShipment,
        cache_max_bytes: u64,
    ) -> Result<(), OfficeError> {
        let id =
            Id::parse(&shipment.attachment_id).map_err(|_| OfficeError::Invalid("an outbox id"))?;
        if !self.outbox().is_pending(&id) {
            return Ok(());
        }
        if let (Some(hash), Some(bytes)) = (
            ContentHash::parse(&shipment.content_sha256),
            self.attachment_outbox_bytes(shipment)?,
        ) {
            self.blob_cache().insert(&hash, &bytes, cache_max_bytes)?;
        }
        self.outbox().release(&id)?;
        Ok(())
    }

    /// How many files wait in this satellite's outbox, shippable or not (diagnostics).
    pub fn attachment_outbox_len(&self) -> Result<usize, OfficeError> {
        Ok(self.outbox().pending()?.len())
    }
}
