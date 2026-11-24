//! Unified v1 `task` and `checklist` command rows on the shared `org-command` grammar
//! (`TASK-49` AC #3 `CLI-15` + `TASK-77` B4b: dependencies, searchable labels, reference/modified
//! files, and task ordering).

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

const STRING_TYPE: Type = Type::String;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::AlreadyExists,
    ErrorCode::NotFound,
    ErrorCode::Conflict,
    ErrorCode::TooLarge,
    ErrorCode::OperationReused,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const TASK_FIELDS: &[Field] = &[
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

const TASK_REF_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key (T-<CODE><N>)"),
    Field::required("title", Type::String, "Task title"),
    Field::required("type", Type::String, "Task type"),
    Field::required("status", Type::String, "Task status"),
    Field::required("priority", Type::String, "Task priority"),
    Field::optional("rank_order", Type::Integer, "Execution rank/order index"),
];
const TASK_REF_TYPE: Type = Type::Object(TASK_REF_FIELDS);

const CHECKLIST_STEP_FIELDS: &[Field] = &[
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
const CHECKLIST_STEP_TYPE: Type = Type::Object(CHECKLIST_STEP_FIELDS);

const COMMENT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Comment UUIDv7"),
    Field::required("position", Type::Integer, "Zero-based comment position"),
    Field::required("author_id", Type::String, "Author employee UUIDv7"),
    Field::required("created_at", Type::String, "RFC 3339 creation timestamp"),
    Field::required("body_markdown", Type::String, "Public comment body"),
];
const COMMENT_TYPE: Type = Type::Object(COMMENT_FIELDS);

const EVENT_FIELDS: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("revision", Type::Integer, "Task revision"),
    Field::optional("occurred_at", Type::String, "RFC 3339 timestamp"),
    Field::required("actor_kind", Type::String, "Actor kind"),
    Field::optional("actor_id", Type::String, "Actor employee UUIDv7"),
    Field::required("payload_sha256", Type::String, "Event SHA-256 digest"),
];
const EVENT_TYPE: Type = Type::Object(EVENT_FIELDS);

const DEP_EDGE_FIELDS: &[Field] = &[
    Field::required("task_id", Type::String, "Dependent task UUIDv7"),
    Field::required(
        "depends_on_task_id",
        Type::String,
        "Prerequisite task UUIDv7",
    ),
];
const DEP_EDGE_TYPE: Type = Type::Object(DEP_EDGE_FIELDS);

const TASK_INSPECT_OUTPUT: &[Field] = &[
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

const TASK_DEPS_OUTPUT: &[Field] = &[
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

const TASK_LABEL_OUTPUT: &[Field] = &[
    Field::required("label", Type::String, "Normalized label"),
    Field::required(
        "task_count",
        Type::Integer,
        "Number of tasks with this label",
    ),
];

const TASK_COMMENT_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
    Field::required("revision", Type::Integer, "New task revision"),
    Field::required("comment", Type::Object(COMMENT_FIELDS), "Appended comment"),
];

const CHECKLIST_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
    Field::required("revision", Type::Integer, "New task revision"),
    Field::required(
        "checklist",
        Type::List(&CHECKLIST_STEP_TYPE),
        "Updated checklist steps",
    ),
];

const TASK_CREATE_INPUT: &[Field] = &[
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

const TASK_LIST_INPUT: &[Field] = &[
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
    Field::optional("max_results", Type::Integer, "Maximum tasks returned"),
];

const TASK_REF_INPUT: &[Field] = &[Field::required(
    "task",
    Type::String,
    "Task UUIDv7 or T-<CODE><N> key",
)];

const TASK_UPDATE_INPUT: &[Field] = &[
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

const TASK_TRANSITION_INPUT: &[Field] = &[
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

const TASK_ASSIGN_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("assigned_team_id", Type::String, "Assigned team UUIDv7"),
    Field::optional("clear_assigned_team", Type::Boolean, "Clear assigned team"),
    Field::optional("assignee_id", Type::String, "Assigned employee UUIDv7"),
    Field::optional("clear_assignee", Type::Boolean, "Clear assignee"),
];

const TASK_COMMENT_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("body", Type::String, "Public shared comment markdown"),
];

const TASK_DEP_MUTATION_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required(
        "depends_on",
        Type::String,
        "Upstream prerequisite task UUIDv7 or T-<CODE><N> key",
    ),
];

const TASK_LABELS_INPUT: &[Field] = &[Field::optional(
    "query",
    Type::String,
    "Optional substring search over labels",
)];

const CHECKLIST_ADD_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("text", Type::String, "Checklist step text"),
    Field::optional("required", Type::Boolean, "Mark step as required"),
];

const CHECKLIST_CHECK_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
    Field::optional("done", Type::Boolean, "Mark step done (default)"),
    Field::optional("undone", Type::Boolean, "Mark step not done"),
];

const CHECKLIST_EDIT_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
    Field::optional("text", Type::String, "Updated step text"),
    Field::optional("required", Type::Boolean, "Updated required flag"),
];

