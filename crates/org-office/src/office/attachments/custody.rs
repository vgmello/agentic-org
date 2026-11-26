//! HQ's authoritative custody of shared bytes across offices (`TASK-98.6`, ADR-0037).
//!
//! - **Ingest.** A satellite ships one shared file from its outbox: HQ checks the record before
//!   any byte arrives ([`Office::attachment_ingest_check`]), then keeps the verified bytes in
//!   `<durable>/blobs/` and binds the record through its journal
//!   ([`Office::attachment_ingest_commit`]), so it replicates to every office. Only a project or
//!   organization file is accepted, from an uploader who may add to its target now, and an
//!   agent's only from the office it is placed in.
//! - **Fetch.** A satellite asks for one shared file by `attachment_id`
//!   ([`Office::attachment_peer_fetch`]): HQ looks in its shared projection and shared store
//!   alone. The private space (`private/user-space.db`, `private/blobs/`) is never read for a
//!   satellite, whatever ID or digest it sends.

use org_proto::attachments::AttachmentScope;
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Sha256};

use super::{invalid, not_found, target_ref, Answer, Office};
use crate::attachments::blobs::ContentHash;
use crate::attachments::media::{self, MediaType};
use crate::attachments::records::{self, AttachmentRecord};
use crate::attachments::remote::{PeerBlob, ShippedAttachment};
use crate::authz::PrincipalKind;
use crate::error::OfficeError;
use crate::office::storage;
use crate::store::open::attachment_ops::{
    attachment_operation, AttachmentById, AttachmentWrite, AttachmentWriteOp,
};

const INGEST: &str = "attachment.ingest";

fn hq_custody_only() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "only HQ keeps shared files for other offices",
    )
}

/// Whether `existing` is the very file `shipped` names: same bytes, target and uploader.
fn same_file(existing: &AttachmentRecord, shipped: &AttachmentRecord) -> bool {
    existing.content_sha256 == shipped.content_sha256
        && existing.byte_count == shipped.byte_count
        && existing.target_kind == shipped.target_kind
        && existing.target_id == shipped.target_id
        && existing.uploaded_by_employee_id == shipped.uploaded_by_employee_id
}

