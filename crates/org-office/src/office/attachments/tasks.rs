//! Files on tasks and task comments (`TASK-98.4`, ADR-0037): the attachment facade's side of
//! `tasks.create`, `tasks.update` (`add_attachments`) and `tasks.comment`.
//!
//! Each ID named in `attachment_ids` / `add_attachments` may be either:
//! - one of the caller's committed staged uploads (from `attachment.upload_*` / `--attach`), or
//! - an existing attachment ID the caller is authorized to read (`--attachment-id`, re-attached
//!   with a fresh binding ID under the task or comment).
//!
//! Bytes are promoted write-once into `<durable>/blobs/sha256/` before the journal commit, and
//! staged uploads are removed only after the commit succeeds. If an ID is missing (for instance
//! on an idempotent retry whose staged upload was already cleaned up), [`PreparedTaskFiles`]
//! marks `missing = true` so `Store::submit` replays an existing receipt while any fresh commit
//! refuses with `not_found`.

use org_proto::attachments::{check_attachment_ids_limit, ArtifactStatus, AttachmentScope};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use super::{record_ref, Answer, Office};
use crate::attachments::blobs::ContentHash;
use crate::attachments::media::{self, MediaType};
use crate::attachments::remote::bytes_unavailable;
use crate::attachments::user_space::satellite_refusal;
use crate::authz::{PrincipalContext, ResourceRef};
use crate::error::OfficeError;
use crate::tasks::features::{PreparedTaskFile, PreparedTaskFiles};

struct ResolvedTaskBlob {
    file: PreparedTaskFile,
    bytes: Vec<u8>,
}

fn default_title(filename: &str) -> String {
    let title: String = filename.chars().take(160).collect();
    title.trim().to_owned()
}

impl Office {
    /// Validates, authorizes, and promotes bytes for attachments named on `tasks.create`,
    /// `tasks.update` (`add_attachments`), or `tasks.comment`.
    pub(crate) fn prepare_task_files(
        &self,
        actor: &str,
        target_task_ref: Option<&str>,
        ids: &[String],
        max: usize,
        owner_label: &str,
    ) -> Answer<PreparedTaskFiles> {
        if ids.is_empty() {
            return Ok(Ok(PreparedTaskFiles::none()));
        }
        if let Err(refusal) = check_attachment_ids_limit(ids, max, owner_label) {
            return Ok(Err(refusal));
        }
        if self.is_satellite() {
            return Ok(Err(ApiError::new(
                ErrorCode::Unavailable,
                "attaching files to tasks or comments is handled at HQ; retry against the HQ office",
            )));
        }
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let target_resource = match target_task_ref {
            Some(reference) => match self.task_inspect(reference)? {
                Ok(found) => Some(ResourceRef::Task(found.task.id)),
                Err(err) if err.code == ErrorCode::NotFound => None,
                Err(err) => return Ok(Err(err)),
            },
            None => None,
        };
        let uploader_id =
            Id::parse(&caller.employee_id).map_err(|_| OfficeError::Invalid("a caller id"))?;

        let mut resolved = Vec::with_capacity(ids.len());
        for source_id in ids {
            match self.resolve_one_task_file(
                &caller,
                &uploader_id,
                source_id,
                target_resource.as_ref(),
            )? {
                Ok(Some(item)) => resolved.push(item),
                Ok(None) => {
                    return Ok(Ok(PreparedTaskFiles {
                        files: Vec::new(),
                        missing: true,
                    }));
                }
                Err(refusal) => return Ok(Err(refusal)),
            }
        }

        // Promote every validated blob into HQ's shared blob store before the journal commit.
        let mut files = Vec::with_capacity(resolved.len());
        for item in resolved {
            let hash = self.attachments.shared.put(&item.bytes)?;
            if hash != item.file.content_sha256 {
                return Err(OfficeError::Invalid(
                    "an attachment blob changed before bind",
                ));
            }
            files.push(item.file);
        }
        Ok(Ok(PreparedTaskFiles {
            files,
            missing: false,
        }))
    }

