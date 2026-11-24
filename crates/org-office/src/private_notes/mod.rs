//! Consumer-owned port and domain contract for isolated per-author private task notes
//! (`TASK-50`, `L8`, `ADR-0031`, `doc-79` PN-01..06).
//!
//! Private task notes NEVER enter `durable/state/`, `durable/journal/`, `office.db`, shared
//! task events, Git, search, feeds, inbox, summaries, or other agents' turn inputs. The storage
//! adapter (`SqlitePrivateNotesStore` in `src/store/sqlite/private_notes.rs`, or
//! [`memory::MemoryPrivateNotesStore`] in tests) owns the isolated HQ-authoritative database at
//! `<durable-root>/private/task-notes.db` (`doc-79` §Private Task Notes).

pub mod memory;

use std::fmt;

use org_proto::private_notes::{PrivateTaskNoteWire, PRIVATE_NOTE_MAX_CHARS};
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;

use crate::error::OfficeError;

/// One row of `private_task_notes` (`private-notes-schema.sql`).
#[derive(Clone, PartialEq, Eq)]
pub struct PrivateNoteRecord {
    pub organization_id: String,
    pub id: String,
    pub schema_version: u64,
    pub revision: u64,
    pub task_id: String,
    pub author_id: String,
    pub body_markdown: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl fmt::Debug for PrivateNoteRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateNoteRecord")
            .field("organization_id", &self.organization_id)
            .field("id", &self.id)
            .field("schema_version", &self.schema_version)
            .field("revision", &self.revision)
            .field("task_id", &self.task_id)
            .field("author_id", &self.author_id)
            .field(
                "body_markdown",
                &self.body_markdown.as_ref().map(|_| "<redacted>"),
            )
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("deleted_at", &self.deleted_at)
            .finish()
    }
}

impl PrivateNoteRecord {
    pub fn to_wire(&self, caller_id: &str) -> PrivateTaskNoteWire {
        PrivateTaskNoteWire {
            id: self.id.clone(),
            task_id: self.task_id.clone(),
            author_id: self.author_id.clone(),
            revision: self.revision,
            body_markdown: self.body_markdown.clone().unwrap_or_default(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            editable: self.author_id == caller_id,
        }
    }
}

/// Request arguments for creating a new private note.
#[derive(Clone)]
pub struct CreateNoteCmd<'a> {
    pub org_id: &'a str,
    pub author_id: &'a str,
    pub task_id: &'a str,
    pub note_id: &'a str,
    pub body_markdown: &'a str,
    pub operation_id: &'a str,
    pub request_sha256: &'a str,
    pub now: &'a str,
}

impl fmt::Debug for CreateNoteCmd<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateNoteCmd")
            .field("org_id", &self.org_id)
            .field("author_id", &self.author_id)
            .field("task_id", &self.task_id)
            .field("note_id", &self.note_id)
            .field("body_markdown", &"<redacted>")
            .field("operation_id", &self.operation_id)
            .finish()
    }
}

/// Request arguments for editing a private note.
#[derive(Clone)]
pub struct EditNoteCmd<'a> {
    pub org_id: &'a str,
    pub caller_id: &'a str,
    pub caller_is_person: bool,
    pub note_id: &'a str,
    pub expected_revision: u64,
    pub body_markdown: &'a str,
    pub operation_id: &'a str,
    pub request_sha256: &'a str,
    pub now: &'a str,
}

impl fmt::Debug for EditNoteCmd<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditNoteCmd")
            .field("org_id", &self.org_id)
            .field("caller_id", &self.caller_id)
            .field("caller_is_person", &self.caller_is_person)
            .field("note_id", &self.note_id)
            .field("expected_revision", &self.expected_revision)
            .field("body_markdown", &"<redacted>")
            .field("operation_id", &self.operation_id)
            .finish()
    }
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

/// Consumer-owned adapter boundary for the isolated private task notes database (`doc-8`, `ENG-1`).
pub trait PrivateNotesStore: Send + Sync {
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

    fn list_notes(
        &self,
        org_id: &str,
        caller_id: &str,
        caller_is_person: bool,
        task_id: &str,
        author_filter: Option<&str>,
    ) -> Result<Vec<PrivateNoteRecord>, OfficeError>;

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
/// Satellites never open or create `<durable>/private/task-notes.db`; private task notes live
/// exclusively at HQ.
#[derive(Debug, Default)]
pub struct SatelliteRefusedPrivateNotesStore;

impl SatelliteRefusedPrivateNotesStore {
    fn refused() -> ApiError {
        ApiError::new(
            ErrorCode::NotAuthorized,
            "private task notes are stored only at HQ and are unavailable on a satellite office",
        )
    }
}

impl PrivateNotesStore for SatelliteRefusedPrivateNotesStore {
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

    fn list_notes(
        &self,
        _org_id: &str,
        _caller_id: &str,
        _caller_is_person: bool,
        _task_id: &str,
        _author_filter: Option<&str>,
    ) -> Result<Vec<PrivateNoteRecord>, OfficeError> {
        Ok(Vec::new())
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

/// Validates and trims a private note body (`PN-01`).
pub fn validate_note_body(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "private note body_markdown cannot be empty",
        ));
    }
    if trimmed.chars().count() > PRIVATE_NOTE_MAX_CHARS {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("private note exceeds {PRIVATE_NOTE_MAX_CHARS} characters"),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Computes the canonical lowercase 64-hex SHA-256 request digest stored in
/// `private_note_command_receipts.request_sha256` (never stores note body text in receipts).
pub fn request_digest(parts: &[&[u8]]) -> String {
    Sha256::of_parts(parts).to_hex()
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
