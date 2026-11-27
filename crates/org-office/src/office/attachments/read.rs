//! `attachment.inspect`, `attachment.list` and `attachment.read_chunk`, plus the model
//! derivative a later phase hands to agents. Every read passes the policy first; a file the
//! caller may not read does not exist for them.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_proto::attachments::{
    AttachmentInspectParams, AttachmentListParams, AttachmentListResult, AttachmentReadChunkParams,
    AttachmentReadChunkResult, AttachmentResult, AttachmentScope, AttachmentTargetKind,
    ATTACHMENT_CHUNK_MAX_BYTES, ATTACHMENT_LIST_MAX_LIMIT,
};
use org_store::Id;

use super::{invalid, not_found, record_ref, target_ref, Answer, Office};
use crate::attachments::blobs::ContentHash;
use crate::attachments::media::{self, MediaType, ModelDerivative};
use crate::attachments::records::AttachmentRecord;
use crate::attachments::remote::{bytes_unavailable, BlobPull};
use crate::attachments::user_space::satellite_refusal;
use crate::attachments::DeliveryAttachmentPayload;
use crate::authz::PrincipalContext;
use crate::error::OfficeError;
use crate::office::storage;
use crate::store::open::attachment_ops::AttachmentsForTarget;

const DEFAULT_LIST_LIMIT: u32 = 50;

impl Office {
    /// A record the caller may read, or `not_found`.
    fn readable(&self, caller: &PrincipalContext, attachment_id: &str) -> Answer<AttachmentRecord> {
        let Some(record) = self.find_attachment(attachment_id)? else {
            return Ok(Err(not_found()));
        };
        let resource = record_ref(&record);
        if let Err(refusal) = self.decide(|policy| policy.can_read(caller, &resource))? {
            return Ok(Err(refusal));
        }
        Ok(Ok(record))
    }

    /// `attachment.inspect`.
    pub fn attachment_inspect(
        &self,
        actor: &str,
        params: AttachmentInspectParams,
    ) -> Answer<AttachmentResult> {
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        Ok(self
            .readable(&caller, &params.attachment_id)?
            .map(|record| AttachmentResult {
                attachment: record.to_view(),
            }))
    }

    /// `attachment.list`: one target's files, or the caller's own private space, by ID.
    pub fn attachment_list(
        &self,
        actor: &str,
        params: AttachmentListParams,
    ) -> Answer<AttachmentListResult> {
        // A scope that says otherwise than the target kind is refused, never ignored.
        let contradicts = match (params.scope, params.target_kind) {
            (Some(AttachmentScope::UserPrivate), Some(kind)) => {
                kind != AttachmentTargetKind::UserPrivate
            }
            (Some(AttachmentScope::Shared), Some(kind)) => {
                kind == AttachmentTargetKind::UserPrivate
            }
            _ => false,
        };
        if contradicts {
            return Ok(Err(invalid(
                "scope and target_kind disagree: user_private files are listed with the user_private scope alone",
            )));
        }
        let private = params.target_kind == Some(AttachmentTargetKind::UserPrivate)
            || (params.target_kind.is_none() && params.scope == Some(AttachmentScope::UserPrivate));
        // A satellite holds no private space: refused the same for every caller.
        if private && !self.attachments.user_space.available() {
            return Ok(Err(satellite_refusal()));
        }
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let limit = params.limit.unwrap_or(DEFAULT_LIST_LIMIT);
        if limit == 0 || limit > ATTACHMENT_LIST_MAX_LIMIT {
            return Ok(Err(invalid("limit is 1 to 200")));
        }
        let after = match params.cursor.as_deref() {
            None => None,
            Some(cursor) => match Id::parse(cursor)
                .ok()
                .filter(|_| org_proto::is_uuid_v7(cursor))
            {
                Some(after) => Some(after),
                None => return Ok(Err(invalid("cursor is the next_cursor of a previous page"))),
            },
        };
        let Some(kind) = params
            .target_kind
            .or(private.then_some(AttachmentTargetKind::UserPrivate))
        else {
            return Ok(Err(invalid(
                "name a target_kind, or the user_private scope",
            )));
        };
        let target_text = if private {
            Some(
                params
                    .target_id
                    .clone()
                    .unwrap_or_else(|| caller.employee_id.clone()),
            )
        } else {
            params.target_id.clone()
        };
        let target_id = match target_text.as_deref() {
            None => None,
            Some(text) => match Id::parse(text).ok().filter(|_| org_proto::is_uuid_v7(text)) {
                Some(id) => Some(id),
                None => {
                    return Ok(Err(invalid(
                        "target_id must be a canonical lowercase UUIDv7",
                    )))
                }
            },
        };
        if (kind == AttachmentTargetKind::Organization) != target_id.is_none() {
            return Ok(Err(invalid(
                "name a target_id for every target_kind but organization",
            )));
        }
        let resource = target_ref(
            kind,
            target_id.as_ref(),
            &Id::parse(&caller.employee_id).map_err(|_| OfficeError::Invalid("a caller id"))?,
        );
        if let Err(refusal) = self.decide(|policy| policy.can_read(&caller, &resource))? {
            return Ok(Err(refusal));
        }
        let page = limit as usize + 1;
        let mut rows = if private {
            let owner = target_id.as_ref().map(Id::as_str).unwrap_or_default();
            self.attachments.user_space.list(
                &self.info.org_id,
                owner,
                after.as_ref().map(Id::as_str),
                page,
            )?
        } else {
            let read = AttachmentsForTarget {
                kind,
                target_id,
                after,
                limit: page,
            };
            self.store.read(read).map_err(storage)?.0
        };
        let next_cursor = if rows.len() > limit as usize {
            rows.truncate(limit as usize);
            rows.last().map(|r| r.id.to_string())
        } else {
            None
        };
        Ok(Ok(AttachmentListResult {
            attachments: rows.iter().map(AttachmentRecord::to_view).collect(),
            next_cursor,
        }))
    }