    fn resolve_one_task_file(
        &self,
        caller: &PrincipalContext,
        uploader_id: &Id,
        source_id: &str,
        target: Option<&ResourceRef>,
    ) -> Answer<Option<ResolvedTaskBlob>> {
        match self.staged_for(caller, source_id)? {
            Ok(upload) => {
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
                let source_ref = ResourceRef::StagedUpload {
                    id: upload.upload_id.clone(),
                    uploaded_by: upload.uploaded_by.clone(),
                };
                if let Err(refusal) = self.authorize_task_source(caller, &source_ref, target)? {
                    return Ok(Err(refusal));
                }
                let bytes = self.attachments.staging.kept_bytes(&upload)?;
                if bytes.len() as u64 > media.max_bytes() {
                    return Ok(Err(media::too_large(media)));
                }
                let hash = ContentHash::of(&bytes);
                if hash.as_str() != committed.content_sha256 {
                    return Err(OfficeError::Invalid("a staged upload changed after commit"));
                }
                let title = default_title(&upload.filename);
                return Ok(Ok(Some(ResolvedTaskBlob {
                    file: PreparedTaskFile {
                        source_id: source_id.to_owned(),
                        upload_id: Some(upload.upload_id),
                        content_sha256: hash,
                        mime_type: committed.mime_type,
                        byte_count: bytes.len() as u64,
                        filename: upload.filename,
                        title,
                        description: None,
                        artifact_status: ArtifactStatus::Final,
                        width: committed.width,
                        height: committed.height,
                        page_count: committed.page_count,
                        uploaded_by_employee_id: uploader_id.clone(),
                    },
                    bytes,
                })));
            }
            Err(refusal) if refusal.code == ErrorCode::NotFound => {}
            Err(refusal) => return Ok(Err(refusal)),
        }

        let Some(existing) = self.find_attachment(source_id)? else {
            return Ok(Ok(None));
        };
        let source_ref = record_ref(&existing);
        if let Err(refusal) = self.authorize_task_source(caller, &source_ref, target)? {
            return Ok(Err(refusal));
        }
        let media = MediaType::from_mime(&existing.mime_type)
            .ok_or(OfficeError::Invalid("an attachment names an unknown type"))?;
        if existing.scope() == AttachmentScope::UserPrivate && self.attachments.private.is_none() {
            return Ok(Err(satellite_refusal()));
        }
        let Some(bytes) = self.local_blob_all(&existing, media.max_bytes())? else {
            return Ok(Err(bytes_unavailable(
                "the attachment's bytes are not on this office",
            )));
        };
        if ContentHash::of(&bytes) != existing.content_sha256 {
            return Err(OfficeError::Invalid(
                "an attachment blob does not match its hash",
            ));
        }
        Ok(Ok(Some(ResolvedTaskBlob {
            file: PreparedTaskFile {
                source_id: source_id.to_owned(),
                upload_id: None,
                content_sha256: existing.content_sha256,
                mime_type: existing.mime_type,
                byte_count: bytes.len() as u64,
                filename: existing.filename,
                title: existing.title,
                description: existing.description,
                artifact_status: existing.artifact_status,
                width: existing.width,
                height: existing.height,
                page_count: existing.page_count,
                uploaded_by_employee_id: uploader_id.clone(),
            },
            bytes,
        })))
    }

    fn authorize_task_source(
        &self,
        caller: &PrincipalContext,
        source: &ResourceRef,
        target: Option<&ResourceRef>,
    ) -> Answer<()> {
        match target {
            Some(target) => self.decide(|policy| policy.can_attach(caller, source, target)),
            None => match source {
                ResourceRef::StagedUpload { .. } => {
                    self.decide(|policy| policy.can_write(caller, source))
                }
                _ => self.decide(|policy| policy.can_read(caller, source)),
            },
        }
    }

    /// Removes staged uploads bound by a committed task or comment write.
    pub(crate) fn release_task_files(&self, files: &PreparedTaskFiles) {
        for file in &files.files {
            if let Some(upload_id) = &file.upload_id {
                let _ = self.attachments.staging.remove(upload_id);
            }
        }
    }
}
