//! Declarative rows for `note add|list|inspect|update|remove|publish` and the
//! `task-note` / `task note` aliases (`TASK-50`, `TASK-111` ADEC-3).

use org_command::{Ack, Alias, Command, Example, Field, IfMatch, Kind, Type};

use crate::shared::STRING_TYPE;

use super::schema::{COMMENT_TYPE, READ_ERRORS, WRITE_ERRORS};

/// `task-note <verb>` is `note <verb>`.
pub const TASK_NOTE_LEGACY_ALIAS: Alias = Alias {
    words: &["task-note"],
    command: &["note"],
};

/// `task note <verb>` is `note <verb>`.
pub const TASK_NOTE_ALIAS: Alias = Alias {
    words: &["task", "note"],
    command: &["note"],
};

const NOTE_KINDS: &[&str] = &["note", "plan", "decision"];
const NOTE_OUTPUT_KINDS: &[&str] = &["note", "plan", "decision", "unknown"];

const NOTE_OUTPUT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Private note UUIDv7"),
    Field::optional(
        "task_id",
        Type::String,
        "Task UUIDv7 (omitted for standalone notes)",
    ),
    Field::optional("project_id", Type::String, "Project UUIDv7"),
    Field::required("author_id", Type::String, "Author employee UUIDv7"),
    Field::required("revision", Type::Integer, "Note revision"),
    Field::optional("kind", Type::Enum(NOTE_OUTPUT_KINDS), "Private note kind"),
    Field::optional("title", Type::String, "Short note or decision title"),
    Field::optional("why", Type::String, "Rationale or decision drivers"),
    Field::optional(
        "alternatives",
        Type::String,
        "Considered alternatives and why they were not chosen",
    ),
    Field::optional("tags", Type::List(&STRING_TYPE), "Normalized note tags"),
    Field::optional(
        "body_markdown",
        Type::String,
        "Private Markdown body (omitted on default list unless requested via --fields)",
    ),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("updated_at", Type::String, "Update timestamp"),
    Field::required("editable", Type::Boolean, "True if caller is author"),
];

const NOTE_OUTPUT: Type = Type::Object(NOTE_OUTPUT_FIELDS);

const NOTE_REMOVED_FIELDS: &[Field] = &[
    Field::required("note_id", Type::String, "Deleted private note UUIDv7"),
    Field::optional("task_id", Type::String, "Task UUIDv7 (when task-bound)"),
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
    Field::required("text", Type::String, "Private note Markdown text"),
    Field::optional(
        "kind",
        Type::Enum(NOTE_KINDS),
        "Note kind: note (default), plan, or decision",
    ),
    Field::optional(
        "title",
        Type::String,
        "Short title (at most 200 characters)",
    ),
    Field::optional(
        "why",
        Type::String,
        "Rationale or trade-off behind this decision",
    ),
    Field::optional(
        "alternatives",
        Type::String,
        "Alternatives considered and rejected",
    ),
    Field::optional(
        "tag",
        Type::List(&STRING_TYPE),
        "Normalized tag (repeat for several, e.g. shortcut, assumption)",
    ),
    Field::optional(
        "task",
        Type::String,
        "Task UUIDv7 or T-<CODE><N> key (auto-bound from active run when omitted)",
    ),
    Field::optional(
        "standalone",
        Type::Boolean,
        "Store without binding to the active task run",
    ),
    Field::optional(
        "project",
        Type::String,
        "Project UUIDv7, P-<n> or slug (auto-inferred from task or run)",
    ),
    Field::optional(
        "checkpoint",
        Type::Boolean,
        "Upsert your single pinned working-checkpoint note on this task",
    ),
];

const NOTE_LIST_INPUT: &[Field] = &[
    Field::optional(
        "task",
        Type::String,
        "Filter by task UUIDv7 or T-<CODE><N> key",
    ),
    Field::optional(
        "project",
        Type::String,
        "Filter by project UUIDv7, P-<n> or slug",
    ),
    Field::optional(
        "kind",
        Type::Enum(NOTE_KINDS),
        "Filter by kind: note, plan, or decision",
    ),
    Field::optional("tag", Type::String, "Filter by normalized tag"),
    Field::optional(
        "since",
        Type::String,
        "Filter to notes updated at or after an RFC 3339 timestamp",
    ),
    Field::optional(
        "author",
        Type::String,
        "Filter by author @handle or UUIDv7 (people only)",
    ),
    Field::optional(
        "checkpoint",
        Type::Boolean,
        "Filter to working-checkpoint notes only",
    ),
    Field::optional(
        "count",
        Type::Boolean,
        "Return only the matching note count (meta.total)",
    ),
];

const NOTE_ID_INPUT: &[Field] = &[Field::required("id", Type::String, "Private note UUIDv7")];

const NOTE_UPDATE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Private note UUIDv7"),
    Field::optional(
        "kind",
        Type::Enum(NOTE_KINDS),
        "Updated kind: note, plan, or decision",
    ),
    Field::optional("title", Type::String, "Updated title"),
    Field::optional("why", Type::String, "Updated rationale"),
    Field::optional("alternatives", Type::String, "Updated alternatives"),
    Field::optional(
        "tag",
        Type::List(&STRING_TYPE),
        "Replacement tag list (repeat for several)",
    ),
    Field::optional("text", Type::String, "Updated private note Markdown text"),
];

const NOTE_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"kind":"decision","title":"Use SQLite FTS5","why":"Local offline search with zero extra service","tags":["search"],"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true}"#;

const NOTE_LIST_EXAMPLE_JSON: &str = r#"[{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"kind":"decision","title":"Use SQLite FTS5","why":"Local offline search with zero extra service","tags":["search"],"created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true}]"#;

/// The `note` rows (formerly `task-note`; aliased as `task-note` and `task note`).
pub const TASK_NOTE_COMMANDS: &[Command] = &[
    Command {
        resource: "note",
        verb: "add",
        summary: "Add private note, plan or decision",
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
            input: r#"{"kind":"decision","title":"Use SQLite FTS5","why":"Local offline search","tag":["search"],"text":"Index shared knowledge locally with FTS5."}"#,
            output: NOTE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "note",
        verb: "list",
        summary: "List private notes, plans and decisions",
        call: "tasks.notes.list",
        kind: Kind::List,
        input: NOTE_LIST_INPUT,
        content_field: None,
        output: NOTE_OUTPUT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"kind":"decision","task":"T-A1"}"#,
            output: NOTE_LIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "note",
        verb: "inspect",
        summary: "Inspect one private note",
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
        resource: "note",
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
        resource: "note",
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
        resource: "note",
        verb: "publish",
        summary: "Publish note copy as task comment",
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
            output: r#"{"note":{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"kind":"decision","title":"Use SQLite FTS5","why":"Local offline search with zero extra service","tags":["search"],"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true},"comment":{"id":"0193a5c0-0000-7000-8000-000000000300","position":0,"author_id":"0193a5c0-0000-7000-8000-000000000001","created_at":"2026-11-22T00:01:00.000Z","body_markdown":"Check race condition"},"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1"}"#,
        },
    },
];
