//! The work slice's error lists and the task schema its rows share: the task record and its
//! parts (checklist steps, comments, events, dependency edges), and the `task` inputs and
//! outputs. `checklist` and `task-note` keep their own inputs in their files.

use org_command::{Field, Type};
use org_proto::ErrorCode;

pub(super) const STRING_TYPE: Type = Type::String;

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

pub(super) const TASK_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Office-coded task key (e.g. T-A1)"),
    Field::required("type", Type::String, "Task type (task, bug, request, ...)"),
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

pub(super) const COMMENT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Comment UUIDv7"),
    Field::required("position", Type::Integer, "Zero-based comment position"),
    Field::required("author_id", Type::String, "Author employee UUIDv7"),
    Field::required("created_at", Type::String, "RFC 3339 creation timestamp"),
    Field::required("body_markdown", Type::String, "Public comment body"),
];

pub(super) const COMMENT_TYPE: Type = Type::Object(COMMENT_FIELDS);

pub(super) const EVENT_FIELDS: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("revision", Type::Integer, "Task revision"),
    Field::optional("occurred_at", Type::String, "RFC 3339 timestamp"),
    Field::required("actor_kind", Type::String, "Actor kind"),
    Field::optional("actor_id", Type::String, "Actor employee UUIDv7"),
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

pub(super) const TASK_INSPECT_OUTPUT: &[Field] = &[
    Field::required("task", Type::Object(TASK_FIELDS), "Task record"),
    Field::required("body_markdown", Type::String, "Task description markdown"),
    Field::required(
        "checklist",
        Type::List(&CHECKLIST_STEP_TYPE),
        "Checklist steps",
    ),
    Field::required("comments", Type::List(&COMMENT_TYPE), "Public comments"),
    Field::required(
        "depends_on_tasks",
        Type::List(&TASK_REF_TYPE),
        "Direct upstream dependency summaries",
    ),
    Field::required(
        "dependent_tasks",
        Type::List(&TASK_REF_TYPE),
        "Direct downstream dependent summaries",
    ),
    Field::required("history", Type::List(&EVENT_TYPE), "Immutable task history"),
];

pub(super) const TASK_DEPS_OUTPUT: &[Field] = &[
    Field::required("task", Type::Object(TASK_REF_FIELDS), "Root task"),
    Field::required(
        "direct_depends_on",
        Type::List(&TASK_REF_TYPE),
        "Direct upstream dependencies",
    ),
    Field::required(
        "direct_dependents",
        Type::List(&TASK_REF_TYPE),
        "Direct downstream dependents",
    ),
    Field::required(
        "transitive_depends_on",
        Type::List(&TASK_REF_TYPE),
        "All transitive upstream dependencies",
    ),
    Field::required(
        "transitive_dependents",
        Type::List(&TASK_REF_TYPE),
        "All transitive downstream dependents",
    ),
    Field::required(
        "edges",
        Type::List(&DEP_EDGE_TYPE),
        "Subgraph dependency edges",
    ),
];

pub(super) const TASK_LABEL_OUTPUT: &[Field] = &[
    Field::required("label", Type::String, "Normalized label"),
    Field::required(
        "task_count",
        Type::Integer,
        "Number of tasks with this label",
    ),
];

pub(super) const TASK_COMMENT_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
    Field::required("revision", Type::Integer, "New task revision"),
    Field::required("comment", Type::Object(COMMENT_FIELDS), "Appended comment"),
];

pub(super) const TASK_CREATE_INPUT: &[Field] = &[
    Field::required("title", Type::String, "Single-line task title"),
    Field::optional(
        "team_id",
        Type::String,
        "Owning team UUIDv7 (optional if project_id set)",
    ),
    Field::optional(
        "project_id",
        Type::String,
        "Optional project ID, P-<n>, or slug",
    ),
    Field::optional("assigned_team_id", Type::String, "Assigned team UUIDv7"),
    Field::optional("assignee_id", Type::String, "Assigned employee UUIDv7"),
    Field::optional(
        "type",
        Type::String,
        "Task type (default: task; or bug, request)",
    ),
    Field::optional(
        "work_type",
        Type::Enum(&["build", "run"]),
        "Optional Build/Run classification",
    ),
    Field::optional("body", Type::String, "Markdown description"),
    Field::optional(
        "priority",
        Type::Enum(&["urgent", "high", "normal", "low"]),
        "Task priority (default: normal)",
    ),
    Field::optional(
        "effort",
        Type::Integer,
        "Optional nonnegative effort estimate",
    ),
    Field::optional("severity", Type::String, "Optional bug severity"),
    Field::optional(
        "order",
        Type::Integer,
        "Optional nonnegative rank/order index",
    ),
    Field::optional(
        "labels",
        Type::List(&STRING_TYPE),
        "Initial searchable labels",
    ),
    Field::optional(
        "reference_files",
        Type::List(&STRING_TYPE),
        "Initial reference file paths",
    ),
    Field::optional(
        "modified_files",
        Type::List(&STRING_TYPE),
        "Initial modified file paths",
    ),
    Field::optional(
        "depends_on",
        Type::List(&STRING_TYPE),
        "Initial upstream dependency task IDs or T-<CODE><N> keys",
    ),
];

