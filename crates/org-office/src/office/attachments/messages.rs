//! Files on messages, asks and answers (`TASK-98.2`, ADR-0037): the attachment facade's side
//! of `chat.send`, `msg.reply`, `chat.ask` and `chat.answer`.
//!
//! - **Before the commit** ([`Office::prepare_message_files`]): every named upload must be the
//!   caller's committed upload, as the policy decides (anyone else's is `not_found`), and within
//!   its type's size limit; its sanitized bytes are then promoted write-once into
//!   `<durable>/blobs/sha256/<aa>/<bb>/<sha256>`, so no committed record ever names a blob
//!   that is not there. The decision binds them inside the commit
//!   ([`crate::messaging::files`]).
//! - **After the commit** ([`Office::release_message_files`]): the bound uploads leave staging.
//!   A refused record leaves them staged; a promoted blob nothing names stays unreadable, as
//!   every read goes through a record and the policy.
//! - **Reads**: a bound file is found by its attachment ID and read as its message or answer
//!   is ([`Office::message_file_record`]). The Chief of Staff's gate reads which conversation
//!   such a file is in ([`Office::attachment_conversation_id`], `TASK-240`), so she touches one
//!   only for a member who is in that conversation too.

use org_proto::attachments::{check_attachment_ids, AttachmentTargetKind};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use super::{Answer, Office};
use crate::attachments::blobs::ContentHash;
use crate::attachments::media;
use crate::attachments::records::AttachmentRecord;
use crate::error::OfficeError;
use crate::messaging::files::{MessageFiles, StagedFile};
use crate::office::storage;
use crate::store::open::attachment_ops::MessageFileById;

