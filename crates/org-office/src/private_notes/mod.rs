//! Consumer-owned port and domain contract for isolated per-author private notes, plans and
//! decisions (`TASK-50`, `TASK-111` ADEC-1/2, `L8`, `ADR-0031`, `doc-79` PN-01..06).
//!
//! Private notes NEVER enter `durable/state/`, `durable/journal/`, `office.db`, shared task
//! events, `LiveEvents`, Git, shared search, feeds, inbox, summaries, or other agents' turn
//! inputs. Only HQ's disposable private search index reads them (`TASK-91`,
//! `store/sqlite/search/private.rs`), applying this module's `ADR-0031` visibility. The
//! storage adapter (`SqlitePrivateNotesStore` in `src/store/sqlite/private_notes.rs`, or
//! [`memory::MemoryPrivateNotesStore`] in tests) owns the isolated HQ-authoritative database at
//! `<durable-root>/private/task-notes.db` (`doc-79` §Private Task Notes).

pub mod content;
pub mod memory;
pub mod query;

use std::fmt;

use org_proto::private_notes::{PrivateNoteField, PrivateNoteKind, PrivateNoteWire};
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;

use crate::error::OfficeError;

pub use content::{
    derive_title, normalize_tag, normalize_tags, validate_note_body, DigestInput, NoteContent,
    NotePatch,
};
pub use query::{FieldSet, NoteCursor, NotePage, NoteQuery};

/// One row of `private_notes` with its `private_note_tags` (`private-notes-schema.sql`).
#[derive(Clone, PartialEq, Eq)]
pub struct PrivateNoteRecord {
    pub organization_id: String,
    pub id: String,
    pub schema_version: u64,
    pub revision: u64,
    pub task_id: Option<String>,
    pub project_id: Option<String>,
    pub author_id: String,
    pub kind: PrivateNoteKind,
    pub title: Option<String>,
    pub why: Option<String>,
    pub alternatives: Option<String>,
    /// Normalized and sorted; empty on a tombstone.
    pub tags: Vec<String>,
    pub body_markdown: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl fmt::Debug for PrivateNoteRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted = |value: &Option<String>| value.as_ref().map(|_| "<redacted>");
        f.debug_struct("PrivateNoteRecord")
            .field("organization_id", &self.organization_id)
            .field("id", &self.id)
            .field("schema_version", &self.schema_version)
            .field("revision", &self.revision)
            .field("task_id", &self.task_id)
            .field("project_id", &self.project_id)
            .field("author_id", &self.author_id)
            .field("kind", &self.kind)
            .field("title", &redacted(&self.title))
            .field("why", &redacted(&self.why))
            .field("alternatives", &redacted(&self.alternatives))
            .field("tags", &format!("<{} redacted>", self.tags.len()))
            .field("body_markdown", &redacted(&self.body_markdown))
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("deleted_at", &self.deleted_at)
            .finish()
    }
}

impl PrivateNoteRecord {
    /// A new active note at revision 1.
    pub fn new_active(cmd: &CreateNoteCmd<'_>) -> Self {
        Self {
            organization_id: cmd.org_id.to_owned(),
            id: cmd.note_id.to_owned(),
            schema_version: 1,
            revision: 1,
            task_id: cmd.task_id.map(str::to_owned),
            project_id: cmd.project_id.map(str::to_owned),
            author_id: cmd.author_id.to_owned(),
            kind: cmd.content.kind,
            title: cmd.content.title.clone(),
            why: cmd.content.why.clone(),
            alternatives: cmd.content.alternatives.clone(),
            tags: cmd.content.tags.clone(),
            body_markdown: Some(cmd.content.body_markdown.clone()),
            created_at: cmd.now.to_owned(),
            updated_at: cmd.now.to_owned(),
            deleted_at: None,
        }
    }

    /// This note at `revision`, holding `content`.
    pub fn with_content(&self, content: &NoteContent, revision: u64, updated_at: &str) -> Self {
        Self {
            revision,
            kind: content.kind,
            title: content.title.clone(),
            why: content.why.clone(),
            alternatives: content.alternatives.clone(),
            tags: content.tags.clone(),
            body_markdown: Some(content.body_markdown.clone()),
            updated_at: updated_at.to_owned(),
            deleted_at: None,
            ..self.clone()
        }
    }

