//! Office facade methods for isolated per-author private notes, plans and decisions
//! (`TASK-50`, `TASK-111` ADEC-1/2, `L8`, `ADR-0031` TASKAUTH-4..7, `doc-79` PN-01..06).

use std::sync::atomic::Ordering;

use org_proto::private_notes::{
    PrivateNoteAddParams, PrivateNoteListParams, PrivateNoteUpdateParams, TaskNotePublishResult,
    TaskNoteRemovedResult, TaskNoteResult, TasksNotesGetParams, TasksNotesListResult,
    TasksNotesPublishParams, TasksNotesRemoveParams,
};
use org_proto::tasks::TasksCommentParams;
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus};
use crate::private_notes::query::{normalize_since, page_limit};
use crate::private_notes::{
    delete_digest, format_receipt_error_code, normalize_tag, publish_digest, BeginPublishCmd,
    CreateNoteCmd, DeleteNoteCmd, DigestInput, EditNoteCmd, FieldSet, NoteContent, NoteCursor,
    NotePatch, NoteQuery, PrivateNoteRecord, PublishAttempt, SatelliteRefusedPrivateNotesStore,
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

fn require_optional_uuid(raw: Option<&str>, field: &str) -> Result<(), ApiError> {
    raw.map_or(Ok(()), |id| require_uuid(id, field))
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

/// Unwraps an inner refusal: `Ok(Err(refusal))` returns it from the enclosing facade method.
macro_rules! refused {
    ($outcome:expr) => {
        match $outcome {
            Ok(value) => value,
            Err(refusal) => return Ok(Err(refusal)),
        }
    };
}

/// The task and project a note belongs to.
struct NoteScope {
    task_id: Option<String>,
    project_id: Option<String>,
}

/// A validated `tasks.notes.list` request: the filters a [`NoteQuery`] borrows.
struct ListFilters {
    task_id: Option<String>,
    tag: Option<String>,
    since: Option<String>,
    after: Option<NoteCursor>,
    limit: usize,
    fields: FieldSet,
}

impl ListFilters {
    fn validate(params: &PrivateNoteListParams) -> Result<Self, ApiError> {
        require_optional_uuid(params.project_id.as_deref(), "project_id")?;
        require_optional_uuid(params.author_id.as_deref(), "author_id")?;
        Ok(Self {
            task_id: None,
            tag: params.tag.as_deref().map(normalize_tag).transpose()?,
            since: params.since.as_deref().map(normalize_since).transpose()?,
            after: params
                .cursor
                .as_deref()
                .map(NoteCursor::parse)
                .transpose()?,
            limit: page_limit(params.limit)?,
            fields: FieldSet::requested(&params.fields),
        })
    }
}

impl Office {
    fn monotonic_private_now(&self) -> String {
        let raw = self.clock.now().0;
        let prev = self.clock_high_ms.fetch_max(raw, Ordering::Relaxed);
        format_rfc3339_ms(raw.max(prev))
    }

    /// `ADR-0031`: only active members read or write private notes; returns the caller and
    /// whether it is a person. Every `tasks.notes.*` call starts here, so a satellite, which
    /// holds no private notes, refuses them all with `Unavailable` before anything else.
    fn require_active_private_caller(
        &self,
        actor: &str,
    ) -> Result<Result<(Employee, bool), ApiError>, OfficeError> {
        if !self.private_notes.available() {
            return Ok(Err(SatelliteRefusedPrivateNotesStore::refused()));
        }
        let Some(emp) = self.load_employee(actor)? else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not a member of this organization",
            )));
        };
        if emp.status != EmployeeStatus::Active {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "inactive or paused employees cannot access private notes",
            )));
        }
        let is_person = emp.kind == EmployeeKind::Person;
        Ok(Ok((emp, is_person)))
    }

    /// The task a reference names, with its project.
    fn resolve_task(&self, task_ref: &str) -> Result<Result<NoteScope, ApiError>, OfficeError> {
        let inspected = refused!(self.task_inspect(task_ref)?);
        Ok(Ok(NoteScope {
            task_id: Some(inspected.task.id),
            project_id: inspected.task.project_id,
        }))
    }

    /// A new note's task and project: a task note belongs to its task's project.
    fn note_scope(
        &self,
        task: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<Result<NoteScope, ApiError>, OfficeError> {
        refused!(require_optional_uuid(project_id, "project_id"));
        let Some(task) = task else {
            return Ok(Ok(NoteScope {
                task_id: None,
                project_id: project_id.map(str::to_owned),
            }));
        };
        let scope = refused!(self.resolve_task(task)?);
        if project_id.is_some() && project_id != scope.project_id.as_deref() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "`project_id` must be the task's project",
            )));
        }
        Ok(Ok(scope))
    }

    /// `tasks.notes.add` (`PN-01`, ADEC-1): creates a private note, plan or decision authored
    /// by the authenticated caller, on a task, a project, or neither.
    pub fn task_note_add(
        &self,
        actor: &str,
        params: PrivateNoteAddParams,
    ) -> Result<Result<TaskNoteResult, ApiError>, OfficeError> {
        let (caller, _) = refused!(self.require_active_private_caller(actor)?);
        refused!(require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id")));
        let content = refused!(NoteContent::from_add(&params));
        let scope =
            refused!(self.note_scope(params.task.as_deref(), params.project_id.as_deref())?);
        let mut digest = DigestInput::new("create");
        digest
            .text(scope.task_id.as_deref())
            .text(scope.project_id.as_deref())
            .text(Some(&params.note_id));
        content.digest_into(&mut digest);
        let now = self.monotonic_private_now();
        let record = refused!(self.private_notes.create_note(CreateNoteCmd {
            org_id: &self.info.org_id,
            author_id: caller.id.as_str(),
            task_id: scope.task_id.as_deref(),
            project_id: scope.project_id.as_deref(),
            note_id: &params.note_id,
            content: &content,
            operation_id: &params.command_id,
            request_sha256: &digest.finish(),
            now: &now,
        })?);
        Ok(Ok(TaskNoteResult {
            note: record.to_wire(caller.id.as_str()),
        }))
    }

    /// `tasks.notes.list` (`PN-02`, `TASKAUTH-4/5`, ADEC-2): one newest-first page of the
    /// caller's visible active notes, across tasks unless `task` narrows it. An agent sees only
    /// its own notes, whatever team it leads or task it holds.
    pub fn task_note_list(
        &self,
        actor: &str,
        params: PrivateNoteListParams,
    ) -> Result<Result<TasksNotesListResult, ApiError>, OfficeError> {
        let (caller, is_person) = refused!(self.require_active_private_caller(actor)?);
        let mut filters = refused!(ListFilters::validate(&params));
        if let Some(task) = params.task.as_deref() {
            filters.task_id = refused!(self.resolve_task(task)?).task_id;
        }
        let page = self.private_notes.list_notes(&NoteQuery {
            org_id: &self.info.org_id,
            caller_id: caller.id.as_str(),
            caller_is_person: is_person,
            task_id: filters.task_id.as_deref(),
            project_id: params.project_id.as_deref(),
            kind: params.kind,
            tag: filters.tag.as_deref(),
            since: filters.since.as_deref(),
            author_id: params.author_id.as_deref(),
            after: filters.after.as_ref(),
            limit: filters.limit,
        })?;
        Ok(Ok(TasksNotesListResult {
            task_id: filters.task_id,
            notes: page
                .notes
                .iter()
                .map(|r| r.to_wire_with(caller.id.as_str(), filters.fields))
                .collect(),
            total: page.total,
            next_cursor: page.next_cursor.as_ref().map(NoteCursor::encode),
        }))
    }

    /// `tasks.notes.get` (`PN-02`, `TASKAUTH-4/5`): fetches one visible active note by ID.
    /// Another agent receives `NotFound` regardless of whether the note exists.
    pub fn task_note_get(
        &self,
        actor: &str,
        params: TasksNotesGetParams,
    ) -> Result<Result<TaskNoteResult, ApiError>, OfficeError> {
        let (caller, is_person) = refused!(self.require_active_private_caller(actor)?);
        refused!(require_uuid(&params.note_id, "note_id"));
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

    /// `tasks.notes.update` (`PN-01`, `TASKAUTH-6`, ADEC-1): author-only revision-checked
    /// patch of the kind, title, why, alternatives, tags or body.
    pub fn task_note_update(
        &self,
        actor: &str,
        params: PrivateNoteUpdateParams,
    ) -> Result<Result<TaskNoteResult, ApiError>, OfficeError> {
        let (caller, is_person) = refused!(self.require_active_private_caller(actor)?);
        refused!(require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id"))
            .and_then(|()| require_revision(params.expected_revision).map(|_| ())));
        let patch = refused!(NotePatch::from_update(&params));
        let mut digest = DigestInput::new("edit");
        digest
            .text(Some(&params.note_id))
            .number(params.expected_revision);
        patch.digest_into(&mut digest);
        let now = self.monotonic_private_now();
        let record = refused!(self.private_notes.edit_note(EditNoteCmd {
            org_id: &self.info.org_id,
            caller_id: caller.id.as_str(),
            caller_is_person: is_person,
            note_id: &params.note_id,
            expected_revision: params.expected_revision,
            patch: &patch,
            operation_id: &params.command_id,
            request_sha256: &digest.finish(),
            now: &now,
        })?);
        Ok(Ok(TaskNoteResult {
            note: record.to_wire(caller.id.as_str()),
        }))
    }

    /// `tasks.notes.remove` (`PN-01`, `PN-06`, `TASKAUTH-6`): author-only revisioned tombstone
    /// clearing every private text field and tag.
    pub fn task_note_remove(
        &self,
        actor: &str,
        params: TasksNotesRemoveParams,
    ) -> Result<Result<TaskNoteRemovedResult, ApiError>, OfficeError> {
        let (caller, is_person) = refused!(self.require_active_private_caller(actor)?);
        refused!(require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id"))
            .and_then(|()| require_revision(params.expected_revision).map(|_| ())));
        let digest = delete_digest(&params.note_id, params.expected_revision);
        let now = self.monotonic_private_now();
        let record = refused!(self.private_notes.delete_note(DeleteNoteCmd {
            org_id: &self.info.org_id,
            caller_id: caller.id.as_str(),
            caller_is_person: is_person,
            note_id: &params.note_id,
            expected_revision: params.expected_revision,
            operation_id: &params.command_id,
            request_sha256: &digest,
            now: &now,
        })?);
        Ok(Ok(TaskNoteRemovedResult {
            note_id: record.id,
            task_id: record.task_id,
            revision: record.revision,
            deleted_at: record.deleted_at.unwrap_or(record.updated_at),
        }))
    }

    /// `tasks.notes.publish` (`PN-05`): explicit author-only 2-phase publish-copy of a task
    /// note to a shared task comment while preserving the private original unchanged. A note
    /// outside any task is refused (`InvalidRequest`).
    /// Returns `(result, replayed)` so callers suppress live event notifications on replay.
    pub fn task_note_publish(
        &self,
        actor: &str,
        params: TasksNotesPublishParams,
    ) -> Result<Result<(TaskNotePublishResult, bool), ApiError>, OfficeError> {
        let (caller, is_person) = refused!(self.require_active_private_caller(actor)?);
        refused!(require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id"))
            .and_then(|()| require_uuid(&params.comment_id, "comment_id"))
            .and_then(|()| require_revision(params.expected_revision).map(|_| ())));
        let digest = publish_digest(
            &params.note_id,
            params.expected_revision,
            &params.comment_id,
        );
        let now = self.monotonic_private_now();
        let attempt = refused!(self.private_notes.begin_publish(BeginPublishCmd {
            org_id: &self.info.org_id,
            caller_id: caller.id.as_str(),
            caller_is_person: is_person,
            note_id: &params.note_id,
            expected_revision: params.expected_revision,
            comment_id: &params.comment_id,
            operation_id: &params.command_id,
            request_sha256: &digest,
            now: &now,
        })?);

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
        mut note: PrivateNoteRecord,
        result_revision: u64,
        shared_comment_id: &str,
    ) -> Result<Result<(TaskNotePublishResult, bool), ApiError>, OfficeError> {
        let Some(task_id) = note.task_id.clone() else {
            return Ok(Err(ApiError::new(
                ErrorCode::StorageError,
                "published private note lost its task",
            )));
        };
        let inspected = refused!(self.task_inspect(&task_id)?);
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
        note: PrivateNoteRecord,
        shared_comment_id: String,
    ) -> Result<Result<(TaskNotePublishResult, bool), ApiError>, OfficeError> {
        let Some(task_id) = note.task_id.clone() else {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "only a private note on a task can be published to its comments",
            )));
        };
        let body = note.body_markdown.clone().unwrap_or_default();
        // `doc-79`: use the shared comment ID as the shared idempotency key, independent of
        // the private operation ID.
        let comment_outcome = self.task_comment(
            actor,
            TasksCommentParams {
                command_id: shared_comment_id.clone(),
                task: task_id,
                comment_id: shared_comment_id,
                body_markdown: body,
                attachment_ids: vec![],
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
