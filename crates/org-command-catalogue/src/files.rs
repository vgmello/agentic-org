//! Declarative command rows for `file upload|list|inspect|fetch|update|render`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::TooLarge,
    ErrorCode::Unavailable,
];

const WRITE_ERRORS: &[ErrorCode] = &[
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
const RENDER_FORMATS: &[&str] = &["pdf"];
const RENDER_STATUSES: &[&str] = &["draft", "review", "final"];

const FILE_FIELDS: &[Field] = &[
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

const FILE_UPLOAD_INPUT: &[Field] = &[
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

const FILE_LIST_INPUT: &[Field] = &[
    Field::optional(
        "scope",
        Type::Enum(SCOPES),
        "Scope: org, project, task, or private",
    ),
    Field::optional("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::optional("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
];

const FILE_ID_INPUT: &[Field] = &[Field::required("id", Type::String, "Attachment UUIDv7")];

const FILE_FETCH_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Attachment UUIDv7"),
    Field::required("output", Type::String, "Writable local output file path"),
];

const FILE_FETCH_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Attachment UUIDv7"),
    Field::required("output", Type::String, "Canonical path written"),
    Field::required("byte_count", Type::Integer, "Bytes written"),
    Field::required(
        "content_sha256",
        Type::String,
        "Verified SHA-256 hex digest",
    ),
];

const FILE_UPDATE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Attachment UUIDv7"),
    Field::optional("title", Type::String, "Updated title (1..=160 chars)"),
    Field::optional(
        "description",
        Type::String,
        "Updated description (empty clears)",
    ),
    Field::optional("status", Type::Enum(STATUSES), "Updated artifact status"),
];

const FILE_RENDER_INPUT: &[Field] = &[
    Field::required(
        "from",
        Type::String,
        "Local .md or .html source path inside an allowed root",
    ),
    Field::optional(
        "to",
        Type::Enum(RENDER_FORMATS),
        "Target document format (default: pdf; `--format pdf` also accepted)",
    ),
    Field::optional(
        "css",
        Type::String,
        "Optional local .css stylesheet path inside an allowed root",
    ),
    Field::optional(
        "scope",
        Type::Enum(SCOPES),
        "Hosted scope: org, project, task, or private (exclusive with --output)",
    ),
    Field::optional(
        "output",
        Type::String,
        "Writable local .pdf output path (exclusive with --scope)",
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
    Field::optional(
        "title",
        Type::String,
        "Optional document title (1..=160 chars)",
    ),
    Field::optional(
        "description",
        Type::String,
        "Optional description (<= 280 chars)",
    ),
    Field::optional(
        "status",
        Type::Enum(RENDER_STATUSES),
        "Artifact status for hosted scope (default: final)",
    ),
];

const FILE_RENDER_OUTPUT: &[Field] = &[
    Field::optional(
        "attachment_id",
        Type::String,
        "Hosted attachment UUIDv7 (when --scope is set)",
    ),
    Field::optional(
        "canonical_url",
        Type::String,
        "Canonical attachment URL (when --scope is set)",
    ),
    Field::optional(
        "output",
        Type::String,
        "Canonical local path written (when --output is set)",
    ),
    Field::required("page_count", Type::Integer, "Rendered PDF page count"),
    Field::required("byte_count", Type::Integer, "Rendered PDF size in bytes"),
    Field::optional(
        "content_sha256",
        Type::String,
        "Verified SHA-256 hex digest",
    ),
];

const FILE_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000500","scope":"shared","target_kind":"organization","filename":"spec.md","mime_type":"text/markdown","byte_count":42,"title":"Spec","artifact_status":"final","uploaded_by_employee_id":"0193a5c0-0000-7000-8000-000000000020","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z","content_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#;

const FILE_RENDER_EXAMPLE_JSON: &str = r#"{"attachment_id":"0193a5c0-0000-7000-8000-000000000500","canonical_url":"/api/v1/organizations/0193a5c0-0000-7000-8000-000000000001/attachments/0193a5c0-0000-7000-8000-000000000500","page_count":2,"byte_count":4096,"content_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#;

/// The `file` command rows.
pub const COMMANDS: &[Command] = &[
    Command {
        resource: "file",
        verb: "upload",
        summary: "Upload and attach a file",
        call: "attachment.attach",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: FILE_UPLOAD_INPUT,
        content_field: None,
        output: FILE_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"path":"spec.md","scope":"org","title":"Spec"}"#,
            output: FILE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "file",
        verb: "list",
        summary: "List files in a scope",
        call: "attachment.list",
        kind: Kind::List,
        input: FILE_LIST_INPUT,
        content_field: None,
        output: FILE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"scope":"org"}"#,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000500","scope":"shared","target_kind":"organization","filename":"spec.md","mime_type":"text/markdown","byte_count":42,"title":"Spec","artifact_status":"final","uploaded_by_employee_id":"0193a5c0-0000-7000-8000-000000000020","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z","content_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]"#,
        },
    },
    Command {
        resource: "file",
        verb: "inspect",
        summary: "Inspect file metadata",
        call: "attachment.inspect",
        kind: Kind::Read,
        input: FILE_ID_INPUT,
        content_field: None,
        output: FILE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000500"}"#,
            output: FILE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "file",
        verb: "fetch",
        summary: "Download a file locally",
        call: "attachment.read_chunk",
        kind: Kind::Read,
        input: FILE_FETCH_INPUT,
        content_field: None,
        output: FILE_FETCH_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000500","output":"spec.md"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000500","output":"spec.md","byte_count":42,"content_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#,
        },
    },
    Command {
        resource: "file",
        verb: "update",
        summary: "Update file metadata",
        call: "attachment.update_meta",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: FILE_UPDATE_INPUT,
        content_field: None,
        output: FILE_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000500","title":"Final spec","status":"final"}"#,
            output: FILE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "file",
        verb: "render",
        summary: "Render Markdown/HTML to PDF",
        call: "attachment.render",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: FILE_RENDER_INPUT,
        content_field: None,
        output: FILE_RENDER_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"from":"report.md","to":"pdf","scope":"org","title":"Report"}"#,
            output: FILE_RENDER_EXAMPLE_JSON,
        },
    },
];

/// All command row slices in the files domain.
pub const ALL: &[&[Command]] = &[COMMANDS];
