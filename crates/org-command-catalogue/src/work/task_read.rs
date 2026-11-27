//! Declarative rows for `task list|inspect|history|deps|labels`.

use org_command::{Command, Example, Field, Kind, Type};

use super::schema::{
    ATTACHMENT_META_TYPE, CHECKLIST_STEP_TYPE, COMMENT_TYPE, DEP_EDGE_TYPE, EVENT_FIELDS,
    EVENT_TYPE, READ_ERRORS, TASK_FIELDS, TASK_REF_FIELDS, TASK_REF_INPUT, TASK_REF_TYPE,
    WATCHER_TYPE,
};
use crate::shared::STRING_TYPE;

const TASK_STATUS_TYPE: Type = Type::Enum(&["todo", "in_progress", "blocked", "cancelled", "done"]);

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
        "FTS5 search over key, title, body, details, labels, files, comments and checklist",
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
        "all",
        Type::Boolean,
        "Include closed tasks (shorthand for --status-scope all)",
    ),
    Field::optional(
        "status_scope",
        Type::Enum(&["open", "closed", "all"]),
        "Lifecycle scope: open (default), closed, or all",
    ),
    Field::optional(
        "done_since",
        Type::String,
        "Include done tasks updated at or after RFC 3339 timestamp",
    ),
    Field::optional(
        "ready",
        Type::Boolean,
        "Only ready tasks (open, not explicitly blocked, not dependency-blocked)",
    ),
    Field::optional(
        "dependency_blocked",
        Type::Boolean,
        "Only tasks with unmet upstream depends_on prerequisites",
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

const TASK_HISTORY_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional(
        "event_type",
        Type::String,
        "Filter history events by change.type (e.g. status_changed)",
    ),
    Field::optional(
        "blocks_only",
        Type::Boolean,
        "Keep only block, blocked-reason edit, and unblock status_changed events",
    ),
];

const TASK_INSPECT_OUTPUT: &[Field] = &[
    Field::required("task", Type::Object(TASK_FIELDS), "Task record"),
    Field::optional("body_markdown", Type::String, "Task description markdown"),
    Field::optional(
        "checklist",
        Type::List(&CHECKLIST_STEP_TYPE),
        "Checklist steps",
    ),
    Field::optional("comments", Type::List(&COMMENT_TYPE), "Public comments"),
    Field::optional(
        "attachments",
        Type::List(&ATTACHMENT_META_TYPE),
        "Files attached to this task",
    ),
    Field::optional("watchers", Type::List(&WATCHER_TYPE), "Task watchers"),
    Field::optional(
        "depends_on_tasks",
        Type::List(&TASK_REF_TYPE),
        "Direct upstream dependency summaries",
    ),
    Field::optional(
        "dependent_tasks",
        Type::List(&TASK_REF_TYPE),
        "Direct downstream dependent summaries",
    ),
    Field::optional("history", Type::List(&EVENT_TYPE), "Immutable task history"),
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

const TASK_LABELS_INPUT: &[Field] = &[Field::optional(
    "query",
    Type::String,
    "Optional substring search over labels",
)];

const TASK_LABEL_OUTPUT: &[Field] = &[
    Field::required("label", Type::String, "Normalized label"),
    Field::required(
        "task_count",
        Type::Integer,
        "Number of tasks with this label",
    ),
];

/// The `task` read rows.
pub const TASK_READ_COMMANDS: &[Command] = &[
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
            output: r#"{"task":{"id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","type":"task","title":"Implement auth","status":"todo","priority":"high","team_id":"0193a5c0-0000-7000-8000-000000000010","rank_order":10,"labels":["backend","security"],"reference_files":["docs/auth.md"],"modified_files":["src/auth.rs"],"depends_on":[],"dependents":[],"revision":1,"checklist_total":0,"checklist_done":0,"comment_count":0,"watcher_count":0},"body_markdown":"","checklist":[],"comments":[],"attachments":[],"watchers":[],"depends_on_tasks":[],"dependent_tasks":[],"history":[]}"#,
        },
    },
    Command {
        resource: "task",
        verb: "history",
        summary: "List task history",
        call: "tasks.history",
        kind: Kind::List,
        input: TASK_HISTORY_INPUT,
        content_field: None,
        output: EVENT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"[{"task_id":"0193a5c0-0000-7000-8000-000000000100","revision":1,"occurred_at":"2026-11-22T00:00:00.000Z","actor_kind":"employee","actor_id":"0193a5c0-0000-7000-8000-000000000020","change":{"type":"created"},"payload_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]"#,
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
];
