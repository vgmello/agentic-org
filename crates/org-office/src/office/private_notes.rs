//! Office facade methods for isolated per-author private task notes
//! (`TASK-50`, `L8`, `ADR-0031` TASKAUTH-4..7, `doc-79` PN-01..06).

use std::sync::atomic::Ordering;

use org_proto::private_notes::{
    TaskNotePublishResult, TaskNoteRemovedResult, TaskNoteResult, TasksNotesAddParams,
    TasksNotesGetParams, TasksNotesListParams, TasksNotesListResult, TasksNotesPublishParams,
    TasksNotesRemoveParams, TasksNotesUpdateParams,
};
use org_proto::tasks::TasksCommentParams;
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus};
use crate::private_notes::{
    format_receipt_error_code, request_digest, validate_note_body, BeginPublishCmd, CreateNoteCmd,
    DeleteNoteCmd, EditNoteCmd, PublishAttempt,
};
use crate::time::format_rfc3339_ms;

use super::Office;

fn require_uuid(raw: &str, field: &str) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(raw) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` must be a canonical lowercase UUIDv7"),
        ));
    }
    Ok(())
}

fn require_revision(rev: u64) -> Result<u64, ApiError> {
    if rev == 0 || rev > i64::MAX as u64 {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "`expected_revision` must be between 1 and i64::MAX",
        ));
    }
    Ok(rev)
}

impl Office {
    fn monotonic_private_now(&self) -> String {
        let raw = self.clock.now().0;
        let prev = self.clock_high_ms.fetch_max(raw, Ordering::Relaxed);
        format_rfc3339_ms(raw.max(prev))
    }

    fn require_active_private_caller(
        &self,
        actor: &str,
    ) -> Result<Result<Employee, ApiError>, OfficeError> {
        let Some(emp) = self.load_employee(actor)? else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not a member of this organization",
            )));
        };
        if emp.status != EmployeeStatus::Active {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "inactive or paused employees cannot access private task notes",
            )));
        }
        Ok(Ok(emp))
    }

    fn resolve_task_id(&self, task_ref: &str) -> Result<Result<String, ApiError>, OfficeError> {
        match self.task_inspect(task_ref)? {
            Ok(inspected) => Ok(Ok(inspected.task.id)),
            Err(api_err) => Ok(Err(api_err)),
        }
    }

    /// `tasks.notes.add` (`PN-01`): creates a private note authored by the authenticated caller.
    pub fn task_note_add(
        &self,
        actor: &str,
        params: TasksNotesAddParams,
    ) -> Result<Result<TaskNoteResult, ApiError>, OfficeError> {
        let caller = match self.require_active_private_caller(actor)? {
            Ok(emp) => emp,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id"))
        {
            return Ok(Err(e));
        }
        let body = match validate_note_body(&params.body_markdown) {
            Ok(b) => b,
            Err(e) => return Ok(Err(e)),
        };
        let task_id = match self.resolve_task_id(&params.task)? {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };
        let now = self.monotonic_private_now();
        let digest = request_digest(&[
            b"create\0",
            task_id.as_bytes(),
            b"\0",
            params.note_id.as_bytes(),
            b"\0",
            body.as_bytes(),
        ]);
        match self.private_notes.create_note(CreateNoteCmd {
            org_id: &self.info.org_id,
            author_id: caller.id.as_str(),
            task_id: &task_id,
            note_id: &params.note_id,
            body_markdown: &body,
            operation_id: &params.command_id,
            request_sha256: &digest,
            now: &now,
        })? {
            Ok(record) => Ok(Ok(TaskNoteResult {
                note: record.to_wire(caller.id.as_str()),
            })),
            Err(e) => Ok(Err(e)),
        }
    }

    /// `tasks.notes.list` (`PN-02`, `TASKAUTH-4/5`): lists visible active notes on a task.
    pub fn task_note_list(
        &self,
        actor: &str,
        params: TasksNotesListParams,
    ) -> Result<Result<TasksNotesListResult, ApiError>, OfficeError> {
        let caller = match self.require_active_private_caller(actor)? {
            Ok(emp) => emp,
            Err(e) => return Ok(Err(e)),
        };
        if let Some(ref aid) = params.author_id {
            if let Err(e) = require_uuid(aid, "author_id") {
                return Ok(Err(e));
            }
        }
        let task_id = match self.resolve_task_id(&params.task)? {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };
        let is_person = caller.kind == EmployeeKind::Person;
        let records = self.private_notes.list_notes(
            &self.info.org_id,
            caller.id.as_str(),
            is_person,
            &task_id,
            params.author_id.as_deref(),
        )?;
        let notes = records
            .into_iter()
            .map(|r| r.to_wire(caller.id.as_str()))
            .collect();
        Ok(Ok(TasksNotesListResult { task_id, notes }))
    }

    /// `tasks.notes.get` (`PN-02`, `TASKAUTH-4/5`): fetches one visible active note by ID.
    /// Another agent receives `NotFound` regardless of whether the note exists.
    pub fn task_note_get(
        &self,
        actor: &str,
        params: TasksNotesGetParams,
    ) -> Result<Result<TaskNoteResult, ApiError>, OfficeError> {
        let caller = match self.require_active_private_caller(actor)? {
            Ok(emp) => emp,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = require_uuid(&params.note_id, "note_id") {
            return Ok(Err(e));
        }
        let is_person = caller.kind == EmployeeKind::Person;
        match self.private_notes.get_note(
            &self.info.org_id,
            caller.id.as_str(),
            is_person,
            &params.note_id,
        )? {
            Some(record) => Ok(Ok(TaskNoteResult {
                note: record.to_wire(caller.id.as_str()),
            })),
            None => Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "private note not found",
            ))),
        }
    }

    /// `tasks.notes.update` (`PN-01`, `TASKAUTH-6`): author-only revision-checked edit.
    pub fn task_note_update(
        &self,
        actor: &str,
        params: TasksNotesUpdateParams,
    ) -> Result<Result<TaskNoteResult, ApiError>, OfficeError> {
        let caller = match self.require_active_private_caller(actor)? {
            Ok(emp) => emp,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id"))
            .and_then(|()| require_revision(params.expected_revision).map(|_| ()))
        {
            return Ok(Err(e));
        }
        let body = match validate_note_body(&params.body_markdown) {
            Ok(b) => b,
            Err(e) => return Ok(Err(e)),
        };
        let now = self.monotonic_private_now();
        let rev_bytes = params.expected_revision.to_be_bytes();
        let digest = request_digest(&[
            b"edit\0",
            params.note_id.as_bytes(),
            b"\0",
            &rev_bytes,
            b"\0",
            body.as_bytes(),
        ]);
        match self.private_notes.edit_note(EditNoteCmd {
            org_id: &self.info.org_id,
            caller_id: caller.id.as_str(),
            caller_is_person: caller.kind == EmployeeKind::Person,
            note_id: &params.note_id,
            expected_revision: params.expected_revision,
            body_markdown: &body,
            operation_id: &params.command_id,
            request_sha256: &digest,
            now: &now,
        })? {
            Ok(record) => Ok(Ok(TaskNoteResult {
                note: record.to_wire(caller.id.as_str()),
            })),
            Err(e) => Ok(Err(e)),
        }
    }

    /// `tasks.notes.remove` (`PN-01`, `PN-06`, `TASKAUTH-6`): author-only revisioned tombstone.
    pub fn task_note_remove(
        &self,
        actor: &str,
        params: TasksNotesRemoveParams,
    ) -> Result<Result<TaskNoteRemovedResult, ApiError>, OfficeError> {
        let caller = match self.require_active_private_caller(actor)? {
            Ok(emp) => emp,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id"))
            .and_then(|()| require_revision(params.expected_revision).map(|_| ()))
        {
            return Ok(Err(e));
        }
        let now = self.monotonic_private_now();
        let rev_bytes = params.expected_revision.to_be_bytes();
        let digest = request_digest(&[b"delete\0", params.note_id.as_bytes(), b"\0", &rev_bytes]);
        match self.private_notes.delete_note(DeleteNoteCmd {
            org_id: &self.info.org_id,
            caller_id: caller.id.as_str(),
            caller_is_person: caller.kind == EmployeeKind::Person,
            note_id: &params.note_id,
            expected_revision: params.expected_revision,
            operation_id: &params.command_id,
            request_sha256: &digest,
            now: &now,
        })? {
            Ok(record) => Ok(Ok(TaskNoteRemovedResult {
                note_id: record.id,
                task_id: record.task_id,
                revision: record.revision,
                deleted_at: record.deleted_at.unwrap_or(record.updated_at),
            })),
            Err(e) => Ok(Err(e)),
        }
    }

    /// `tasks.notes.publish` (`PN-05`): explicit author-only 2-phase publish-copy to a shared
    /// task comment while preserving the private original unchanged.
    /// Returns `(result, replayed)` so callers suppress live event notifications on replay.
    pub fn task_note_publish(
        &self,
        actor: &str,
        params: TasksNotesPublishParams,
    ) -> Result<Result<(TaskNotePublishResult, bool), ApiError>, OfficeError> {
        let caller = match self.require_active_private_caller(actor)? {
            Ok(emp) => emp,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id"))
            .and_then(|()| require_uuid(&params.comment_id, "comment_id"))
            .and_then(|()| require_revision(params.expected_revision).map(|_| ()))
        {
            return Ok(Err(e));
        }
        let now = self.monotonic_private_now();
        let rev_bytes = params.expected_revision.to_be_bytes();
        let digest = request_digest(&[
            b"publish\0",
            params.note_id.as_bytes(),
            b"\0",
            &rev_bytes,
            b"\0",
            params.comment_id.as_bytes(),
        ]);
        let attempt = match self.private_notes.begin_publish(BeginPublishCmd {
            org_id: &self.info.org_id,
            caller_id: caller.id.as_str(),
            caller_is_person: caller.kind == EmployeeKind::Person,
            note_id: &params.note_id,
            expected_revision: params.expected_revision,
            comment_id: &params.comment_id,
            operation_id: &params.command_id,
            request_sha256: &digest,
            now: &now,
        })? {
            Ok(a) => a,
            Err(e) => return Ok(Err(e)),
        };

        match attempt {
            PublishAttempt::AlreadyCompleted {
                note,
                result_revision,
                shared_comment_id,
            } => self.replay_completed_publish(
                caller.id.as_str(),
                note,
                result_revision,
                &shared_comment_id,
            ),
            PublishAttempt::Pending {
                note,
                shared_comment_id,
            } => self.execute_pending_publish(
                actor,
                caller.id.as_str(),
                &params.command_id,
                note,
                shared_comment_id,
            ),
        }
    }

    fn replay_completed_publish(
        &self,
        caller_id: &str,
        mut note: crate::private_notes::PrivateNoteRecord,
        result_revision: u64,
        shared_comment_id: &str,
    ) -> Result<Result<(TaskNotePublishResult, bool), ApiError>, OfficeError> {
        let inspected = match self.task_inspect(&note.task_id)? {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        };
        let Some(comment) = inspected
            .comments
            .into_iter()
            .find(|c| c.id == shared_comment_id)
        else {
            return Ok(Err(ApiError::new(
                ErrorCode::StorageError,
                "published shared comment missing from task projection",
            )));
        };
        note.revision = result_revision;
        if note.body_markdown.is_none() {
            note.body_markdown = Some(comment.body_markdown.clone());
        }
        Ok(Ok((
            TaskNotePublishResult {
                note: note.to_wire(caller_id),
                comment,
                task: inspected.task,
            },
            true,
        )))
    }

    fn execute_pending_publish(
        &self,
        actor: &str,
        caller_id: &str,
        command_id: &str,
        note: crate::private_notes::PrivateNoteRecord,
        shared_comment_id: String,
    ) -> Result<Result<(TaskNotePublishResult, bool), ApiError>, OfficeError> {
        let body = note.body_markdown.clone().unwrap_or_default();
        // `doc-79`: use the shared comment ID as the shared idempotency key, independent of
        // the private operation ID.
        let comment_outcome = self.task_comment(
            actor,
            TasksCommentParams {
                command_id: shared_comment_id.clone(),
                task: note.task_id.clone(),
                comment_id: shared_comment_id,
                body_markdown: body,
            },
        )?;
        let finish_now = self.monotonic_private_now();
        match comment_outcome {
            Ok((comment_res, replayed)) => {
                self.private_notes.finish_publish(
                    &self.info.org_id,
                    caller_id,
                    command_id,
                    None,
                    &finish_now,
                )?;
                Ok(Ok((
                    TaskNotePublishResult {
                        note: note.to_wire(caller_id),
                        comment: comment_res.comment,
                        task: comment_res.task,
                    },
                    replayed,
                )))
            }
            Err(api_err)
                if matches!(
                    api_err.code,
                    ErrorCode::StorageError | ErrorCode::Unavailable
                ) =>
            {
                // Transient shared-store failures leave the receipt `pending` for safe retry (`doc-79`).
                Ok(Err(api_err))
            }
            Err(api_err) => {
                self.private_notes.finish_publish(
                    &self.info.org_id,
                    caller_id,
                    command_id,
                    Some(format_receipt_error_code(api_err.code)),
                    &finish_now,
                )?;
                Ok(Err(api_err))
            }
        }
    }

    /// Mirrors shared task deletion into the isolated `private_note_tasks` mirror (`PN-06`).
    pub fn private_note_mark_task_deleted(&self, task_id: &str) -> Result<(), OfficeError> {
        self.private_notes
            .mark_task_deleted(&self.info.org_id, task_id)
    }
}
