//! `attachment.attach` and `attachment.update_meta`: binding a committed upload, or re-attaching
//! a file the caller may read, to one target under a fresh `attachment_id`; and editing a
//! standalone file's title, description and status.

use std::sync::Arc;

use org_proto::attachments::{
    ArtifactStatus, AttachmentAttachParams, AttachmentResult, AttachmentTargetKind,
    AttachmentUpdateMetaParams, AttachmentView,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Outcome};
use serde_json::json;

use super::{hq_only, invalid, not_found, record_ref, Answer, Office};
use crate::attachments::blobs::{BlobStore, ContentHash};
use crate::attachments::media::MediaType;
use crate::attachments::records::{is_description, is_title, AttachmentRecord};
use crate::attachments::user_space::satellite_refusal;
use crate::authz::{PrincipalContext, ResourceRef};
use crate::error::OfficeError;
use crate::office::storage;
use crate::store::open::attachment_ops::{
    attachment_operation, AttachmentWriteMode, AttachmentWriteOp,
};
use crate::store::open::ReceiptById;
use crate::time::format_rfc3339_ms;

const ATTACH: &str = "attachment.attach";
const UPDATE_META: &str = "attachment.update_meta";

/// Where a binding goes, as the call named it and as the policy sees it.
struct Target {
    kind: AttachmentTargetKind,
    id: Option<Id>,
    resource: ResourceRef,
}

/// The bytes and facts a binding takes from its source.
struct Source {
    resource: ResourceRef,
    bytes: Vec<u8>,
    media: MediaType,
    filename: String,
    width: Option<u32>,
    height: Option<u32>,
    page_count: Option<u32>,
    /// The staged upload to remove once bound.
    upload_id: Option<String>,
}

fn unsupported_target(kind: AttachmentTargetKind) -> ApiError {
    ApiError::new(
        ErrorCode::Unsupported,
        format!(
            "attaching to a `{}` is not available yet: Phase 1 binds project, organization and private files",
            kind.as_str()
        ),
    )
}

/// A title from a file name: at most 160 characters of one line.
fn default_title(filename: &str) -> String {
    let title: String = filename.chars().take(160).collect();
    title.trim().to_owned()
}

fn check_meta(title: Option<&str>, description: Option<&str>) -> Result<(), ApiError> {
    if title.is_some_and(|t| !is_title(t)) {
        return Err(invalid(
            "title: 1 to 160 characters of one line, no edge spaces",
        ));
    }
    if description.is_some_and(|d| !is_description(d)) {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            "description: at most 280 characters",
        ));
    }
    Ok(())
}

impl Office {
    fn attach_target(
        &self,
        caller: &PrincipalContext,
        kind: AttachmentTargetKind,
        target_id: Option<&str>,
    ) -> Result<Target, ApiError> {
        let parse = |text: Option<&str>| -> Result<Id, ApiError> {
            text.filter(|t| org_proto::is_uuid_v7(t))
                .and_then(|t| Id::parse(t).ok())
                .ok_or_else(|| invalid("target_id must be a canonical lowercase UUIDv7"))
        };
        match kind {
            AttachmentTargetKind::Organization => {
                if target_id.is_some() {
                    return Err(invalid("an organization-wide file names no target_id"));
                }
                Ok(Target {
                    kind,
                    id: None,
                    resource: ResourceRef::Organization,
                })
            }
            AttachmentTargetKind::Project => {
                let id = parse(target_id)?;
                Ok(Target {
                    kind,
                    resource: ResourceRef::Project(id.to_string()),
                    id: Some(id),
                })
            }
            AttachmentTargetKind::UserPrivate => {
                if !self.attachments.user_space.available() {
                    return Err(satellite_refusal());
                }
                let id = parse(Some(target_id.unwrap_or(&caller.employee_id)))?;
                Ok(Target {
                    kind,
                    resource: ResourceRef::UserPrivate {
                        owner_employee_id: id.to_string(),
                    },
                    id: Some(id),
                })
            }
            other => Err(unsupported_target(other)),
        }
    }

