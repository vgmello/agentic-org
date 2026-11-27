//! `attachment.attach` and `attachment.update_meta`: binding a committed upload, or re-attaching
//! a file the caller may read, to one target under a fresh `attachment_id`; and editing a
//! standalone file's title, description and status.

use std::sync::Arc;

use org_proto::attachments::{
    ArtifactStatus, AttachmentAttachParams, AttachmentResult, AttachmentScope,
    AttachmentTargetKind, AttachmentUpdateMetaParams, AttachmentView,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Outcome};
use serde_json::json;

use super::{hq_only, invalid, not_found, record_ref, Answer, Office};
use crate::attachments::blobs::{BlobStore, ContentHash};
use crate::attachments::media::MediaType;
use crate::attachments::records::{is_description, is_title, AttachmentRecord, MetaPatch};
use crate::attachments::remote::bytes_unavailable;
use crate::attachments::user_space::{operation_reused, request_fingerprint, satellite_refusal};
use crate::authz::{PrincipalContext, ResourceRef};
use crate::error::OfficeError;
use crate::office::storage;
use crate::office::tasks::ensure_local_task_write_authority;
use crate::store::open::attachment_ops::{
    attachment_operation, AttachmentWrite, AttachmentWriteOp,
};
use crate::store::open::task_ops::{TaskAttachOp, TaskUpdateAttachmentMetaOp};
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
            "attaching to a `{}` is not available through attachment.attach: bind project, organization, task and private files here",
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
            AttachmentTargetKind::Task => {
                let id = parse(target_id)?;
                Ok(Target {
                    kind,
                    resource: ResourceRef::Task(id.to_string()),
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
        if record.scope() == AttachmentScope::UserPrivate && self.attachments.private.is_none() {
            return Ok(Err(satellite_refusal()));
        }
        let Some(bytes) = self.local_blob_all(&record, media.max_bytes())? else {
            return Ok(Err(bytes_unavailable(
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
    /// A private command is looked up in the caller's own space alone, and answers only the
    /// same request for the same target: anything else is `operation_reused`.
    fn attach_retry(
        &self,
        caller: &PrincipalContext,
        params: &AttachmentAttachParams,
        target: &Target,
        fingerprint: &[u8],
    ) -> Answer<Option<AttachmentView>> {
        if params.target_kind == AttachmentTargetKind::UserPrivate {
            let Some(earlier) = self.attachments.user_space.by_operation(
                &self.info.org_id,
                &caller.employee_id,
                &params.command_id,
            )?
            else {
                return Ok(Ok(None));
            };
            if earlier.record.target_id != target.id
                || earlier.record.uploaded_by_employee_id.as_str() != caller.employee_id
            {
                return Ok(Err(operation_reused()));
            }
            return Ok(earlier
                .replay(&request_fingerprint(fingerprint))
                .map(|record| Some(record.to_view())));
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
            return Ok(Err(operation_reused()));
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
        // A satellite holds no private space: refused before anything else, the same for
        // every caller. Its shared files wait in its outbox for HQ (`TASK-98.6`).
        if self.is_satellite() && params.target_kind == AttachmentTargetKind::UserPrivate {
            return Ok(Err(satellite_refusal()));
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
        // A replay of a command already answered gets that answer and binds nothing again:
        // no bytes are read or written and nothing enters the outbox. An upload a crash left
        // staged after its binding committed goes now.
        match self.attach_retry(&caller, &params, &target, &fingerprint)? {
            Ok(Some(view)) => {
                self.drop_bound_upload(&caller, params.upload_id.as_deref())?;
                return Ok(Ok(AttachmentResult { attachment: view }));
            }
            Ok(None) => {}
            Err(reused) => return Ok(Err(reused)),
        }
        let source = match self.attach_source(&caller, &params)? {
            Ok(source) => source,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if let Err(refusal) =
            self.decide(|policy| policy.can_attach(&caller, &source.resource, &target.resource))?
        {
            return Ok(Err(refusal));
        }
        self.bind(&caller, &params, target, source, &fingerprint)
    }

    /// Removes the caller's staged upload `upload_id`, which a replayed binding already bound.
    fn drop_bound_upload(
        &self,
        caller: &PrincipalContext,
        upload_id: Option<&str>,
    ) -> Result<(), OfficeError> {
        if let Some(upload_id) = upload_id {
            if self.staged_for(caller, upload_id)?.is_ok() {
                self.attachments.staging.remove(upload_id)?;
            }
        }
        Ok(())
    }

    /// Where a new binding's bytes go on this office: HQ's shared or private store, or (on a
    /// satellite, for a shared file) nowhere yet: they wait in the outbox with the record.
    fn binding_store(&self, private: bool) -> Result<Option<Arc<dyn BlobStore>>, ApiError> {
        if private {
            return match &self.attachments.private {
                Some(store) => Ok(Some(Arc::clone(store))),
                None => Err(satellite_refusal()),
            };
        }
        Ok((!self.is_satellite()).then(|| Arc::clone(&self.attachments.shared)))
    }

    fn check_task_bind_authority(&self, target: &Target) -> Answer<()> {
        if self.is_satellite() {
            return Ok(Err(ApiError::new(
                ErrorCode::Unavailable,
                "attaching files to tasks is handled at HQ; retry against the HQ office",
            )));
        }
        if let Some(tid) = target.id.as_ref() {
            return ensure_local_task_write_authority(self, tid.as_str());
        }
        Ok(Ok(()))
    }

    fn persist_bound_record(
        &self,
        private: bool,
        is_task: bool,
        caller: &PrincipalContext,
        params: &AttachmentAttachParams,
        fingerprint: &[u8],
        record: AttachmentRecord,
    ) -> Answer<AttachmentView> {
        if private {
            return Ok(self
                .attachments
                .user_space
                .insert(
                    &self.info.org_id,
                    &params.command_id,
                    &request_fingerprint(fingerprint),
                    &record,
                )?
                .map(|r| r.to_view()));
        }
        let Some(operation) =
            attachment_operation(&params.command_id, ATTACH, &caller.employee_id, fingerprint)
        else {
            return Ok(Err(invalid(
                "command_id must be a canonical lowercase UUIDv7",
            )));
        };
        if is_task {
            return self
                .store
                .run(TaskAttachOp {
                    operation,
                    actor: caller.employee_id.clone(),
                    command_id: params.command_id.clone(),
                    record,
                })
                .map_err(storage);
        }
        self.store
            .run(AttachmentWriteOp {
                operation,
                write: AttachmentWrite::Create(record),
            })
            .map_err(storage)
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
        let is_task = target.kind == AttachmentTargetKind::Task;
        if is_task {
            if let Err(err) = self.check_task_bind_authority(&target)? {
                return Ok(Err(err));
            }
        }
        let store = match self.binding_store(private) {
            Ok(store) => store,
            Err(refusal) => return Ok(Err(refusal)),
        };
        // The bytes are durable before any record names them. Should the record then be
        // refused (the project archived meanwhile, a reused command), the blob stays
        // unreferenced and unreadable: every read goes through a record and the policy.
        let hash = match &store {
            Some(blobs) => blobs.put(&source.bytes)?,
            None => ContentHash::of(&source.bytes),
        };
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
        // A satellite's shared file: bytes and record wait durably in the outbox, readable
        // here at once and offline, until HQ verifies and acks them.
        let outboxed = store.is_none() && record.scope() == AttachmentScope::Shared;
        if outboxed {
            self.outbox().enqueue(&record, &source.bytes)?;
        }
        let record_id = record.id.clone();
        let bound =
            self.persist_bound_record(private, is_task, caller, params, fingerprint, record)?;
        // Only this call's own record ships: a refused binding was never made, and a replay
        // (the same command answered concurrently) answered with its earlier record.
        if outboxed && !matches!(&bound, Ok(view) if view.id == record_id.as_str()) {
            self.outbox().release(&record_id)?;
        }
        // A refused binding (a reused command, an archived project) leaves the upload staged.
        if bound.is_ok() {
            if let Some(upload_id) = &source.upload_id {
                self.attachments.staging.remove(upload_id)?;
            }
        }
        Ok(bound.map(|attachment| AttachmentResult { attachment }))
    }

    fn validate_update_meta_params(params: &AttachmentUpdateMetaParams) -> Result<(), ApiError> {
        if !org_proto::is_uuid_v7(&params.command_id) {
            return Err(invalid("command_id must be a canonical lowercase UUIDv7"));
        }
        if params.title.is_none()
            && params.description.is_none()
            && params.artifact_status.is_none()
        {
            return Err(invalid(
                "name a title, description or artifact_status to change",
            ));
        }
        check_meta(params.title.as_deref(), params.description.as_deref())
    }

    fn resolve_updatable_attachment(
        &self,
        caller: &PrincipalContext,
        attachment_id: &str,
    ) -> Answer<AttachmentRecord> {
        let Some(record) = self.find_attachment(attachment_id)? else {
            return Ok(Err(not_found()));
        };
        let resource = record_ref(&record);
        if let Err(refusal) = self.decide(|policy| policy.can_write(caller, &resource))? {
            return Ok(Err(refusal));
        }
        let kind = record.target_kind;
        if !matches!(
            kind,
            AttachmentTargetKind::Project
                | AttachmentTargetKind::Organization
                | AttachmentTargetKind::Task
                | AttachmentTargetKind::UserPrivate
        ) {
            return Ok(Err(unsupported_target(kind)));
        }
        if kind == AttachmentTargetKind::Task {
            if let Some(tid) = record.target_id.as_ref() {
                if let Err(err) = ensure_local_task_write_authority(self, tid.as_str())? {
                    return Ok(Err(err));
                }
            }
        }
        Ok(Ok(record))
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
        if let Err(refusal) = Self::validate_update_meta_params(&params) {
            return Ok(Err(refusal));
        }
        let record = match self.resolve_updatable_attachment(&caller, &params.attachment_id)? {
            Ok(record) => record,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let kind = record.target_kind;
        // Only the named fields travel: each is applied where the write is decided, onto the
        // file as it is then.
        let patch = MetaPatch {
            title: params.title.clone(),
            description: params.description.clone(),
            artifact_status: params.artifact_status,
            updated_at: format_rfc3339_ms(self.attachment_now_ms()),
        };
        if kind == AttachmentTargetKind::UserPrivate {
            let updated = self.attachments.user_space.update_meta(
                &self.info.org_id,
                &caller.employee_id,
                record.id.as_str(),
                &patch,
            )?;
            return Ok(match updated {
                Some(updated) => Ok(AttachmentResult {
                    attachment: updated.to_view(),
                }),
                None => Err(not_found()),
            });
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
        let written = if kind == AttachmentTargetKind::Task {
            self.store
                .run(TaskUpdateAttachmentMetaOp {
                    operation,
                    actor: caller.employee_id.clone(),
                    command_id: params.command_id.clone(),
                    attachment_id: record.id,
                    patch,
                })
                .map_err(storage)?
        } else {
            self.store
                .run(AttachmentWriteOp {
                    operation,
                    write: AttachmentWrite::Update {
                        id: record.id,
                        patch,
                    },
                })
                .map_err(storage)?
        };
        Ok(written.map(|attachment| AttachmentResult { attachment }))
    }
}
