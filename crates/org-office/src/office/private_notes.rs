//! Office facade methods for isolated per-author private notes, plans and decisions
//! (`TASK-50`, `TASK-111` ADEC-1/2, `L8`, `ADR-0031` TASKAUTH-4..7, `doc-79` PN-01..06, `TASK-220`).

use std::collections::BTreeMap;
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

type Out<T> = Result<Result<T, ApiError>, OfficeError>;

fn api_err(code: ErrorCode, msg: impl Into<String>) -> ApiError {
    ApiError::new(code, msg)
}

fn require_uuid(raw: &str, field: &str) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(raw) {
        return Err(api_err(
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
        return Err(api_err(
            ErrorCode::InvalidRequest,
            "`expected_revision` must be between 1 and i64::MAX",
        ));
    }
    Ok(rev)
}

macro_rules! refused {
    ($outcome:expr) => {
        match $outcome {
            Ok(value) => value,
            Err(refusal) => return Ok(Err(refusal)),
        }
    };
}

struct NoteScope {
    task_id: Option<String>,
    project_id: Option<String>,
}

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

    fn require_active_private_caller(&self, actor: &str) -> Out<(Employee, bool)> {
        if !self.private_notes.available() {
            return Ok(Err(SatelliteRefusedPrivateNotesStore::refused()));
        }
        let Some(emp) = self.load_employee(actor)? else {
            return Ok(Err(api_err(
                ErrorCode::NotAuthorized,
                "caller is not a member of this organization",
            )));
        };
        if emp.status != EmployeeStatus::Active {
            return Ok(Err(api_err(
                ErrorCode::NotAuthorized,
                "inactive or paused employees cannot access private notes",
            )));
        }
        let is_person = emp.kind == EmployeeKind::Person;
        Ok(Ok((emp, is_person)))
    }

    fn resolve_task(&self, task_ref: &str) -> Out<NoteScope> {
        let inspected = refused!(self.task_inspect(task_ref)?);
        Ok(Ok(NoteScope {
            task_id: Some(inspected.task.id),
            project_id: inspected.task.project_id,
        }))
    }

    fn note_scope(&self, task: Option<&str>, project_id: Option<&str>) -> Out<NoteScope> {
        refused!(require_optional_uuid(project_id, "project_id"));
        let Some(task) = task else {
            return Ok(Ok(NoteScope {
                task_id: None,
                project_id: project_id.map(str::to_owned),
            }));
        };
        let scope = refused!(self.resolve_task(task)?);
        if project_id.is_some() && project_id != scope.project_id.as_deref() {
            return Ok(Err(api_err(
                ErrorCode::InvalidRequest,
                "`project_id` must be the task's project",
            )));
        }
        Ok(Ok(scope))
    }

    /// `tasks.notes.add`: creates a private note or atomically upserts the caller's pinned checkpoint.
    pub fn task_note_add(&self, actor: &str, params: PrivateNoteAddParams) -> Out<TaskNoteResult> {
        let (caller, _) = refused!(self.require_active_private_caller(actor)?);
        refused!(require_uuid(&params.command_id, "command_id")
            .and_then(|()| require_uuid(&params.note_id, "note_id")));
        if params.checkpoint && params.task.is_none() {
            return Ok(Err(api_err(
                ErrorCode::InvalidRequest,
                "a checkpoint note must belong to a task (`--task` is required)",
            )));
        }
        let content = refused!(NoteContent::from_add(&params));
        let scope =
            refused!(self.note_scope(params.task.as_deref(), params.project_id.as_deref())?);
        let mut digest = DigestInput::new(if params.checkpoint {
            "checkpoint"
        } else {
            "create"
        });
        digest
            .text(scope.task_id.as_deref())
            .text(scope.project_id.as_deref())
            .text(Some(&params.note_id));
        content.digest_into(&mut digest);
        let now = self.monotonic_private_now();
        let sha = digest.finish();
        let cmd = CreateNoteCmd {
            org_id: &self.info.org_id,
            author_id: caller.id.as_str(),
            task_id: scope.task_id.as_deref(),
            project_id: scope.project_id.as_deref(),
            note_id: &params.note_id,
            content: &content,
            operation_id: &params.command_id,
            request_sha256: &sha,
            now: &now,
        };
        let record = refused!(if params.checkpoint {
            self.private_notes.upsert_checkpoint_note(cmd)?
        } else {
            self.private_notes.create_note(cmd)?
        });
        Ok(Ok(TaskNoteResult {
            note: record.to_wire(caller.id.as_str()),
        }))
    }

    /// `tasks.notes.list`: newest-first page of the caller's visible active notes (`ADR-0031`).
    pub fn task_note_list(
        &self,
        actor: &str,
        params: PrivateNoteListParams,
    ) -> Out<TasksNotesListResult> {
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

    /// `tasks.notes.get`: fetches one visible active note by ID (`ADR-0031`).
    pub fn task_note_get(&self, actor: &str, params: TasksNotesGetParams) -> Out<TaskNoteResult> {
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
            None => Ok(Err(api_err(ErrorCode::NotFound, "private note not found"))),
        }
    }

    /// `tasks.notes.update`: author-only revision-checked patch of a private note.
    pub fn task_note_update(
        &self,
        actor: &str,
        params: PrivateNoteUpdateParams,
    ) -> Out<TaskNoteResult> {
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

    /// `tasks.notes.remove`: author-only revisioned tombstone clearing private text and tags.
    pub fn task_note_remove(
        &self,
        actor: &str,
        params: TasksNotesRemoveParams,
    ) -> Out<TaskNoteRemovedResult> {
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

    /// `tasks.notes.publish`: explicit author-only 2-phase publish-copy to a shared comment.
    pub fn task_note_publish(
        &self,
        actor: &str,
        params: TasksNotesPublishParams,
    ) -> Out<(TaskNotePublishResult, bool)> {
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
    ) -> Out<(TaskNotePublishResult, bool)> {
        let Some(task_id) = note.task_id.clone() else {
            return Ok(Err(api_err(
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
            return Ok(Err(api_err(
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
    ) -> Out<(TaskNotePublishResult, bool)> {
        let Some(task_id) = note.task_id.clone() else {
            return Ok(Err(api_err(
                ErrorCode::InvalidRequest,
                "only a private note on a task can be published to its comments",
            )));
        };
        let outcome = self.task_comment(
            actor,
            TasksCommentParams {
                command_id: shared_comment_id.clone(),
                task: task_id,
                comment_id: shared_comment_id,
                body_markdown: note.body_markdown.clone().unwrap_or_default(),
                attachment_ids: vec![],
            },
        )?;
        let now = self.monotonic_private_now();
        match outcome {
            Ok((res, replayed)) => {
                self.private_notes.finish_publish(
                    &self.info.org_id,
                    caller_id,
                    command_id,
                    None,
                    &now,
                )?;
                Ok(Ok((
                    TaskNotePublishResult {
                        note: note.to_wire(caller_id),
                        comment: res.comment,
                        task: res.task,
                    },
                    replayed,
                )))
            }
            Err(err) if matches!(err.code, ErrorCode::StorageError | ErrorCode::Unavailable) => {
                Ok(Err(err))
            }
            Err(err) => {
                self.private_notes.finish_publish(
                    &self.info.org_id,
                    caller_id,
                    command_id,
                    Some(format_receipt_error_code(err.code)),
                    &now,
                )?;
                Ok(Err(err))
            }
        }
    }

    /// Zero-N+1 lookup of `actor`'s own latest private note for each task in `task_ids` (`ADR-0031`).
    pub fn latest_author_notes_for_tasks(
        &self,
        actor: &str,
        task_ids: &[String],
    ) -> Result<BTreeMap<String, PrivateNoteRecord>, OfficeError> {
        if task_ids.is_empty() || !self.private_notes.available() {
            return Ok(BTreeMap::new());
        }
        self.private_notes
            .latest_author_notes_for_tasks(&self.info.org_id, actor, task_ids)
    }

    /// Mirrors shared task deletion into the isolated `private_note_tasks` mirror (`PN-06`).
    pub fn private_note_mark_task_deleted(&self, task_id: &str) -> Result<(), OfficeError> {
        self.private_notes
            .mark_task_deleted(&self.info.org_id, task_id)
    }
}