impl Office {
    /// The files a message, ask or answer names, ready for its decision. `record_id` is the
    /// message or ask the files bind under: a canonical UUIDv7. An upload that is not there for
    /// the caller leaves the files incomplete: the decision refuses a new record with
    /// `not_found`, and recognizes an exact retry of a committed one by its bound IDs.
    pub(crate) fn prepare_message_files(
        &self,
        actor: &str,
        record_id: &str,
        attachment_ids: &[String],
    ) -> Answer<MessageFiles> {
        if attachment_ids.is_empty() {
            return Ok(Ok(MessageFiles::none()));
        }
        if let Err(refusal) = check_attachment_ids(attachment_ids) {
            return Ok(Err(refusal));
        }
        if !org_proto::is_uuid_v7(record_id) {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "a message, ask or answer with attachments needs a canonical lowercase UUIDv7 id",
            )));
        }
        // Message/ask/answer attachments promote into HQ's shared blob store directly.
        if self.is_satellite() {
            return Ok(Err(ApiError::new(
                ErrorCode::Unavailable,
                "attaching files to messages, asks or answers is handled at HQ; retry against the HQ office",
            )));
        }
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let mut files = MessageFiles {
            caller: Some(caller),
            requested: attachment_ids.to_vec(),
            staged: Vec::new(),
        };
        let mut uploads = Vec::with_capacity(attachment_ids.len());
        for upload_id in attachment_ids {
            let caller = files.caller.as_ref().expect("set above");
            match self.staged_for(caller, upload_id)? {
                Ok(upload) => uploads.push(upload),
                Err(refusal) if refusal.code == ErrorCode::NotFound => return Ok(Ok(files)),
                Err(refusal) => return Ok(Err(refusal)),
            }
        }
        // Every upload is checked before any byte moves.
        let mut kept = Vec::with_capacity(uploads.len());
        for upload in &uploads {
            let Some(committed) = upload.committed.clone() else {
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    "commit the upload before attaching it",
                )));
            };
            let media = committed.media().ok_or(OfficeError::Invalid(
                "a staged upload names an unknown type",
            ))?;
            if committed.byte_count > media.max_bytes() {
                return Ok(Err(media::too_large(media)));
            }
            kept.push((committed, media));
        }
        for (upload, (committed, media)) in uploads.into_iter().zip(kept) {
            let bytes = self.attachments.staging.kept_bytes(&upload)?;
            if bytes.len() as u64 > media.max_bytes() {
                return Ok(Err(media::too_large(media)));
            }
            let hash = self.attachments.shared.put(&bytes)?;
            if hash.as_str() != committed.content_sha256 {
                return Err(OfficeError::Invalid("a staged upload changed after commit"));
            }
            files.staged.push(StagedFile {
                upload_id: upload.upload_id,
                uploaded_by: upload.uploaded_by,
                content_sha256: committed.content_sha256,
                media_type: committed.mime_type,
                byte_count: bytes.len() as u64,
                filename: upload.filename,
                width: committed.width,
                height: committed.height,
                page_count: committed.page_count,
            });
        }
        Ok(Ok(files))
    }

    /// Removes the uploads a committed record bound from staging. The record is committed
    /// whatever happens here, so a removal that fails is left to the staging area's expiry
    /// sweep rather than reported as the call's failure; the upload stays its uploader's.
    pub(crate) fn release_message_files(&self, files: &MessageFiles) {
        for file in &files.staged {
            let _ = self.attachments.staging.remove(&file.upload_id);
        }
    }

    /// The conversation the shared file `attachment_id` is in when it is bound to a message, an
    /// ask or an answer (`TASK-240`); `None` for any other file and for an unknown ID. A bound
    /// file whose message is in no conversation is an error, so a gate reading this refuses.
    pub fn attachment_conversation_id(
        &self,
        attachment_id: &str,
    ) -> Result<Option<String>, OfficeError> {
        let Some(id) = Id::parse(attachment_id)
            .ok()
            .filter(|_| org_proto::is_uuid_v7(attachment_id))
        else {
            return Ok(None);
        };
        let Some(record) = self.shared_file(&id)? else {
            return Ok(None);
        };
        if !matches!(
            record.target_kind,
            AttachmentTargetKind::Message | AttachmentTargetKind::AskAnswer
        ) {
            return Ok(None);
        }
        let message = record
            .target_id
            .ok_or(OfficeError::Invalid("a message's file names no message"))?;
        self.message_conversation_id(message.as_str())?
            .map(Some)
            .ok_or(OfficeError::Invalid(
                "a message's file names a message in no conversation",
            ))
    }

    /// A file bound to a message or an ask answer, as the attachment record the policy reads:
    /// a message's file inherits the message's read rule, an answer's the ask's.
    pub(super) fn message_file_record(
        &self,
        attachment_id: &str,
    ) -> Result<Option<AttachmentRecord>, OfficeError> {
        let read = MessageFileById(attachment_id.to_owned());
        let Some(bound) = self.store.read(read).map_err(storage)?.0 else {
            return Ok(None);
        };
        let file = bound.file;
        let uploader = file
            .uploaded_by_employee_id
            .clone()
            .or_else(|| bound.answerer.clone());
        let (Ok(id), Ok(target), Some(uploader), Some(hash)) = (
            Id::parse(&file.id),
            Id::parse(&bound.message_id),
            uploader,
            ContentHash::parse(&file.content_sha256),
        ) else {
            // A file this phase did not bind (no uploader, no canonical IDs) is not served.
            return Ok(None);
        };
        let filename = file.filename.clone().unwrap_or_else(|| file.id.clone());
        let created_at = file.created_at.clone().unwrap_or_default();
        Ok(Some(AttachmentRecord {
            id,
            target_kind: if bound.answerer.is_some() {
                AttachmentTargetKind::AskAnswer
            } else {
                AttachmentTargetKind::Message
            },
            target_id: Some(target),
            content_sha256: hash,
            mime_type: file.media_type,
            byte_count: file.byte_count,
            title: file.title.unwrap_or_else(|| filename.clone()),
            filename,
            description: file.description,
            artifact_status: file.artifact_status,
            width: file.width,
            height: file.height,
            page_count: file.page_count,
            uploaded_by_employee_id: uploader,
            updated_at: created_at.clone(),
            created_at,
        }))
    }
}