pub(super) const TASK_LIST_INPUT: &[Field] = &[
    Field::optional("type", Type::String, "Filter by task type"),
    Field::optional(
        "status",
        Type::Enum(&["todo", "in_progress", "blocked", "cancelled", "done"]),
        "Filter by status",
    ),
    Field::optional("team_id", Type::String, "Filter by owning team UUIDv7"),
    Field::optional(
        "assigned_team_id",
        Type::String,
        "Filter by assigned team UUIDv7",
    ),
    Field::optional("assignee_id", Type::String, "Filter by assignee UUIDv7"),
    Field::optional("project_id", Type::String, "Filter by project UUIDv7"),
    Field::optional(
        "labels",
        Type::List(&STRING_TYPE),
        "Filter by one or more labels",
    ),
    Field::optional(
        "label_match",
        Type::Enum(&["all", "any"]),
        "Multi-label match mode: all (AND, default) or any (OR)",
    ),
    Field::optional(
        "depends_on",
        Type::String,
        "Filter to tasks depending on this task",
    ),
    Field::optional(
        "dependent_of",
        Type::String,
        "Filter to tasks this task depends on",
    ),
    Field::optional(
        "query",
        Type::String,
        "Substring search over key, title, body, labels, files",
    ),
    Field::optional(
        "sort",
        Type::Enum(&["order", "number", "updated_desc"]),
        "Sort order (default: order)",
    ),
    Field::optional(
        "exclude_status",
        Type::List(&TASK_STATUS_TYPE),
        "Leave out tasks in this status (repeat for several)",
    ),
    Field::optional(
        "unassigned",
        Type::Boolean,
        "Only tasks no employee is assigned to",
    ),
    Field::optional(
        "modified_file",
        Type::String,
        "Only tasks whose modified files name this path or a file under it",
    ),
    Field::optional(
        "count",
        Type::Boolean,
        "Answer no tasks, only how many match (meta.total)",
    ),
];

const TASK_STATUS_TYPE: Type = Type::Enum(&["todo", "in_progress", "blocked", "cancelled", "done"]);

pub(super) const TASK_REF_INPUT: &[Field] = &[Field::required(
    "task",
    Type::String,
    "Task UUIDv7 or T-<CODE><N> key",
)];

pub(super) const TASK_UPDATE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("title", Type::String, "Updated title"),
    Field::optional("body", Type::String, "Updated Markdown description"),
    Field::optional("type", Type::String, "Updated task type"),
    Field::optional(
        "priority",
        Type::Enum(&["urgent", "high", "normal", "low"]),
        "Updated priority",
    ),
    Field::optional("effort", Type::Integer, "Updated effort estimate"),
    Field::optional("severity", Type::String, "Updated severity"),
    Field::optional("order", Type::Integer, "Updated rank/order index"),
    Field::optional("clear_order", Type::Boolean, "Clear rank/order index"),
    Field::optional("labels", Type::List(&STRING_TYPE), "Replace all labels"),
    Field::optional("add_labels", Type::List(&STRING_TYPE), "Add labels"),
    Field::optional("remove_labels", Type::List(&STRING_TYPE), "Remove labels"),
    Field::optional(
        "reference_files",
        Type::List(&STRING_TYPE),
        "Replace all reference files",
    ),
    Field::optional(
        "add_reference_files",
        Type::List(&STRING_TYPE),
        "Add reference files",
    ),
    Field::optional(
        "remove_reference_files",
        Type::List(&STRING_TYPE),
        "Remove reference files",
    ),
    Field::optional(
        "modified_files",
        Type::List(&STRING_TYPE),
        "Replace all modified files",
    ),
    Field::optional(
        "add_modified_files",
        Type::List(&STRING_TYPE),
        "Add modified files",
    ),
    Field::optional(
        "remove_modified_files",
        Type::List(&STRING_TYPE),
        "Remove modified files",
    ),
];

pub(super) const TASK_TRANSITION_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required(
        "status",
        Type::Enum(&["todo", "in_progress", "blocked", "cancelled", "done"]),
        "Target status",
    ),
    Field::optional(
        "blocked_reason",
        Type::String,
        "Required when transitioning to blocked",
    ),
    Field::optional("resolution", Type::String, "Optional closure resolution"),
    Field::optional("note", Type::String, "Optional transition note"),
];

pub(super) const TASK_ASSIGN_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("assigned_team_id", Type::String, "Assigned team UUIDv7"),
    Field::optional("clear_assigned_team", Type::Boolean, "Clear assigned team"),
    Field::optional("assignee_id", Type::String, "Assigned employee UUIDv7"),
    Field::optional("clear_assignee", Type::Boolean, "Clear assignee"),
];

pub(super) const TASK_COMMENT_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("body", Type::String, "Public shared comment markdown"),
];

pub(super) const TASK_DEP_MUTATION_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required(
        "depends_on",
        Type::String,
        "Upstream prerequisite task UUIDv7 or T-<CODE><N> key",
    ),
];

pub(super) const TASK_LABELS_INPUT: &[Field] = &[Field::optional(
    "query",
    Type::String,
    "Optional substring search over labels",
)];

pub(super) const TASK_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","type":"task","title":"Implement auth","status":"todo","priority":"high","team_id":"0193a5c0-0000-7000-8000-000000000010","rank_order":10,"labels":["backend","security"],"reference_files":["docs/auth.md"],"modified_files":["src/auth.rs"],"depends_on":[],"dependents":[],"revision":1,"checklist_total":0,"checklist_done":0,"comment_count":0,"watcher_count":0}"#;
