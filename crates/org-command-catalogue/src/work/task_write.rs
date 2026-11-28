//! Declarative rows for `task create|update|transition|assign|comment|watch|unwatch|dep-add|dep-remove`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use super::schema::{COMMENT_FIELDS, TASK_EXAMPLE_JSON, TASK_FIELDS, TASK_REF_INPUT, WRITE_ERRORS};
use crate::shared::STRING_TYPE;

const TASK_COMMENT_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
    Field::required("revision", Type::Integer, "New task revision"),
    Field::required("comment", Type::Object(COMMENT_FIELDS), "Appended comment"),
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
    Field::optional(
        "attach",
        Type::List(&STRING_TYPE),
        "Local file path to upload and attach (repeatable)",
    ),
    Field::optional(
        "attachment_id",
        Type::List(&STRING_TYPE),
        "Pre-uploaded attachment UUIDv7 to attach (repeatable)",
    ),
];

const TASK_UPDATE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("title", Type::String, "Updated title"),
    Field::optional(
        "body",
        Type::String,
        "Replace Markdown description (mutually exclusive with --append-body)",
    ),
    Field::optional(
        "append_body",
        Type::String,
        "Append Markdown block to description (mutually exclusive with --body)",
    ),
    Field::optional("type", Type::String, "Updated task type"),
    Field::optional(
        "work_type",
        Type::Enum(&["build", "run"]),
        "Updated Build/Run classification",
    ),
    Field::optional(
        "clear_work_type",
        Type::Boolean,
        "Clear Build/Run classification",
    ),
    Field::optional(
        "priority",
        Type::Enum(&["urgent", "high", "normal", "low"]),
        "Updated priority",
    ),
    Field::optional("effort", Type::Integer, "Updated effort estimate"),
    Field::optional("clear_effort", Type::Boolean, "Clear effort estimate"),
    Field::optional("severity", Type::String, "Updated severity"),
    Field::optional("clear_severity", Type::Boolean, "Clear severity"),
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
    Field::optional(
        "attach",
        Type::List(&STRING_TYPE),
        "Local file path to upload and add to task attachments (repeatable)",
    ),
    Field::optional(
        "add_attachment",
        Type::List(&STRING_TYPE),
        "Attachment UUIDv7 to add to this task (repeatable)",
    ),
    Field::optional(
        "remove_attachment",
        Type::List(&STRING_TYPE),
        "Attachment UUIDv7 to remove from this task (repeatable)",
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
        "reason",
        Type::String,
        "Transition reason (required for blocked; optional note for other statuses)",
    ),
    Field::optional(
        "blocked_reason",
        Type::String,
        "Alias for --reason on blocked transitions only (refused for other statuses)",
    ),
    Field::optional(
        "note",
        Type::String,
        "Alias for --reason (optional transition note)",
    ),
    Field::optional(
        "waiting_on",
        Type::Enum(&["owner", "none"]),
        "Mark explicit block as waiting on owner (or clear with none; blocked only)",
    ),
    Field::optional(
        "waiting_on_owner",
        Type::Boolean,
        "Mark explicit block as waiting on owner action (blocked only)",
    ),
    Field::optional("resolution", Type::String, "Optional closure resolution"),
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
    Field::optional("body", Type::String, "Public shared comment markdown"),
    Field::optional(
        "attach",
        Type::List(&STRING_TYPE),
        "Local file path to upload and attach (repeatable)",
    ),
    Field::optional(
        "attachment_id",
        Type::List(&STRING_TYPE),
        "Pre-uploaded attachment UUIDv7 to attach (repeatable)",
    ),
];

const TASK_DEP_MUTATION_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required(
        "depends_on",
        Type::String,
        "Upstream prerequisite task UUIDv7 or T-<CODE><N> key",
    ),
];

/// The `task` write rows.
pub const TASK_WRITE_COMMANDS: &[Command] = &[
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
        verb: "update",
        summary: "Update task fields, labels, files, or order",
        call: "tasks.update",
        kind: Kind::Write {
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
        verb: "dep-add",
        summary: "Add task dependency",
        call: "tasks.add_dependency",
        kind: Kind::Write {
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
];