const CHECKLIST_REMOVE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
];

const CHECKLIST_MOVE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
    Field::required("position", Type::Integer, "Zero-based target position"),
];

const NOTE_OUTPUT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Private note UUIDv7"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("author_id", Type::String, "Author employee UUIDv7"),
    Field::required("revision", Type::Integer, "Note revision"),
    Field::required("body_markdown", Type::String, "Private Markdown body"),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("updated_at", Type::String, "Update timestamp"),
    Field::required("editable", Type::Boolean, "True if caller is author"),
];

const NOTE_OUTPUT: Type = Type::Object(NOTE_OUTPUT_FIELDS);

const NOTE_REMOVED_FIELDS: &[Field] = &[
    Field::required("note_id", Type::String, "Deleted private note UUIDv7"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("revision", Type::Integer, "Tombstone revision"),
    Field::required("deleted_at", Type::String, "Deletion timestamp"),
];

const NOTE_PUBLISH_FIELDS: &[Field] = &[
    Field::required("note", NOTE_OUTPUT, "Private note (unchanged)"),
    Field::required("comment", COMMENT_TYPE, "Published shared comment"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
];

const NOTE_ADD_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("text", Type::String, "Private note Markdown text"),
];

const NOTE_LIST_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("author_id", Type::String, "Filter by author ID (people)"),
];

const NOTE_ID_INPUT: &[Field] = &[Field::required("id", Type::String, "Private note UUIDv7")];

const NOTE_UPDATE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Private note UUIDv7"),
    Field::required("text", Type::String, "Updated private note text"),
];

const NOTE_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true}"#;

const TASK_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","type":"task","title":"Implement auth","status":"todo","priority":"high","team_id":"0193a5c0-0000-7000-8000-000000000010","rank_order":10,"labels":["backend","security"],"reference_files":["docs/auth.md"],"modified_files":["src/auth.rs"],"depends_on":[],"dependents":[],"revision":1,"checklist_total":0,"checklist_done":0,"comment_count":0,"watcher_count":0}"#;

const CHECKLIST_EXAMPLE_JSON: &str = r#"{"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","revision":2,"checklist":[{"id":"0193a5c0-0000-7000-8000-000000000200","position":0,"text":"Add tests","required":true,"done":false,"revision":1}]}"#;

