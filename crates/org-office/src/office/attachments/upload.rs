//! `attachment.upload_begin`, `upload_chunk`, `upload_commit` and `upload_abort`: uploads into
//! the uploader-only staging area, validated and sanitized on commit.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_proto::attachments::{
    AttachmentUploadAbortResult, AttachmentUploadBeginParams, AttachmentUploadBeginResult,
    AttachmentUploadChunkParams, AttachmentUploadChunkResult, AttachmentUploadCommitResult,
    AttachmentUploadRef, StagedUploadView, ATTACHMENT_CHUNK_MAX_BASE64_CHARS,
    ATTACHMENT_CHUNK_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode};

use super::{invalid, not_found, Answer, Office};
use crate::attachments::media::{self, MediaType};
use crate::attachments::staging::{NewUpload, StagedUpload};
use crate::authz::{PrincipalContext, ResourceRef};
use crate::time::format_rfc3339_ms;

fn view(upload: &StagedUpload) -> StagedUploadView {
    let committed = upload.committed.as_ref();
    StagedUploadView {
        upload_id: upload.upload_id.clone(),
        filename: upload.filename.clone(),
        mime_type: committed.map_or_else(|| upload.declared_mime.clone(), |c| c.mime_type.clone()),
        byte_count: committed.map_or(upload.declared_bytes, |c| c.byte_count),
        received_bytes: upload.received_bytes,
        committed: committed.is_some(),
        expires_at: format_rfc3339_ms(upload.expires_at_ms),
        width: committed.and_then(|c| c.width),
        height: committed.and_then(|c| c.height),
        page_count: committed.and_then(|c| c.page_count),
        content_sha256: committed.map(|c| c.content_sha256.clone()),
    }
}

impl Office {
    /// The caller's staged upload, after the policy said they may touch it. Anyone but the
    /// uploader is told it does not exist, and so is the Chief of Staff once her run acts for
    /// another member than the one she staged it for (`TASK-240`). Every chunk, commit, abort
    /// and binding of an upload (`attachment.attach` and the files of a message, ask or
    /// answer) comes through here.
    pub(super) fn staged_for(
        &self,
        caller: &PrincipalContext,
        upload_id: &str,
    ) -> Answer<StagedUpload> {
        let now = self.attachment_now_ms();
        // The policy decides; the staging area checks the uploader again under its own lock
        // in every operation that follows.
        let Some(upload) = self.attachments.staging.peek(upload_id, now)? else {
            return Ok(Err(not_found()));
        };
        let resource = ResourceRef::StagedUpload {
            id: upload.upload_id.clone(),
            uploaded_by: upload.uploaded_by.clone(),
        };
        if let Err(refusal) = self.decide(|policy| policy.can_write(caller, &resource))? {
            return Ok(Err(refusal));
        }
        if !self.attachments.staging.made_for_now(&upload) {
            return Ok(Err(not_found()));
        }
        Ok(Ok(upload))
    }

    /// `attachment.upload_begin`: checks the declared type, name and size, and reserves an
    /// upload within the caller's staging quota.
    pub fn attachment_upload_begin(
        &self,
        actor: &str,
        params: AttachmentUploadBeginParams,
    ) -> Answer<AttachmentUploadBeginResult> {
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let media = match MediaType::parse_declared(&params.mime_type) {
            Ok(media) => media,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let filename = match media::check_filename(&params.filename, media) {
            Ok(name) => name,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if params.byte_count == 0 {
            return Ok(Err(invalid("byte_count must be at least 1")));
        }
        if params.byte_count > media.max_bytes() {
            return Ok(Err(media::too_large(media)));
        }
        let upload_id = self.mint_attachment_id();
        let begun = self.attachments.staging.begin(&NewUpload {
            upload_id: &upload_id,
            uploaded_by: &caller.employee_id,
            filename: &filename,
            media,
            declared_bytes: params.byte_count,
            now_ms: self.attachment_now_ms(),
        })?;
        Ok(begun.map(|upload| AttachmentUploadBeginResult {
            upload: view(&upload),
            chunk_size_bytes: ATTACHMENT_CHUNK_MAX_BYTES,
        }))
    }

    /// `attachment.upload_chunk`: appends at most 512 KiB at the next offset.
    pub fn attachment_upload_chunk(
        &self,
        actor: &str,
        params: AttachmentUploadChunkParams,
    ) -> Answer<AttachmentUploadChunkResult> {
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if params.data_base64.len() > ATTACHMENT_CHUNK_MAX_BASE64_CHARS {
            return Ok(Err(ApiError::new(
                ErrorCode::TooLarge,
                "a chunk is at most 512 KiB",
            )));
        }
        let Ok(data) = STANDARD.decode(params.data_base64.as_bytes()) else {
            return Ok(Err(invalid("data_base64 is not standard base64")));
        };
        if data.is_empty() || data.len() > ATTACHMENT_CHUNK_MAX_BYTES as usize {
            return Ok(Err(ApiError::new(
                ErrorCode::TooLarge,
                "a chunk holds 1 byte to 512 KiB",
            )));
        }
        if let Err(refusal) = self.staged_for(&caller, &params.upload_id)? {
            return Ok(Err(refusal));
        }
        let appended = self.attachments.staging.append(
            &caller.employee_id,
            &params.upload_id,
            params.offset,
            &data,
            self.attachment_now_ms(),
        )?;
        Ok(appended.map(|upload| AttachmentUploadChunkResult {
            upload_id: upload.upload_id.clone(),
            received_bytes: upload.received_bytes,
            complete: upload.received_bytes == upload.declared_bytes,
        }))
    }

    /// `attachment.upload_commit`: validates and sanitizes the complete upload (EXIF and
    /// every other metadata block stripped, the content hash taken after), ready to attach.
    pub fn attachment_upload_commit(
        &self,
        actor: &str,
        params: AttachmentUploadRef,
    ) -> Answer<AttachmentUploadCommitResult> {
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if let Err(refusal) = self.staged_for(&caller, &params.upload_id)? {
            return Ok(Err(refusal));
        }
        let committed = self.attachments.staging.commit(
            &caller.employee_id,
            &params.upload_id,
            self.attachment_now_ms(),
            media::validate,
        )?;
        Ok(committed.map(|upload| AttachmentUploadCommitResult {
            upload: view(&upload),
        }))
    }

    /// `attachment.upload_abort`: removes the caller's staged upload.
    pub fn attachment_upload_abort(
        &self,
        actor: &str,
        params: AttachmentUploadRef,
    ) -> Answer<AttachmentUploadAbortResult> {
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if let Err(refusal) = self.staged_for(&caller, &params.upload_id)? {
            return Ok(Err(refusal));
        }
        let aborted = self.attachments.staging.abort(
            &caller.employee_id,
            &params.upload_id,
            self.attachment_now_ms(),
        )?;
        Ok(aborted.map(|()| AttachmentUploadAbortResult {
            upload_id: params.upload_id,
        }))
    }
}