/// The command a shipped file is bound under at HQ: derived from its attachment ID (its
/// timestamp kept), so a replay is the same command.
fn ingest_command_id(id: &Id) -> String {
    let hex: String = id.as_str().chars().filter(|c| *c != '-').collect();
    let hash = Sha256::of_parts(&[INGEST.as_bytes(), b"\0", id.as_str().as_bytes()]);
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().take(6).enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    bytes[6..16].copy_from_slice(&hash.as_bytes()[0..10]);
    bytes[6] = 0x70 | (bytes[6] & 0x0f);
    bytes[8] = 0x80 | (bytes[8] & 0x3f);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

impl Office {
    /// The shared record `id`, from the projection alone (never the private space).
    fn shared_record(&self, id: &Id) -> Result<Option<AttachmentRecord>, OfficeError> {
        Ok(self
            .store
            .read(AttachmentById(id.clone()))
            .map_err(storage)?
            .0
            .filter(|record| record.scope() == AttachmentScope::Shared))
    }

    /// Whether agent `agent_id` is placed in office `office_id`.
    fn agent_placed_at(&self, agent_id: &str, office_id: &str) -> Result<bool, OfficeError> {
        Ok(self
            .org_snapshot()?
            .members
            .iter()
            .any(|m| m.id == agent_id && m.office_id.as_deref() == Some(office_id)))
    }

    /// HQ: checks a shared file satellite `origin_office` announces from its outbox, before any
    /// of its bytes arrive. `present` says HQ already holds the bytes (none are then sent).
    pub fn attachment_ingest_check(
        &self,
        origin_office: &str,
        attachment_id: &str,
        content_sha256: &str,
        byte_count: u64,
        record_yaml: &[u8],
    ) -> Answer<ShippedAttachment> {
        if self.is_satellite() {
            return Ok(Err(hq_custody_only()));
        }
        let Ok(record) = records::decode(record_yaml) else {
            return Ok(Err(invalid(
                "the shipped record is not a valid attachment record",
            )));
        };
        if record.id.as_str() != attachment_id
            || record.content_sha256.as_str() != content_sha256
            || record.byte_count != byte_count
        {
            return Ok(Err(invalid(
                "the shipped record does not name the announced file",
            )));
        }
        if record.scope() != AttachmentScope::Shared || records::record_path(&record).is_none() {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only a shared project or organization file is shipped to HQ",
            )));
        }
        let Some(media) = MediaType::from_mime(&record.mime_type) else {
            return Ok(Err(invalid("the shipped file has no accepted type")));
        };
        if byte_count == 0 || byte_count > media.max_bytes() {
            return Ok(Err(media::too_large(media)));
        }
        let present = self.attachments.shared.size(&record.content_sha256)? == Some(byte_count);
        // A file HQ already bound (a retry after a lost ack) is answered as it is, whatever
        // has changed since: nothing new is granted.
        if let Some(existing) = self.shared_record(&record.id)? {
            if !same_file(&existing, &record) {
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    "attachment id in use",
                )));
            }
            return Ok(Ok(ShippedAttachment {
                record,
                media,
                present,
            }));
        }
        let uploader = record.uploaded_by_employee_id.to_string();
        let caller = match self.attachment_caller(&uploader)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if caller.kind == PrincipalKind::Agent && !self.agent_placed_at(&uploader, origin_office)? {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "an agent's files ship only from the office it is placed in",
            )));
        }
        let target = target_ref(
            record.target_kind,
            record.target_id.as_ref(),
            &record.uploaded_by_employee_id,
        );
        if let Err(refusal) = self.decide(|policy| policy.can_write(&caller, &target))? {
            return Ok(Err(refusal));
        }
        Ok(Ok(ShippedAttachment {
            record,
            media,
            present,
        }))
    }

    /// HQ: keeps a checked file's verified `bytes` (when HQ did not hold them already) in
    /// `<durable>/blobs/`, written once, and binds its record. `Ok(Ok(()))` means both are
    /// durable: only then may the satellite drop its outbox entry.
    pub fn attachment_ingest_commit(
        &self,
        shipped: &ShippedAttachment,
        bytes: Option<Vec<u8>>,
    ) -> Answer<()> {
        if self.is_satellite() {
            return Ok(Err(hq_custody_only()));
        }
        let record = &shipped.record;
        match bytes {
            Some(bytes) => {
                if bytes.len() as u64 != record.byte_count
                    || ContentHash::of(&bytes) != record.content_sha256
                {
                    return Ok(Err(invalid("the bytes do not match the announced file")));
                }
                // The satellite checked the file when it was uploaded; HQ checks again that
                // the bytes are what their type says before it keeps them for everyone.
                let bytes = if shipped.media.is_image() {
                    if let Err(refusal) = media::validate(shipped.media, bytes.clone()) {
                        return Ok(Err(refusal));
                    }
                    bytes
                } else {
                    match media::validate(shipped.media, bytes) {
                        Ok(kept) => kept.bytes,
                        Err(refusal) => return Ok(Err(refusal)),
                    }
                };
                if self.attachments.shared.put(&bytes)? != record.content_sha256 {
                    return Err(OfficeError::Invalid(
                        "a stored blob does not match its hash",
                    ));
                }
            }
            None => {
                if self.attachments.shared.size(&record.content_sha256)? != Some(record.byte_count)
                {
                    return Ok(Err(ApiError::new(
                        ErrorCode::Conflict,
                        "HQ does not hold this file's bytes; send them",
                    )));
                }
            }
        }
        self.bind_shipped(record)
    }

    fn bind_shipped(&self, record: &AttachmentRecord) -> Answer<()> {
        if let Some(existing) = self.shared_record(&record.id)? {
            return Ok(if same_file(&existing, record) {
                Ok(())
            } else {
                Err(ApiError::new(ErrorCode::Conflict, "attachment id in use"))
            });
        }
        let fingerprint = records::encode(record);
        let Some(operation) = attachment_operation(
            &ingest_command_id(&record.id),
            INGEST,
            record.uploaded_by_employee_id.as_str(),
            &fingerprint,
        ) else {
            return Ok(Err(invalid("the shipped attachment id is not a UUIDv7")));
        };
        let written = self
            .store
            .run(AttachmentWriteOp {
                operation,
                write: AttachmentWrite::Create(record.clone()),
            })
            .map_err(storage)?;
        match written {
            Ok(_) => Ok(Ok(())),
            // A replay that raced this one: what is bound now decides.
            Err(refusal) => match self.shared_record(&record.id)? {
                Some(existing) if same_file(&existing, record) => Ok(Ok(())),
                _ => Ok(Err(refusal)),
            },
        }
    }

    /// HQ: the shared file a satellite may fetch, by `attachment_id` alone. Anything that is not
    /// a shared record of HQ's projection (a private file's ID, a digest, garbage) is
    /// `not_found`.
    pub fn attachment_peer_fetch(&self, attachment_id: &str) -> Answer<PeerBlob> {
        if self.is_satellite() {
            return Ok(Err(hq_custody_only()));
        }
        let Some(id) = Id::parse(attachment_id)
            .ok()
            .filter(|_| org_proto::is_uuid_v7(attachment_id))
        else {
            return Ok(Err(not_found()));
        };
        let Some(record) = self.shared_record(&id)? else {
            return Ok(Err(not_found()));
        };
        if self.attachments.shared.size(&record.content_sha256)? != Some(record.byte_count) {
            return Ok(Err(ApiError::new(
                ErrorCode::Unavailable,
                "HQ does not hold this file's bytes yet",
            )));
        }
        Ok(Ok(PeerBlob {
            attachment_id: record.id.to_string(),
            content_sha256: record.content_sha256,
            byte_count: record.byte_count,
        }))
    }

    /// HQ: up to `max` bytes of a fetched file from `offset`, from the shared store only.
    pub fn attachment_peer_range(
        &self,
        blob: &PeerBlob,
        offset: u64,
        max: usize,
    ) -> Result<Option<Vec<u8>>, OfficeError> {
        self.attachments
            .shared
            .read_range(&blob.content_sha256, offset, max)
    }
}

#[cfg(test)]
mod tests {
    use super::ingest_command_id;
    use org_store::Id;

    #[test]
    fn an_ingest_command_is_a_uuid_v7_named_by_its_attachment() {
        let id = Id::parse("0195a000-1234-7abc-8def-0123456789ab").unwrap();
        let command = ingest_command_id(&id);
        assert!(org_proto::is_uuid_v7(&command), "{command}");
        assert_eq!(command, ingest_command_id(&id));
        assert!(command.starts_with("0195a000-1234-7"));
        let other = Id::parse("0195a000-1234-7abc-8def-0123456789ac").unwrap();
        assert_ne!(command, ingest_command_id(&other));
    }
}
