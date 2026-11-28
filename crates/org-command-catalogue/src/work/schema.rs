//! Shared output field schemas and error sets for the work domain.

use org_command::{Field, Type};
use org_proto::ErrorCode;

use crate::shared::STRING_TYPE;

pub(super) const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

pub(super) const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::AlreadyExists,
    ErrorCode::NotFound,
    ErrorCode::Conflict,
    ErrorCode::TooLarge,
    ErrorCode::OperationReused,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

pub(super) const BLOCKING_DEP_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Prerequisite task UUIDv7"),
    Field::required("key", Type::String, "Prerequisite task key (T-<CODE><N>)"),
    Field::required("title", Type::String, "Prerequisite task title"),
    Field::required(
        "status",
        Type::Enum(&["todo", "in_progress", "blocked", "cancelled", "done"]),
        "Current prerequisite status",
    ),
    Field::required(
        "cancelled",
        Type::Boolean,
        "True when prerequisite was cancelled rather than completed",
    ),
];

pub(super) const BLOCKING_DEP_TYPE: Type = Type::Object(BLOCKING_DEP_FIELDS);

pub(super) const TASK_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Office-coded task key (e.g. T-A1)"),
    Field::required("type", Type::String, "Task type (task, bug, request, ...)"),
    Field::optional(
        "work_type",
        Type::Enum(&["build", "run"]),
        "Optional Build/Run classification",
    ),
    Field::required("title", Type::String, "Single-line task title"),
    Field::required(
        "status",
        Type::Enum(&["todo", "in_progress", "blocked", "cancelled", "done"]),
        "Current lifecycle status",
    ),
    Field::required(
        "priority",
        Type::Enum(&["urgent", "high", "normal", "low"]),
        "Task priority",
    ),
    Field::optional("effort", Type::Integer, "Optional effort estimate"),
    Field::optional("severity", Type::String, "Optional bug severity"),
    Field::optional("resolution", Type::String, "Optional closure resolution"),
    Field::optional(
        "blocked_reason",
        Type::String,
        "Explicit block reason when status is blocked",
    ),
    Field::optional(
        "blocked_since",
        Type::String,
        "RFC 3339 start of current continuous blocked span",
    ),
    Field::optional(
        "waiting_on_owner",
        Type::Boolean,
        "Whether explicit block awaits owner action",
    ),
    Field::optional(
        "dependency_blocked",
        Type::Boolean,
        "Whether any direct depends_on prerequisite is unmet",
    ),
    Field::optional(
        "unmet_blocking_dependencies",
        Type::List(&BLOCKING_DEP_TYPE),
        "Unmet direct upstream depends_on prerequisites",
    ),
    Field::required("team_id", Type::String, "Accountable owning team UUIDv7"),
    Field::optional("assigned_team_id", Type::String, "Assigned team UUIDv7"),
    Field::optional("assignee_id", Type::String, "Assigned employee UUIDv7"),
    Field::optional("project_id", Type::String, "Optional owning project UUIDv7"),
    Field::optional(
        "rank_order",
        Type::Integer,
        "Explicit execution rank/order index",
    ),
    Field::required(
        "labels",
        Type::List(&STRING_TYPE),
        "Normalized searchable labels",
    ),
    Field::required(
        "reference_files",
        Type::List(&STRING_TYPE),
        "Reference file paths",
    ),
    Field::required(
        "modified_files",
        Type::List(&STRING_TYPE),
        "Modified file paths",
    ),
    Field::required(
        "depends_on",
        Type::List(&STRING_TYPE),
        "Direct upstream dependency task IDs",
    ),
    Field::required(
        "dependents",
        Type::List(&STRING_TYPE),
        "Direct downstream dependent task IDs",
    ),
    Field::required("revision", Type::Integer, "Task record revision"),
    Field::required("checklist_total", Type::Integer, "Total checklist steps"),
    Field::required("checklist_done", Type::Integer, "Completed checklist steps"),
    Field::required("comment_count", Type::Integer, "Public comment count"),
    Field::required("watcher_count", Type::Integer, "Watcher count"),
];

pub(super) const TASK_REF_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key (T-<CODE><N>)"),
    Field::required("title", Type::String, "Task title"),
    Field::required("type", Type::String, "Task type"),
    Field::required("status", Type::String, "Task status"),
    Field::required("priority", Type::String, "Task priority"),
    Field::optional("rank_order", Type::Integer, "Execution rank/order index"),
];