pub const TASK_COMMANDS: &[Command] = &[
    Command {
        resource: "task",
        verb: "create",
        summary: "Create a task, bug, or request",
        call: "tasks.create",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: TASK_CREATE_INPUT,
        content_field: Some("body"),
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"title":"Implement auth","team_id":"0193a5c0-0000-7000-8000-000000000010","order":10,"labels":["backend","security"]}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "list",
        summary: "List and filter tasks",
        call: "tasks.list",
        kind: Kind::List,
        input: TASK_LIST_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"labels":["backend","security"],"label_match":"all","sort":"order"}"#,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","type":"task","title":"Implement auth","status":"todo","priority":"high","team_id":"0193a5c0-0000-7000-8000-000000000010","rank_order":10,"labels":["backend","security"],"reference_files":["docs/auth.md"],"modified_files":["src/auth.rs"],"depends_on":[],"dependents":[],"revision":1,"checklist_total":0,"checklist_done":0,"comment_count":0,"watcher_count":0}]"#,
        },
    },
    Command {
        resource: "task",
        verb: "inspect",
        summary: "Inspect a task",
        call: "tasks.inspect",
        kind: Kind::Read,
        input: TASK_REF_INPUT,
        content_field: None,
        output: TASK_INSPECT_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"{"task":{"id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","type":"task","title":"Implement auth","status":"todo","priority":"high","team_id":"0193a5c0-0000-7000-8000-000000000010","rank_order":10,"labels":["backend","security"],"reference_files":["docs/auth.md"],"modified_files":["src/auth.rs"],"depends_on":[],"dependents":[],"revision":1,"checklist_total":0,"checklist_done":0,"comment_count":0,"watcher_count":0},"body_markdown":"","checklist":[],"comments":[],"depends_on_tasks":[],"dependent_tasks":[],"history":[]}"#,
        },
    },
    Command {
        resource: "task",
        verb: "update",
        summary: "Update task fields, labels, files, or order",
        call: "tasks.update",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: TASK_UPDATE_INPUT,
        content_field: Some("body"),
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","order":1,"add_labels":["p0"]}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "transition",
        summary: "Change task status",
        call: "tasks.transition",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: TASK_TRANSITION_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","status":"in_progress"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "assign",
        summary: "Assign task team or employee",
        call: "tasks.assign",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: TASK_ASSIGN_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","assignee_id":"0193a5c0-0000-7000-8000-000000000020"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "comment",
        summary: "Add a public task comment",
        call: "tasks.comment",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: TASK_COMMENT_INPUT,
        content_field: Some("body"),
        output: TASK_COMMENT_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","body":"Ready for review."}"#,
            output: r#"{"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","revision":2,"comment":{"id":"0193a5c0-0000-7000-8000-000000000300","position":0,"author_id":"0193a5c0-0000-7000-8000-000000000020","created_at":"2026-11-22T00:00:00.000Z","body_markdown":"Ready for review."}}"#,
        },
    },
    Command {
        resource: "task",
        verb: "watch",
        summary: "Watch a task",
        call: "tasks.watch",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: TASK_REF_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "unwatch",
        summary: "Stop watching a task",
        call: "tasks.unwatch",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: TASK_REF_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "history",
        summary: "List task history",
        call: "tasks.history",
        kind: Kind::List,
        input: TASK_REF_INPUT,
        content_field: None,
        output: EVENT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"[{"task_id":"0193a5c0-0000-7000-8000-000000000100","revision":1,"occurred_at":"2026-11-22T00:00:00.000Z","actor_kind":"employee","actor_id":"0193a5c0-0000-7000-8000-000000000020","payload_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]"#,
        },
    },
    Command {
        resource: "task",
        verb: "deps",
        summary: "Show task dependency graph",
        call: "tasks.dependencies",
        kind: Kind::Read,
        input: TASK_REF_INPUT,
        content_field: None,
        output: TASK_DEPS_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A2"}"#,
            output: r#"{"task":{"id":"0193a5c0-0000-7000-8000-000000000101","key":"T-A2","title":"Adapter","type":"task","status":"todo","priority":"normal"},"direct_depends_on":[],"direct_dependents":[],"transitive_depends_on":[],"transitive_dependents":[],"edges":[]}"#,
        },
    },
    Command {
        resource: "task",
        verb: "dep-add",
        summary: "Add task dependency",
        call: "tasks.add_dependency",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: TASK_DEP_MUTATION_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A2","depends_on":"T-A1"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "dep-remove",
        summary: "Remove task dependency",
        call: "tasks.remove_dependency",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: TASK_DEP_MUTATION_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A2","depends_on":"T-A1"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "labels",
        summary: "Search task labels",
        call: "tasks.labels",
        kind: Kind::List,
        input: TASK_LABELS_INPUT,
        content_field: None,
        output: TASK_LABEL_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"query":"sec"}"#,
            output: r#"[{"label":"security","task_count":2}]"#,
        },
    },
    Command {
        resource: "checklist",
        verb: "add",
        summary: "Add checklist step",
        call: "tasks.checklist.add",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_ADD_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","text":"Add tests","required":true}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "check",
        summary: "Check or uncheck step",
        call: "tasks.checklist.check",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_CHECK_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","done":true}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "edit",
        summary: "Edit checklist step",
        call: "tasks.checklist.edit",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_EDIT_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","text":"Add integration tests"}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "remove",
        summary: "Remove checklist step",
        call: "tasks.checklist.remove",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_REMOVE_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200"}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "move",
        summary: "Move checklist step",
        call: "tasks.checklist.move",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_MOVE_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","position":0}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task-note",
        verb: "add",
        summary: "Add private note",
        call: "tasks.notes.add",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: NOTE_ADD_INPUT,
        content_field: Some("text"),
        output: NOTE_OUTPUT_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","text":"Check race condition"}"#,
            output: NOTE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task-note",
        verb: "list",
        summary: "List private notes",
        call: "tasks.notes.list",
        kind: Kind::List,
        input: NOTE_LIST_INPUT,
        content_field: None,
        output: NOTE_OUTPUT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true}]"#,
        },
    },
    Command {
        resource: "task-note",
        verb: "get",
        summary: "Get private note",
        call: "tasks.notes.get",
        kind: Kind::Read,
        input: NOTE_ID_INPUT,
        content_field: None,
        output: NOTE_OUTPUT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000400"}"#,
            output: NOTE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task-note",
        verb: "update",
        summary: "Edit private note",
        call: "tasks.notes.update",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: NOTE_UPDATE_INPUT,
        content_field: Some("text"),
        output: NOTE_OUTPUT_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000400","text":"Updated plan"}"#,
            output: NOTE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task-note",
        verb: "remove",
        summary: "Delete private note",
        call: "tasks.notes.remove",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: NOTE_ID_INPUT,
        content_field: None,
        output: NOTE_REMOVED_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000400"}"#,
            output: r#"{"note_id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","revision":2,"deleted_at":"2026-11-22T00:05:00.000Z"}"#,
        },
    },
    Command {
        resource: "task-note",
        verb: "publish",
        summary: "Publish note copy",
        call: "tasks.notes.publish",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: NOTE_ID_INPUT,
        content_field: None,
        output: NOTE_PUBLISH_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000400"}"#,
            output: r#"{"note":{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true},"comment":{"id":"0193a5c0-0000-7000-8000-000000000300","position":0,"author_id":"0193a5c0-0000-7000-8000-000000000001","created_at":"2026-11-22T00:01:00.000Z","body_markdown":"Check race condition"},"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1"}"#,
        },
    },
];
