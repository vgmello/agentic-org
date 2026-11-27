//! Field schemas and error sets for `agent-os file` (`TASK-98.4`).

use org_command::{Field, Type};
use org_proto::ErrorCode;

pub(super) const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::TooLarge,
    ErrorCode::Unavailable,
];

pub(super) const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::AlreadyExists,
    ErrorCode::NotFound,
    ErrorCode::Conflict,
    ErrorCode::TooLarge,
    ErrorCode::Unsupported,
    ErrorCode::OperationReused,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const SCOPES: &[&str] = &["org", "project", "task", "private"];
const STATUSES: &[&str] = &["draft", "review", "final", "superseded", "archived"];

pub(super) const FILE_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Attachment UUIDv7"),
    Field::required(
        "scope",
        Type::Enum(&["shared", "user_private"]),
        "Storage scope",
    ),
    Field::required("target_kind", Type::String, "Attached target kind"),
    Field::optional("target_id", Type::String, "Target UUIDv7 (absent for org)"),
    Field::required("filename", Type::String, "Original filename"),
    Field::required("mime_type", Type::String, "Canonical MIME type"),
    Field::required("byte_count", Type::Integer, "Stored size in bytes"),
    Field::required("title", Type::String, "File title"),
    Field::optional("description", Type::String, "Optional short description"),
    Field::required(
        "artifact_status",
        Type::Enum(STATUSES),
        "Review lifecycle status",
    ),
    Field::optional("width", Type::Integer, "Image width in pixels"),
    Field::optional("height", Type::Integer, "Image height in pixels"),
    Field::optional("page_count", Type::Integer, "PDF page count"),
    Field::required(
        "uploaded_by_employee_id",
        Type::String,
        "Uploader employee UUIDv7",
    ),
    Field::required("created_at", Type::String, "RFC 3339 creation time"),
    Field::required("updated_at", Type::String, "RFC 3339 update time"),
    Field::required("content_sha256", Type::String, "Stored SHA-256 hex digest"),
];

pub(super) const FILE_UPLOAD_INPUT: &[Field] = &[
    Field::required(
        "path",
        Type::String,
        "Local file path inside an allowed root",
    ),
    Field::required(
        "scope",
        Type::Enum(SCOPES),
        "Target scope: org, project, task, or private",
    ),
    Field::optional(
        "project",
        Type::String,
        "Project UUIDv7, P-<n>, or slug (with --scope project)",
    ),
    Field::optional(
        "task",
        Type::String,
        "Task UUIDv7 or T-<CODE><N> key (with --scope task)",
    ),
    Field::optional("title", Type::String, "Optional title (1..=160 chars)"),
    Field::optional(
        "description",
        Type::String,
        "Optional description (<= 280 chars)",
    ),
    Field::optional(
        "status",
        Type::Enum(STATUSES),
        "Artifact status (default: final)",
    ),
];

pub(super) const FILE_LIST_INPUT: &[Field] = &[
    Field::optional(
        "scope",
        Type::Enum(SCOPES),
        "Scope: org, project, task, or private",
    ),
    Field::optional("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::optional("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
];

pub(super) const FILE_ID_INPUT: &[Field] =
    &[Field::required("id", Type::String, "Attachment UUIDv7")];

pub(super) const FILE_FETCH_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Attachment UUIDv7"),
    Field::required("output", Type::String, "Writable local output file path"),
];

pub(super) const FILE_FETCH_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Attachment UUIDv7"),
    Field::required("output", Type::String, "Canonical path written"),
    Field::required("byte_count", Type::Integer, "Bytes written"),
    Field::required(
        "content_sha256",
        Type::String,
        "Verified SHA-256 hex digest",
    ),
];

pub(super) const FILE_UPDATE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Attachment UUIDv7"),
    Field::optional("title", Type::String, "Updated title (1..=160 chars)"),
    Field::optional(
        "description",
        Type::String,
        "Updated description (empty clears)",
    ),
    Field::optional("status", Type::Enum(STATUSES), "Updated artifact status"),
];

pub(super) const FILE_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000500","scope":"shared","target_kind":"organization","filename":"spec.md","mime_type":"text/markdown","byte_count":42,"title":"Spec","artifact_status":"final","uploaded_by_employee_id":"0193a5c0-0000-7000-8000-000000000020","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z","content_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#;