    /// `attachment.read_chunk`: at most 512 KiB of a readable file from `offset`, from the
    /// bytes this office holds.
    pub fn attachment_read_chunk(
        &self,
        actor: &str,
        params: AttachmentReadChunkParams,
    ) -> Answer<AttachmentReadChunkResult> {
        self.attachment_read_chunk_with(actor, params, None)
    }

    /// `attachment.read_chunk` that may `pull` a shared file's bytes from HQ when this
    /// satellite does not hold them (`TASK-98.6`): only after the policy allowed the read, and
    /// `unavailable` (worth retrying) when HQ cannot be reached.
    pub fn attachment_read_chunk_with(
        &self,
        actor: &str,
        params: AttachmentReadChunkParams,
        pull: Option<BlobPull<'_>>,
    ) -> Answer<AttachmentReadChunkResult> {
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let max = params.max_bytes.unwrap_or(ATTACHMENT_CHUNK_MAX_BYTES);
        if max == 0 || max > ATTACHMENT_CHUNK_MAX_BYTES {
            return Ok(Err(invalid("max_bytes is 1 to 524288")));
        }
        let record = match self.readable(&caller, &params.attachment_id)? {
            Ok(record) => record,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if params.offset > record.byte_count {
            return Ok(Err(invalid("offset is past the end of the file")));
        }
        let bytes = match self.local_blob_range(&record, params.offset, max as usize)? {
            Some(bytes) => bytes,
            None => match pull.filter(|_| self.may_pull(&record)) {
                None => {
                    return Ok(Err(bytes_unavailable(
                        "the attachment's bytes are not on this office",
                    )))
                }
                Some(pull) => match self.pull_shared_blob(&record, pull)? {
                    Ok(whole) => {
                        let start = usize::try_from(params.offset)
                            .unwrap_or(usize::MAX)
                            .min(whole.len());
                        let end = start.saturating_add(max as usize).min(whole.len());
                        whole[start..end].to_vec()
                    }
                    Err(refusal) => return Ok(Err(refusal)),
                },
            },
        };
        let end = params.offset + bytes.len() as u64;
        Ok(Ok(AttachmentReadChunkResult {
            attachment_id: record.id.to_string(),
            offset: params.offset,
            data_base64: STANDARD.encode(&bytes),
            eof: end >= record.byte_count,
            total_bytes: record.byte_count,
            content_sha256: record.content_sha256.to_string(),
        }))
    }

    /// Reads and hash-verifies all bytes of an already-authorized `record`, pulling a shared
    /// blob from HQ on a satellite when `pull` is supplied and the blob is not cached locally.
    fn read_authorized_blob_all(
        &self,
        record: &AttachmentRecord,
        max_bytes: u64,
        pull: Option<BlobPull<'_>>,
    ) -> Answer<Vec<u8>> {
        if record.scope() == AttachmentScope::UserPrivate && self.attachments.private.is_none() {
            return Ok(Err(satellite_refusal()));
        }
        let bytes = match self.local_blob_all(record, max_bytes)? {
            Some(bytes) => bytes,
            None => match pull.filter(|_| self.may_pull(record)) {
                None => {
                    return Ok(Err(bytes_unavailable(
                        "the attachment's bytes are not on this office",
                    )))
                }
                Some(pull) => match self.pull_shared_blob(record, pull)? {
                    Ok(bytes) => bytes,
                    Err(refusal) => return Ok(Err(refusal)),
                },
            },
        };
        if ContentHash::of(&bytes) != record.content_sha256 {
            return Err(OfficeError::Invalid(
                "an attachment blob does not match its hash",
            ));
        }
        Ok(Ok(bytes))
    }

    /// The bounded copy of a readable image a model reads (at most 1568 px on its longest
    /// side and 1 MiB), made from the stored bytes.
    pub fn attachment_model_derivative(
        &self,
        actor: &str,
        attachment_id: &str,
    ) -> Answer<ModelDerivative> {
        Ok(self
            .attachment_model_derivative_with(actor, attachment_id, None)?
            .map(|(_, derivative)| derivative))
    }

    /// Bounded model derivative for an image attachment, optionally pulling missing shared bytes
    /// from HQ on a satellite (`TASK-98.3`, `TASK-98.6`).
    pub fn attachment_model_derivative_with(
        &self,
        actor: &str,
        attachment_id: &str,
        pull: Option<BlobPull<'_>>,
    ) -> Answer<(AttachmentRecord, ModelDerivative)> {
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let record = match self.readable(&caller, attachment_id)? {
            Ok(record) => record,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let Some(media) = MediaType::from_mime(&record.mime_type).filter(|m| m.is_image()) else {
            return Ok(Err(media::unsupported(
                "only images have a model derivative",
            )));
        };
        let bytes = match self.read_authorized_blob_all(&record, media.max_bytes(), pull)? {
            Ok(bytes) => bytes,
            Err(refusal) => return Ok(Err(refusal)),
        };
        match media::model_derivative(media, &bytes) {
            Ok(derivative) => Ok(Ok((record, derivative))),
            Err(err) => Ok(Err(err)),
        }
    }

    /// Resolves one policy-authorized attachment for model turn delivery (`TASK-98.3`):
    /// - Allowlisted images return their bounded [`ModelDerivative`] (`<= 1568px`, `<= 1 MiB`).
    /// - Allowlisted documents return raw bytes when `record.byte_count <= remaining_doc_budget`,
    ///   or `bytes: None` when staging would exceed the caller's per-turn eager staging budget
    ///   (without loading or pulling the oversized blob).
    pub fn attachment_read_for_delivery_with(
        &self,
        actor: &str,
        attachment_id: &str,
        remaining_doc_budget: u64,
        pull: Option<BlobPull<'_>>,
    ) -> Answer<DeliveryAttachmentPayload> {
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let record = match self.readable(&caller, attachment_id)? {
            Ok(record) => record,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let Some(media) = MediaType::from_mime(&record.mime_type) else {
            return Ok(Err(media::unsupported(
                "attachment media type is not allowlisted for model delivery",
            )));
        };
        if media.is_image() {
            let bytes = match self.read_authorized_blob_all(&record, media.max_bytes(), pull)? {
                Ok(bytes) => bytes,
                Err(refusal) => return Ok(Err(refusal)),
            };
            return match media::model_derivative(media, &bytes) {
                Ok(derivative) => Ok(Ok(DeliveryAttachmentPayload::Image { record, derivative })),
                Err(err) => Ok(Err(err)),
            };
        }
        if record.byte_count > media.max_bytes() {
            return Ok(Err(media::unsupported(
                "document attachment exceeds per-file size limit",
            )));
        }
        if record.byte_count > remaining_doc_budget {
            return Ok(Ok(DeliveryAttachmentPayload::Document {
                record,
                bytes: None,
            }));
        }
        let bytes = match self.read_authorized_blob_all(&record, media.max_bytes(), pull)? {
            Ok(bytes) => bytes,
            Err(refusal) => return Ok(Err(refusal)),
        };
        Ok(Ok(DeliveryAttachmentPayload::Document {
            record,
            bytes: Some(bytes),
        }))
    }
}