pub(super) const TASK_REF_TYPE: Type = Type::Object(TASK_REF_FIELDS);

pub(super) const CHECKLIST_STEP_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Checklist step UUIDv7"),
    Field::optional(
        "section",
        Type::Enum(&["acceptance", "dod", "work"]),
        "Checklist section",
    ),
    Field::required("position", Type::Integer, "Zero-based step position"),
    Field::required("text", Type::String, "Step text"),
    Field::required("required", Type::Boolean, "Whether step is required"),
    Field::required("done", Type::Boolean, "Whether step is checked"),
    Field::optional(
        "checked_by_employee_id",
        Type::String,
        "Employee who checked it",
    ),
    Field::optional("checked_at", Type::String, "RFC 3339 check timestamp"),
    Field::required("revision", Type::Integer, "Step revision"),
];

pub(super) const CHECKLIST_STEP_TYPE: Type = Type::Object(CHECKLIST_STEP_FIELDS);

pub(super) const WATCHER_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Watcher record UUIDv7"),
    Field::required("watcher_kind", Type::String, "Watcher kind"),
    Field::optional("watcher_id", Type::String, "Watcher employee UUIDv7"),
];

pub(super) const WATCHER_TYPE: Type = Type::Object(WATCHER_FIELDS);

pub(super) const ATTACHMENT_META_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Attachment UUIDv7"),
    Field::required("filename", Type::String, "Original filename"),
    Field::required("title", Type::String, "File title"),
    Field::optional("description", Type::String, "Optional short description"),
    Field::required("artifact_status", Type::String, "Artifact status"),
    Field::required("mime_type", Type::String, "Canonical MIME type"),
    Field::required("byte_count", Type::Integer, "Stored size in bytes"),
    Field::optional("width", Type::Integer, "Image width in pixels"),
    Field::optional("height", Type::Integer, "Image height in pixels"),
    Field::optional("page_count", Type::Integer, "PDF page count"),
    Field::required(
        "uploaded_by_employee_id",
        Type::String,
        "Uploader employee UUIDv7",
    ),
    Field::required("created_at", Type::String, "RFC 3339 creation timestamp"),
];

pub(super) const ATTACHMENT_META_TYPE: Type = Type::Object(ATTACHMENT_META_FIELDS);

pub(super) const COMMENT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Comment UUIDv7"),
    Field::required("position", Type::Integer, "Zero-based comment position"),
    Field::required("author_id", Type::String, "Author employee UUIDv7"),
    Field::required("created_at", Type::String, "RFC 3339 creation timestamp"),
    Field::required("body_markdown", Type::String, "Public comment body"),
    Field::optional(
        "attachments",
        Type::List(&ATTACHMENT_META_TYPE),
        "Files attached to this comment",
    ),
];

pub(super) const COMMENT_TYPE: Type = Type::Object(COMMENT_FIELDS);

pub(super) const EVENT_FIELDS: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("revision", Type::Integer, "Task revision"),
    Field::optional("occurred_at", Type::String, "RFC 3339 timestamp"),
    Field::required("actor_kind", Type::String, "Actor kind"),
    Field::optional("actor_id", Type::String, "Actor employee UUIDv7"),
    Field::required("change", Type::Json, "Structured event change delta"),
    Field::required("payload_sha256", Type::String, "Event SHA-256 digest"),
];

pub(super) const EVENT_TYPE: Type = Type::Object(EVENT_FIELDS);

pub(super) const DEP_EDGE_FIELDS: &[Field] = &[
    Field::required("task_id", Type::String, "Dependent task UUIDv7"),
    Field::required(
        "depends_on_task_id",
        Type::String,
        "Prerequisite task UUIDv7",
    ),
];

pub(super) const DEP_EDGE_TYPE: Type = Type::Object(DEP_EDGE_FIELDS);

pub(super) const TASK_REF_INPUT: &[Field] = &[Field::required(
    "task",
    Type::String,
    "Task UUIDv7 or T-<CODE><N> key",
)];

pub(super) const TASK_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","type":"task","title":"Implement auth","status":"todo","priority":"high","team_id":"0193a5c0-0000-7000-8000-000000000010","rank_order":10,"labels":["backend","security"],"reference_files":["docs/auth.md"],"modified_files":["src/auth.rs"],"depends_on":[],"dependents":[],"revision":1,"checklist_total":0,"checklist_done":0,"comment_count":0,"watcher_count":0}"#;