    /// The bytes to bind: a committed upload of the caller's, or an attachment they may read.
    fn attach_source(
        &self,
        caller: &PrincipalContext,
        params: &AttachmentAttachParams,
    ) -> Answer<Source> {
        if let Some(upload_id) = &params.upload_id {
            let upload = match self.staged_for(caller, upload_id)? {
                Ok(upload) => upload,
                Err(refusal) => return Ok(Err(refusal)),
            };
            let Some(kept) = upload.committed.clone() else {
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    "commit the upload before attaching it",
                )));
            };
            let media = kept.media().ok_or(OfficeError::Invalid(
                "a staged upload names an unknown type",
            ))?;
            return Ok(Ok(Source {
                resource: ResourceRef::StagedUpload {
                    id: upload.upload_id.clone(),
                    uploaded_by: upload.uploaded_by.clone(),
                },
                bytes: self.attachments.staging.kept_bytes(&upload)?,
                media,
                filename: upload.filename.clone(),
                width: kept.width,
                height: kept.height,
                page_count: kept.page_count,
                upload_id: Some(upload.upload_id),
            }));
        }
        let source_id = params.source_attachment_id.as_deref().unwrap_or_default();
        let Some(record) = self.find_attachment(source_id)? else {
            return Ok(Err(not_found()));
        };
        let resource = record_ref(&record);
        if let Err(refusal) = self.decide(|policy| policy.can_read(caller, &resource))? {
            return Ok(Err(refusal));
        }
        let media = MediaType::from_mime(&record.mime_type)
            .ok_or(OfficeError::Invalid("an attachment names an unknown type"))?;
        let Some(blobs) = self.blobs_for(&record) else {
            return Ok(Err(satellite_refusal()));
        };
        let Some(bytes) = blobs.read_all(&record.content_sha256, media.max_bytes())? else {
            return Ok(Err(ApiError::new(
                ErrorCode::Unavailable,
                "the attachment's bytes are not on this office",
            )));
        };
        if ContentHash::of(&bytes) != record.content_sha256 {
            return Err(OfficeError::Invalid(
                "an attachment blob does not match its hash",
            ));
        }
        Ok(Ok(Source {
            resource,
            bytes,
            media,
            filename: record.filename,
            width: record.width,
            height: record.height,
            page_count: record.page_count,
            upload_id: None,
        }))
    }

    /// The answer a retried command already got, if any (its staged upload is gone by now).
    fn attach_retry(
        &self,
        caller: &PrincipalContext,
        params: &AttachmentAttachParams,
        fingerprint: &[u8],
    ) -> Answer<Option<AttachmentView>> {
        if params.target_kind == AttachmentTargetKind::UserPrivate {
            let earlier = self
                .attachments
                .user_space
                .by_operation(&self.info.org_id, &params.command_id)?;
            return Ok(Ok(earlier
                .filter(|r| r.uploaded_by_employee_id.as_str() == caller.employee_id)
                .map(|r| r.to_view())));
        }
        let Ok(id) = Id::parse(&params.command_id) else {
            return Ok(Ok(None));
        };
        let Some(receipt) = self.store.read(ReceiptById(id)).map_err(storage)?.0 else {
            return Ok(Ok(None));
        };
        let expected =
            attachment_operation(&params.command_id, ATTACH, &caller.employee_id, fingerprint);
        if expected.is_none_or(|op| {
            op.fingerprint != receipt.fingerprint
                || op.actor != receipt.actor
                || receipt.kind != ATTACH
        }) {
            return Ok(Err(ApiError::new(
                ErrorCode::OperationReused,
                "command_id reused with a different request",
            )));
        }
        if receipt.outcome == Outcome::Rejected {
            let refusal = serde_json::from_slice::<ApiError>(&receipt.reply)
                .unwrap_or_else(|_| invalid("attach refused"));
            return Ok(Err(refusal));
        }
        let view = serde_json::from_slice::<AttachmentView>(&receipt.reply)
            .map_err(|_| OfficeError::Invalid("a corrupt attach receipt"))?;
        Ok(Ok(Some(view)))
    }

    /// `attachment.attach`.
    pub fn attachment_attach(
        &self,
        actor: &str,
        params: AttachmentAttachParams,
    ) -> Answer<AttachmentResult> {
        // A satellite holds no private space and writes no shared file: refused before
        // anything else, the same for every caller.
        if self.is_satellite() {
            return Ok(Err(match params.target_kind {
                AttachmentTargetKind::UserPrivate => satellite_refusal(),
                _ => hq_only(),
            }));
        }
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if !org_proto::is_uuid_v7(&params.command_id) {
            return Ok(Err(invalid(
                "command_id must be a canonical lowercase UUIDv7",
            )));
        }
        if params.upload_id.is_some() == params.source_attachment_id.is_some() {
            return Ok(Err(invalid(
                "name exactly one of upload_id and source_attachment_id",
            )));
        }
        if let Err(refusal) = check_meta(params.title.as_deref(), params.description.as_deref()) {
            return Ok(Err(refusal));
        }
        let target =
            match self.attach_target(&caller, params.target_kind, params.target_id.as_deref()) {
                Ok(target) => target,
                Err(refusal) => return Ok(Err(refusal)),
            };
        let fingerprint = serde_json::to_vec(&json!({
            "upload_id": params.upload_id,
            "source_attachment_id": params.source_attachment_id,
            "target_kind": params.target_kind,
            "target_id": params.target_id,
            "title": params.title,
            "description": params.description,
            "artifact_status": params.artifact_status,
        }))
        .expect("a fingerprint encodes");
        let source = match self.attach_source(&caller, &params)? {
            Ok(source) => source,
            Err(refusal) => match self.attach_retry(&caller, &params, &fingerprint)? {
                Ok(Some(view)) => return Ok(Ok(AttachmentResult { attachment: view })),
                Ok(None) => return Ok(Err(refusal)),
                Err(reused) => return Ok(Err(reused)),
            },
        };
        if let Err(refusal) =
            self.decide(|policy| policy.can_attach(&caller, &source.resource, &target.resource))?
        {
            return Ok(Err(refusal));
        }
        self.bind(&caller, &params, target, source, &fingerprint)
    }

    /// Stores the bytes write-once where the target keeps them and records the binding.
    fn bind(
        &self,
        caller: &PrincipalContext,
        params: &AttachmentAttachParams,
        target: Target,
        source: Source,
        fingerprint: &[u8],
    ) -> Answer<AttachmentResult> {
        let private = target.kind == AttachmentTargetKind::UserPrivate;
        let blobs: Arc<dyn BlobStore> = if private {
            match &self.attachments.private {
                Some(store) => Arc::clone(store),
                None => return Ok(Err(satellite_refusal())),
            }
        } else {
            Arc::clone(&self.attachments.shared)
        };
        // The bytes are durable before any record names them. Should the record then be
        // refused (the project archived meanwhile, a reused command), the blob stays
        // unreferenced and unreadable: every read goes through a record and the policy.
        let hash = blobs.put(&source.bytes)?;
        let now = format_rfc3339_ms(self.attachment_now_ms());
        let record = AttachmentRecord {
            id: Id::parse(&self.mint_attachment_id())
                .map_err(|_| OfficeError::Invalid("a minted attachment id"))?,
            target_kind: target.kind,
            target_id: target.id,
            content_sha256: hash,
            mime_type: source.media.mime().to_owned(),
            byte_count: source.bytes.len() as u64,
            filename: source.filename.clone(),
            title: params
                .title
                .clone()
                .unwrap_or_else(|| default_title(&source.filename)),
            description: params.description.clone().filter(|d| !d.is_empty()),
            artifact_status: params.artifact_status.unwrap_or_default(),
            width: source.width,
            height: source.height,
            page_count: source.page_count,
            uploaded_by_employee_id: Id::parse(&caller.employee_id)
                .map_err(|_| OfficeError::Invalid("a caller id"))?,
            created_at: now.clone(),
            updated_at: now,
        };
        let bound = if private {
            self.attachments
                .user_space
                .insert(&self.info.org_id, &params.command_id, &record)?
                .map(|r| r.to_view())
        } else {
            let Some(operation) =
                attachment_operation(&params.command_id, ATTACH, &caller.employee_id, fingerprint)
            else {
                return Ok(Err(invalid(
                    "command_id must be a canonical lowercase UUIDv7",
                )));
            };
            self.store
                .run(AttachmentWriteOp {
                    operation,
                    record,
                    mode: AttachmentWriteMode::Create,
                })
                .map_err(storage)?
        };
        if bound.is_ok() {
            if let Some(upload_id) = &source.upload_id {
                self.attachments.staging.remove(upload_id)?;
            }
        }
        Ok(bound.map(|attachment| AttachmentResult { attachment }))
    }

    /// `attachment.update_meta`.
    pub fn attachment_update_meta(
        &self,
        actor: &str,
        params: AttachmentUpdateMetaParams,
    ) -> Answer<AttachmentResult> {
        if self.is_satellite() {
            return Ok(Err(hq_only()));
        }
        let caller = match self.attachment_caller(actor)? {
            Ok(caller) => caller,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if !org_proto::is_uuid_v7(&params.command_id) {
            return Ok(Err(invalid(
                "command_id must be a canonical lowercase UUIDv7",
            )));
        }
        if params.title.is_none()
            && params.description.is_none()
            && params.artifact_status.is_none()
        {
            return Ok(Err(invalid(
                "name a title, description or artifact_status to change",
            )));
        }
        if let Err(refusal) = check_meta(params.title.as_deref(), params.description.as_deref()) {
            return Ok(Err(refusal));
        }
        let Some(record) = self.find_attachment(&params.attachment_id)? else {
            return Ok(Err(not_found()));
        };
        let resource = record_ref(&record);
        if let Err(refusal) = self.decide(|policy| policy.can_write(&caller, &resource))? {
            return Ok(Err(refusal));
        }
        let kind = record.target_kind;
        if !matches!(
            kind,
            AttachmentTargetKind::Project
                | AttachmentTargetKind::Organization
                | AttachmentTargetKind::UserPrivate
        ) {
            return Ok(Err(unsupported_target(kind)));
        }
        let mut updated = record;
        if let Some(title) = &params.title {
            updated.title = title.clone();
        }
        if let Some(description) = &params.description {
            updated.description = (!description.is_empty()).then(|| description.clone());
        }
        updated.artifact_status = params.artifact_status.unwrap_or(updated.artifact_status);
        updated.updated_at = format_rfc3339_ms(self.attachment_now_ms());
        if kind == AttachmentTargetKind::UserPrivate {
            self.attachments
                .user_space
                .update(&self.info.org_id, &updated)?;
            return Ok(Ok(AttachmentResult {
                attachment: updated.to_view(),
            }));
        }
        let fingerprint = serde_json::to_vec(&json!({
            "attachment_id": params.attachment_id,
            "title": params.title,
            "description": params.description,
            "artifact_status": params.artifact_status.map(ArtifactStatus::as_str),
        }))
        .expect("a fingerprint encodes");
        let Some(operation) = attachment_operation(
            &params.command_id,
            UPDATE_META,
            &caller.employee_id,
            &fingerprint,
        ) else {
            return Ok(Err(invalid(
                "command_id must be a canonical lowercase UUIDv7",
            )));
        };
        let written = self
            .store
            .run(AttachmentWriteOp {
                operation,
                record: updated,
                mode: AttachmentWriteMode::Update,
            })
            .map_err(storage)?;
        Ok(written.map(|attachment| AttachmentResult { attachment }))
    }
}
