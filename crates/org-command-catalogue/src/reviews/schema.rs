//! Shared declarative schemas and error sets for the `review` command domain (`TASK-92.1`).

use org_command::{Field, Type};
use org_proto::ErrorCode;

use crate::shared::STRING_TYPE;

pub(super) const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

pub(super) const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::AlreadyExists,
    ErrorCode::Conflict,
    ErrorCode::TooLarge,
    ErrorCode::OperationReused,
    ErrorCode::Unavailable,
];

pub(super) const REVIEW_REF_INPUT: &[Field] = &[Field::required(
    "review",
    Type::String,
    "Review UUIDv7 or RV-<OFFICE><N> key",
)];

pub(super) const TARGET_KIND_TYPE: Type = Type::Enum(&["code", "doc", "task", "artifact"]);
pub(super) const STATUS_TYPE: Type =
    Type::Enum(&["open", "changes_requested", "approved", "closed"]);
pub(super) const SESSION_MODE_TYPE: Type = Type::Enum(&["warm", "fresh"]);
pub(super) const VERDICT_TYPE: Type =
    Type::Enum(&["approve", "changes_requested", "comment", "incomplete"]);
pub(super) const SEVERITY_TYPE: Type =
    Type::Enum(&["blocker", "major", "minor", "nit", "question"]);
pub(super) const FINDING_STATUS_TYPE: Type = Type::Enum(&["open", "resolved", "wont_fix"]);
pub(super) const ANCHOR_KIND_TYPE: Type =
    Type::Enum(&["general", "file_line", "doc_section", "checklist_item"]);
pub(super) const ROUTING_MODE_TYPE: Type = Type::Enum(&["explicit", "pool", "single_agent"]);
pub(super) const DIFF_FILE_STATUS_TYPE: Type =
    Type::Enum(&["added", "modified", "deleted", "type_changed"]);

pub(super) const REVIEW_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Review UUIDv7"),
    Field::required("key", Type::String, "Office-coded key (e.g. RV-A1)"),
    Field::required("office_code", Type::String, "Originating office code"),
    Field::required("number", Type::Integer, "Per-office sequence number"),
    Field::required("organization_id", Type::String, "Organization UUIDv7"),
    Field::optional("project_id", Type::String, "Project UUIDv7"),
    Field::optional("task_id", Type::String, "Task UUIDv7"),
    Field::required("target_kind", TARGET_KIND_TYPE, "Reviewed target kind"),
    Field::optional("repository_id", Type::String, "Repository UUIDv7"),
    Field::optional("branch", Type::String, "Task branch name"),
    Field::optional("target_ref", Type::String, "Doc or artifact target ref"),
    Field::required("kind_slug", Type::String, "Review kind slug"),
    Field::optional("category_slug", Type::String, "Optional category slug"),
    Field::required("title", Type::String, "Review title"),
    Field::optional("body_markdown", Type::String, "Review description markdown"),
    Field::required("independent", Type::Boolean, "Independent review guard"),
    Field::required("status", STATUS_TYPE, "Current review status"),
    Field::required("current_round", Type::Integer, "Active round number"),
    Field::required("pin_stale", Type::Boolean, "Worktree head moved past pin"),
    Field::optional(
        "guidance_refs",
        Type::List(&STRING_TYPE),
        "Referenced guidance docs",
    ),
    Field::optional("criteria", Type::List(&STRING_TYPE), "Acceptance criteria"),
    Field::required("open_findings", Type::Integer, "Open findings count"),
    Field::optional("closed_reason", Type::String, "Close reason"),
    Field::required("revision", Type::Integer, "Optimistic concurrency revision"),
    Field::required("created_by", Type::String, "Creator employee UUIDv7"),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("updated_at", Type::String, "Last update timestamp"),
];

pub(super) const VERDICT_FIELDS: &[Field] = &[
    Field::required("reviewer_id", Type::String, "Reviewer employee UUIDv7"),
    Field::required("round_number", Type::Integer, "Round number"),
    Field::required("verdict", VERDICT_TYPE, "Submitted verdict"),
    Field::optional("summary_markdown", Type::String, "Verdict summary"),
    Field::optional(
        "finding_ids",
        Type::List(&STRING_TYPE),
        "Finding UUIDv7s raised with verdict",
    ),
    Field::optional(
        "attachment_ids",
        Type::List(&STRING_TYPE),
        "Cited shared attachment UUIDv7s",
    ),
    Field::required("submitted_at", Type::String, "Submission timestamp"),
];

pub(super) const VERDICT_ITEM_TYPE: Type = Type::Object(VERDICT_FIELDS);

pub(super) const ROUND_FIELDS: &[Field] = &[
    Field::required("round_number", Type::Integer, "Round number from 1"),
    Field::optional("base_oid", Type::String, "Pinned base commit OID"),
    Field::optional("head_oid", Type::String, "Pinned head commit OID"),
    Field::required("session_mode", SESSION_MODE_TYPE, "Reviewer session mode"),
    Field::required("submitted_by", Type::String, "Submitter employee UUIDv7"),
    Field::required("submitted_at", Type::String, "Round submission timestamp"),
    Field::optional("note_markdown", Type::String, "Author submission note"),
    Field::optional(
        "verdicts",
        Type::List(&VERDICT_ITEM_TYPE),
        "Verdicts submitted on this round",
    ),
];

pub(super) const ROUND_ITEM_TYPE: Type = Type::Object(ROUND_FIELDS);