    /// This note's revisioned tombstone (`PN-06`): every private text field and tag cleared.
    pub fn tombstone(&self, revision: u64, deleted_at: &str) -> Self {
        Self {
            revision,
            title: None,
            why: None,
            alternatives: None,
            tags: Vec::new(),
            body_markdown: None,
            updated_at: deleted_at.to_owned(),
            deleted_at: Some(deleted_at.to_owned()),
            ..self.clone()
        }
    }

    pub fn to_wire(&self, caller_id: &str) -> PrivateNoteWire {
        self.to_wire_with(caller_id, FieldSet::all())
    }

    /// The wire view carrying only the content `fields` (identity and times always).
    pub fn to_wire_with(&self, caller_id: &str, fields: FieldSet) -> PrivateNoteWire {
        let pick =
            |field, value: &Option<String>| fields.includes(field).then(|| value.clone()).flatten();
        PrivateNoteWire {
            id: self.id.clone(),
            task_id: self.task_id.clone(),
            project_id: self.project_id.clone(),
            author_id: self.author_id.clone(),
            kind: self.kind,
            revision: self.revision,
            title: pick(PrivateNoteField::Title, &self.title),
            why: pick(PrivateNoteField::Why, &self.why),
            alternatives: pick(PrivateNoteField::Alternatives, &self.alternatives),
            tags: if fields.includes(PrivateNoteField::Tags) {
                self.tags.clone()
            } else {
                Vec::new()
            },
            body_markdown: pick(PrivateNoteField::BodyMarkdown, &self.body_markdown),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            editable: self.author_id == caller_id,
        }
    }
}

/// Request arguments for creating a new private note.
#[derive(Debug, Clone)]
pub struct CreateNoteCmd<'a> {
    pub org_id: &'a str,
    pub author_id: &'a str,
    pub task_id: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub note_id: &'a str,
    pub content: &'a NoteContent,
    pub operation_id: &'a str,
    pub request_sha256: &'a str,
    pub now: &'a str,
}

/// Request arguments for an author's edit of a private note.
#[derive(Debug, Clone)]
pub struct EditNoteCmd<'a> {
    pub org_id: &'a str,
    pub caller_id: &'a str,
    pub caller_is_person: bool,
    pub note_id: &'a str,
    pub expected_revision: u64,
    pub patch: &'a NotePatch,
    pub operation_id: &'a str,
    pub request_sha256: &'a str,
    pub now: &'a str,
}

/// Request arguments for soft-deleting (tombstoning) a private note.
#[derive(Debug, Clone)]
pub struct DeleteNoteCmd<'a> {
    pub org_id: &'a str,
    pub caller_id: &'a str,
    pub caller_is_person: bool,
    pub note_id: &'a str,
    pub expected_revision: u64,
    pub operation_id: &'a str,
    pub request_sha256: &'a str,
    pub now: &'a str,
}

/// Request arguments for starting or resuming an explicit publish-copy (`PN-05`).
#[derive(Debug, Clone)]
pub struct BeginPublishCmd<'a> {
    pub org_id: &'a str,
    pub caller_id: &'a str,
    pub caller_is_person: bool,
    pub note_id: &'a str,
    pub expected_revision: u64,
    pub comment_id: &'a str,
    pub operation_id: &'a str,
    pub request_sha256: &'a str,
    pub now: &'a str,
}

/// Outcome of [`PrivateNotesStore::begin_publish`].
#[derive(Debug, Clone)]
pub enum PublishAttempt {
    /// Receipt recorded (or resumed) in `pending` state; caller must post the shared comment
    /// using `shared_comment_id` and then call [`PrivateNotesStore::finish_publish`].
    Pending {
        note: PrivateNoteRecord,
        shared_comment_id: String,
    },
    /// Identical `operation_id` was already published and completed.
    AlreadyCompleted {
        note: PrivateNoteRecord,
        result_revision: u64,
        shared_comment_id: String,
    },
}

/// Consumer-owned adapter boundary for the isolated private notes database (`doc-8`, `ENG-1`).
/// Every engine enforces `ADR-0031` itself: an agent reads only its own notes.
pub trait PrivateNotesStore: Send + Sync {
    /// Whether this office holds private notes at all. `false` only on a satellite, where every
    /// `tasks.notes.*` call fails closed with `Unavailable` before reaching the store.
    fn available(&self) -> bool {
        true
    }

