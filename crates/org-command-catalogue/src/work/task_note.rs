//! Declarative rows for `task-note add|list|get|update|remove|publish` and the `task note` alias.

use org_command::{Ack, Alias, Command, Example, Field, IfMatch, Kind, Type};

use crate::shared::STRING_TYPE;

use super::schema::{COMMENT_TYPE, READ_ERRORS, WRITE_ERRORS};

/// `task note <verb>` is `task-note <verb>`.
pub const TASK_NOTE_ALIAS: Alias = Alias {
    words: &["task", "note"],
    command: &["task-note"],
};

const NOTE_OUTPUT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Private note UUIDv7"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("author_id", Type::String, "Author employee UUIDv7"),
    Field::required("revision", Type::Integer, "Note revision"),
    Field::optional(
        "kind",
        Type::Enum(&["note", "plan", "decision", "unknown"]),
        "Private note kind",
    ),
    Field::optional("title", Type::String, "Note title"),
    Field::optional("tags", Type::List(&STRING_TYPE), "Normalized note tags"),
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
    Field::optional(
        "checkpoint",
        Type::Boolean,
        "Upsert your single pinned working-checkpoint note on this task",
    ),
];

const NOTE_LIST_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("author_id", Type::String, "Filter by author ID (people)"),
    Field::optional(
        "checkpoint",
        Type::Boolean,
        "Filter to working-checkpoint notes only",
    ),
];

const NOTE_ID_INPUT: &[Field] = &[Field::required("id", Type::String, "Private note UUIDv7")];

const NOTE_UPDATE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Private note UUIDv7"),
    Field::required("text", Type::String, "Updated private note text"),
];

const NOTE_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true}"#;

/// The `task-note` rows.
pub const TASK_NOTE_COMMANDS: &[Command] = &[
    Command {
        resource: "task-note",
        verb: "add",
        summary: "Add private note",
        call: "tasks.notes.add",
        kind: Kind::Write {
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