pub(super) const REVIEWER_FIELDS: &[Field] = &[
    Field::required("reviewer_id", Type::String, "Reviewer employee UUIDv7"),
    Field::required("routing", ROUTING_MODE_TYPE, "Routing mode"),
    Field::optional("team_id", Type::String, "Pool team UUIDv7"),
    Field::required("session_mode", SESSION_MODE_TYPE, "Reviewer session mode"),
    Field::required("assigned_by", Type::String, "Assigner employee UUIDv7"),
    Field::required("assigned_at", Type::String, "Assignment timestamp"),
    Field::optional("last_verdict", VERDICT_TYPE, "Latest verdict"),
    Field::optional("last_verdict_round", Type::Integer, "Latest verdict round"),
    Field::optional("removed", Type::Boolean, "Soft-removed by later routing"),
];

pub(super) const REVIEWER_ITEM_TYPE: Type = Type::Object(REVIEWER_FIELDS);

pub(super) const ANCHOR_FIELDS: &[Field] = &[
    Field::required("kind", ANCHOR_KIND_TYPE, "Anchor kind"),
    Field::optional("path", Type::String, "Repository-relative file path"),
    Field::optional("start_line", Type::Integer, "First anchored line"),
    Field::optional("end_line", Type::Integer, "Last anchored line"),
    Field::optional(
        "anchor_snippet_sha256",
        Type::String,
        "SHA-256 of anchored lines at round head",
    ),
    Field::optional("doc_section", Type::String, "Document section heading"),
    Field::optional("checklist_item_id", Type::String, "Checklist item UUIDv7"),
];

pub(super) const REPLY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Reply UUIDv7"),
    Field::required("finding_id", Type::String, "Parent finding UUIDv7"),
    Field::required("author_id", Type::String, "Reply author UUIDv7"),
    Field::optional("body_markdown", Type::String, "Reply markdown"),
    Field::optional("set_status", FINDING_STATUS_TYPE, "New finding status"),
    Field::optional(
        "attachment_ids",
        Type::List(&STRING_TYPE),
        "Shared attachment UUIDv7s",
    ),
    Field::required("created_at", Type::String, "Reply creation timestamp"),
];

pub(super) const REPLY_ITEM_TYPE: Type = Type::Object(REPLY_FIELDS);

pub(super) const FINDING_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Finding UUIDv7"),
    Field::required("review_id", Type::String, "Review UUIDv7"),
    Field::required("number", Type::Integer, "Finding number from 1"),
    Field::required("round_number", Type::Integer, "Round raised in"),
    Field::required("severity", SEVERITY_TYPE, "Finding severity"),
    Field::required("status", FINDING_STATUS_TYPE, "Finding status"),
    Field::required("anchor", Type::Object(ANCHOR_FIELDS), "Finding anchor"),
    Field::optional(
        "anchor_stale",
        Type::Boolean,
        "Anchored lines changed at current round head",
    ),
    Field::required("body_markdown", Type::String, "Finding markdown"),
    Field::required("author_id", Type::String, "Author employee UUIDv7"),
    Field::optional(
        "attachment_ids",
        Type::List(&STRING_TYPE),
        "Shared attachment UUIDv7s",
    ),
    Field::optional("resolved_by", Type::String, "Resolver employee UUIDv7"),
    Field::optional("resolved_at", Type::String, "Resolution timestamp"),
    Field::optional("resolved_in_round", Type::Integer, "Round resolved in"),
    Field::optional("replies", Type::List(&REPLY_ITEM_TYPE), "Finding replies"),
    Field::required("revision", Type::Integer, "Finding revision"),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("updated_at", Type::String, "Last update timestamp"),
];

pub(super) const FINDING_ITEM_TYPE: Type = Type::Object(FINDING_FIELDS);

pub(super) const DIFF_FILE_FIELDS: &[Field] = &[
    Field::required("path", Type::String, "Repository-relative path"),
    Field::required("status", DIFF_FILE_STATUS_TYPE, "File change status"),
    Field::required("added", Type::Integer, "Added line count"),
    Field::required("deleted", Type::Integer, "Deleted line count"),
    Field::optional("binary", Type::Boolean, "Binary file flag"),
];

pub(super) const DIFF_FILE_ITEM_TYPE: Type = Type::Object(DIFF_FILE_FIELDS);

pub(super) const SAMPLE_REVIEW_JSON: &str = r#"{"id":"018bce83-1200-7000-8000-000000000001","key":"RV-A1","office_code":"A","number":1,"organization_id":"018bce83-1200-7000-8000-000000000099","target_kind":"code","kind_slug":"code-review","title":"Add parser","independent":false,"status":"open","current_round":1,"pin_stale":false,"open_findings":0,"revision":1,"created_by":"018bce83-1200-7000-8000-000000000010","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"}"#;
pub(super) const SAMPLE_ROUND_JSON: &str = r#"{"round_number":1,"session_mode":"warm","submitted_by":"018bce83-1200-7000-8000-000000000010","submitted_at":"2026-11-27T00:00:00.000Z"}"#;
pub(super) const SAMPLE_FINDING_JSON: &str = r#"{"id":"018bce83-1200-7000-8000-000000000020","review_id":"018bce83-1200-7000-8000-000000000001","number":1,"round_number":1,"severity":"major","status":"open","anchor":{"kind":"general"},"body_markdown":"Check error path","author_id":"018bce83-1200-7000-8000-000000000011","revision":1,"created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z"}"#;