    fn create_note(
        &self,
        cmd: CreateNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError>;

    fn edit_note(
        &self,
        cmd: EditNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError>;

    fn delete_note(
        &self,
        cmd: DeleteNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError>;

    fn get_note(
        &self,
        org_id: &str,
        caller_id: &str,
        caller_is_person: bool,
        note_id: &str,
    ) -> Result<Option<PrivateNoteRecord>, OfficeError>;

    /// One newest-first page of the caller's visible active notes matching `query`.
    fn list_notes(&self, query: &NoteQuery<'_>) -> Result<NotePage, OfficeError>;

    fn begin_publish(
        &self,
        cmd: BeginPublishCmd<'_>,
    ) -> Result<Result<PublishAttempt, ApiError>, OfficeError>;

    fn finish_publish(
        &self,
        org_id: &str,
        author_id: &str,
        operation_id: &str,
        error_code: Option<&str>,
        now: &str,
    ) -> Result<(), OfficeError>;

    /// Mirrors a shared task deletion into `private_note_tasks.is_deleted = 1` (`PN-06`).
    fn mark_task_deleted(&self, org_id: &str, task_id: &str) -> Result<(), OfficeError>;
}

/// Fail-closed private-notes adapter for satellite offices (`doc-79`, `TASK-52` / `ARCH-02`).
/// Satellites never open or create `<durable>/private/task-notes.db`; private notes live
/// exclusively at HQ.
#[derive(Debug, Default)]
pub struct SatelliteRefusedPrivateNotesStore;

impl SatelliteRefusedPrivateNotesStore {
    /// The refusal every `tasks.notes.*` call receives on a satellite: the notes exist only at
    /// HQ, so the request must go there (`Unavailable`, never an authorization answer).
    pub fn refused() -> ApiError {
        ApiError::new(
            ErrorCode::Unavailable,
            "private notes are stored only at HQ and are unavailable on a satellite office",
        )
    }
}

impl PrivateNotesStore for SatelliteRefusedPrivateNotesStore {
    fn available(&self) -> bool {
        false
    }

    fn create_note(
        &self,
        _cmd: CreateNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        Ok(Err(Self::refused()))
    }

    fn edit_note(
        &self,
        _cmd: EditNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        Ok(Err(Self::refused()))
    }

    fn delete_note(
        &self,
        _cmd: DeleteNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        Ok(Err(Self::refused()))
    }

    fn get_note(
        &self,
        _org_id: &str,
        _caller_id: &str,
        _caller_is_person: bool,
        _note_id: &str,
    ) -> Result<Option<PrivateNoteRecord>, OfficeError> {
        Ok(None)
    }

    fn list_notes(&self, _query: &NoteQuery<'_>) -> Result<NotePage, OfficeError> {
        Ok(NotePage::default())
    }

    fn begin_publish(
        &self,
        _cmd: BeginPublishCmd<'_>,
    ) -> Result<Result<PublishAttempt, ApiError>, OfficeError> {
        Ok(Err(Self::refused()))
    }

    fn finish_publish(
        &self,
        _org_id: &str,
        _author_id: &str,
        _operation_id: &str,
        _error_code: Option<&str>,
        _now: &str,
    ) -> Result<(), OfficeError> {
        Ok(())
    }

    fn mark_task_deleted(&self, _org_id: &str, _task_id: &str) -> Result<(), OfficeError> {
        Ok(())
    }
}

/// Computes the canonical lowercase 64-hex SHA-256 request digest stored in
/// `private_note_command_receipts.request_sha256` (never stores note text in receipts).
pub fn request_digest(parts: &[&[u8]]) -> String {
    Sha256::of_parts(parts).to_hex()
}

/// The receipt digest of a `tasks.notes.remove` request.
pub fn delete_digest(note_id: &str, expected_revision: u64) -> String {
    let mut input = DigestInput::new("delete");
    input.text(Some(note_id)).number(expected_revision);
    input.finish()
}

/// The receipt digest of a `tasks.notes.publish` request.
pub fn publish_digest(note_id: &str, expected_revision: u64, comment_id: &str) -> String {
    let mut input = DigestInput::new("publish");
    input
        .text(Some(note_id))
        .number(expected_revision)
        .text(Some(comment_id));
    input.finish()
}

/// Enforces `ADR-0031` / `TASKAUTH-6` / `PN-02` non-author mutation refusal:
/// - Another agent ALWAYS receives `NotFound` (zero existence disclosure).
/// - Another person receives `NotAuthorized` (can read, cannot edit/delete/publish).
pub fn non_author_mutation_error(caller_is_person: bool) -> ApiError {
    if caller_is_person {
        ApiError::new(
            ErrorCode::NotAuthorized,
            "only the note author may modify, delete, or publish a private note",
        )
    } else {
        ApiError::new(ErrorCode::NotFound, "private note not found")
    }
}

/// Refuses an author mutation of `existing` unless it is the caller's active note at
/// `expected_revision` with no pending publication. `pending` is asked only of the author.
pub fn check_author_mutation(
    existing: Option<PrivateNoteRecord>,
    caller_id: &str,
    caller_is_person: bool,
    expected_revision: u64,
    pending: impl FnOnce() -> Result<bool, OfficeError>,
    pending_msg: &str,
) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
    let Some(existing) = existing.filter(|note| note.deleted_at.is_none()) else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "private note not found",
        )));
    };
    if existing.author_id != caller_id {
        return Ok(Err(non_author_mutation_error(caller_is_person)));
    }
    if pending()? {
        return Ok(Err(ApiError::new(ErrorCode::Conflict, pending_msg)));
    }
    if existing.revision != expected_revision {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "stale expected_revision {expected_revision}: note is at revision {}",
                existing.revision
            ),
        )));
    }
    Ok(Ok(existing))
}

