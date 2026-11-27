//! Task and comment attachment preparation and direct task-attachment binding
//! (`TASK-98.4`, ADR-0037).

use org_proto::attachments::{
    ArtifactStatus, AttachmentTargetKind, AttachmentView, TASK_ATTACHMENTS_MAX,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::json;

use crate::attachments::blobs::ContentHash;
use crate::attachments::ports::AttachmentsRead;
use crate::attachments::records::{AttachmentRecord, MetaPatch};
use crate::messaging::ports::MessagingRead;
use crate::tasks::ops::{
    current_attachments, current_links, derived_id, require_local_task_write_authority,
    task_location, TaskWriteAuthority, Writer,
};
use crate::tasks::ports::TasksRead;
use crate::time::format_rfc3339_ms;

use super::{refuse, require_active_employee, require_task, storage_refusal};

/// One validated, blob-promoted file ready to bind inside a task or task-comment commit.
#[derive(Clone, Debug)]
pub struct PreparedTaskFile {
    pub source_id: String,
    /// Present when this file came from a staged upload that should be removed after commit.
    pub upload_id: Option<String>,
    pub content_sha256: ContentHash,
    pub mime_type: String,
    pub byte_count: u64,
    pub filename: String,
    pub title: String,
    pub description: Option<String>,
    pub artifact_status: ArtifactStatus,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_count: Option<u32>,
    pub uploaded_by_employee_id: Id,
}

/// The prepared files handed from [`crate::Office`] into a task or comment store decision.
#[derive(Clone, Default, Debug)]
pub struct PreparedTaskFiles {
    pub files: Vec<PreparedTaskFile>,
    /// True when a named upload/source ID was absent before the decision ran: an exact retry
    /// still replays from its receipt, while a fresh commit refuses with `not_found`.
    pub missing: bool,
}

impl PreparedTaskFiles {
    pub fn none() -> Self {
        Self::default()
    }

    /// Materializes deterministic [`AttachmentRecord`]s for `(target_kind, target_id)` under
    /// `command_id` at timestamp `at`.
    pub fn to_records(
        &self,
        target_kind: AttachmentTargetKind,
        target_id: &Id,
        command_id: &str,
        at: &str,
    ) -> Result<Vec<AttachmentRecord>, Refusal> {
        if self.missing {
            return Err(refuse(ErrorCode::NotFound, "attachment not found"));
        }
        let mut records = Vec::with_capacity(self.files.len());
        for file in &self.files {
            let id = derived_id(&[
                b"task-attachment-v1:",
                target_kind.as_str().as_bytes(),
                b":",
                target_id.as_str().as_bytes(),
                b":",
                command_id.as_bytes(),
                b":",
                file.source_id.as_bytes(),
            ]);
            records.push(AttachmentRecord {
                id,
                target_kind,
                target_id: Some(target_id.clone()),
                content_sha256: file.content_sha256.clone(),
                mime_type: file.mime_type.clone(),
                byte_count: file.byte_count,
                filename: file.filename.clone(),
                title: file.title.clone(),
                description: file.description.clone(),
                artifact_status: file.artifact_status,
                width: file.width,
                height: file.height,
                page_count: file.page_count,
                uploaded_by_employee_id: file.uploaded_by_employee_id.clone(),
                created_at: at.to_owned(),
                updated_at: at.to_owned(),
            });
        }
        Ok(records)
    }
}

/// Binds one attachment record directly to a task (`attachment.attach` with `target_kind = task`),
/// rewriting `task.md` and emitting a `task_updated` history event so projection rebuilds never
/// lose the attachment.
pub struct AttachToTask {
    pub actor_member_id: String,
    pub command_id: String,
    pub record: AttachmentRecord,
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
}

impl AttachToTask {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, AttachmentView), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let Some(task_id) = self.record.target_id.as_ref() else {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "a task attachment requires target_id",
            ));
        };
        if uow
            .view()
            .attachment(&self.record.id)
            .map_err(storage_refusal)?
            .is_some()
            || uow
                .view()
                .message_file(self.record.id.as_str())
                .map_err(storage_refusal)?
                .is_some()
        {
            return Err(refuse(ErrorCode::AlreadyExists, "attachment id in use"));
        }
        let mut task = require_task(uow.view(), task_id.as_str())?;
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
        let mut attachments = current_attachments(uow.view(), &task)?;
        if attachments.len() >= TASK_ATTACHMENTS_MAX {
            return Err(refuse(
                ErrorCode::TooLarge,
                format!("a task carries at most {TASK_ATTACHMENTS_MAX} attachments"),
            ));
        }
        attachments.push(self.record.clone());
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let location = task_location(uow.view(), &task)?;
        let links = current_links(uow.view(), &task)?;
        let change = Writer::new(uow.view(), &actor.id, &at, &self.command_id)?.task_and_event(
            Change::new(Ack::Durable),
            &location,
            &task,
            &links,
            &attachments,
            json!({
                "type": "task_updated",
                "fields": ["attachments"],
                "added_attachment_ids": [self.record.id.as_str()],
            }),
        )?;
        Ok((change, self.record.to_view()))
    }
}

/// Edits metadata (`title`, `description`, `artifact_status`) of an attachment bound to a task
/// (`attachment.update_meta`), rewriting `task.md` atomically.
pub struct UpdateTaskAttachmentMeta {
    pub actor_member_id: String,
    pub command_id: String,
    pub attachment_id: Id,
    pub patch: MetaPatch,
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
}

impl UpdateTaskAttachmentMeta {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, AttachmentView), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let Some(existing) = uow
            .view()
            .attachment(&self.attachment_id)
            .map_err(storage_refusal)?
        else {
            return Err(refuse(ErrorCode::NotFound, "attachment not found"));
        };
        if existing.target_kind != AttachmentTargetKind::Task {
            return Err(refuse(
                ErrorCode::Unsupported,
                "this attachment is not bound to a task",
            ));
        }
        let Some(task_id) = existing.target_id.as_ref() else {
            return Err(refuse(ErrorCode::NotFound, "attachment not found"));
        };
        let mut task = require_task(uow.view(), task_id.as_str())?;
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
        let mut attachments = current_attachments(uow.view(), &task)?;
        let Some(slot) = attachments.iter_mut().find(|a| a.id == self.attachment_id) else {
            return Err(refuse(ErrorCode::NotFound, "attachment not found"));
        };
        self.patch.apply(slot);
        let updated_view = slot.to_view();
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let location = task_location(uow.view(), &task)?;
        let links = current_links(uow.view(), &task)?;
        let change = Writer::new(uow.view(), &actor.id, &at, &self.command_id)?.task_and_event(
            Change::new(Ack::Durable),
            &location,
            &task,
            &links,
            &attachments,
            json!({
                "type": "task_updated",
                "fields": ["attachments"],
                "updated_attachment_id": self.attachment_id.as_str(),
            }),
        )?;
        Ok((change, updated_view))
    }
}