/// The revision after `current`, refused once SQLite's signed range is exhausted.
pub fn next_revision(current: u64) -> Result<u64, ApiError> {
    if current >= i64::MAX as u64 {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "private note revision limit reached",
        ));
    }
    Ok(current + 1)
}

/// `tasks.notes.publish` copies a note into its task's shared comments: a note outside any
/// task has nowhere to go.
pub fn require_publishable(note: &PrivateNoteRecord) -> Result<(), ApiError> {
    if note.task_id.is_none() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "only a private note on a task can be published to its comments",
        ));
    }
    Ok(())
}

/// Refuses a reused `command_id` whose stored command or request differs.
pub fn check_receipt_reuse(
    stored_command: &str,
    stored_sha256: &str,
    expected_cmd: &str,
    request_sha256: &str,
) -> Result<(), ApiError> {
    if stored_command != expected_cmd || stored_sha256 != request_sha256 {
        return Err(ApiError::new(
            ErrorCode::OperationReused,
            "command_id reused with different private note request",
        ));
    }
    Ok(())
}

/// The later of `now` and `prev`, so stored times never run backwards.
pub fn monotonic_ts(now: &str, prev: &str) -> String {
    if now >= prev { now } else { prev }.to_owned()
}

/// What a replayed command had accepted.
#[derive(Debug, Clone, Copy)]
pub enum Accepted<'a> {
    Created(&'a NoteContent),
    Edited(&'a NotePatch),
    Deleted,
}

/// The note a replayed command answers: `current` at the receipt's `result_revision` and time,
/// holding what that command accepted.
pub fn replayed_note(
    current: &PrivateNoteRecord,
    result_revision: u64,
    receipt_time: &str,
    accepted: Accepted<'_>,
) -> PrivateNoteRecord {
    let content = match accepted {
        Accepted::Created(content) => Some(content.clone()),
        Accepted::Edited(patch) => patch.apply(current),
        Accepted::Deleted => None,
    };
    match content {
        Some(content) => current.with_content(&content, result_revision, receipt_time),
        None => current.tombstone(result_revision, receipt_time),
    }
}

/// Maps a stored `private_note_command_receipts.error_code` string back to an `ErrorCode`.
pub fn parse_receipt_error_code(raw: Option<&str>) -> ErrorCode {
    match raw {
        Some("not_found") => ErrorCode::NotFound,
        Some("not_authorized") => ErrorCode::NotAuthorized,
        Some("conflict") => ErrorCode::Conflict,
        Some("too_large") => ErrorCode::TooLarge,
        Some("already_exists") => ErrorCode::AlreadyExists,
        Some("operation_reused") => ErrorCode::OperationReused,
        Some("unavailable") => ErrorCode::Unavailable,
        Some("storage_error") => ErrorCode::StorageError,
        _ => ErrorCode::InvalidRequest,
    }
}

/// Formats an `ErrorCode` as a stable lowercase string for `private_note_command_receipts.error_code`.
pub fn format_receipt_error_code(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::NotFound => "not_found",
        ErrorCode::NotAuthorized => "not_authorized",
        ErrorCode::Conflict => "conflict",
        ErrorCode::TooLarge => "too_large",
        ErrorCode::AlreadyExists => "already_exists",
        ErrorCode::OperationReused => "operation_reused",
        ErrorCode::InvalidRequest => "invalid_request",
        ErrorCode::Unavailable => "unavailable",
        _ => "storage_error",
    }
}

#[cfg(test)]
mod tests;
