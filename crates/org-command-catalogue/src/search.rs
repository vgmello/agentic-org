//! Declarative command row for unified cross-entity search (`TASK-91` AC #3):
//! `search query` (`search.query`).

use org_command::{Command, Example, Field, Kind, Type};
use org_proto::ErrorCode;

const SEARCH_ENTITY_TYPES: &[&str] = &[
    "task",
    "process",
    "project",
    "member",
    "team",
    "role",
    "message",
    "private_note",
    "private_decision",
];

const SEARCH_ENTITY_TYPE: Type = Type::Enum(SEARCH_ENTITY_TYPES);

const SEARCH_STATUSES: &[&str] = &[
    "todo",
    "in_progress",
    "blocked",
    "cancelled",
    "done",
    "active",
    "paused",
    "inactive",
    "archived",
    "draft",
    "current",
    "deprecated",
];

const SEARCH_QUERY_INPUT: &[Field] = &[
    Field::required("q", Type::String, "Search query words (FTS5 prefix + stem)"),
    Field::optional(
        "types",
        Type::List(&SEARCH_ENTITY_TYPE),
        "Entity types to search (private_note/private_decision only on their own)",
    ),
    Field::optional(
        "project",
        Type::String,
        "Filter by project UUIDv7, P-<n> or slug",
    ),
    Field::optional(
        "channel",
        Type::String,
        "Filter messages by conversation UUIDv7, #name or name",
    ),
    Field::optional(
        "status",
        Type::Enum(SEARCH_STATUSES),
        "Filter by task, process, member, team or project status",
    ),
    Field::optional(
        "modified_file",
        Type::String,
        "Filter tasks by modified repository file path",
    ),
    Field::optional(
        "assignee",
        Type::String,
        "Filter tasks by assignee @handle or UUIDv7",
    ),
    Field::optional(
        "label",
        Type::String,
        "Filter tasks by label or private notes by tag",
    ),
    Field::optional(
        "author_id",
        Type::String,
        "Filter messages or private notes by author UUIDv7 (people for others' notes)",
    ),
    Field::optional(
        "count",
        Type::Boolean,
        "Return only the matching hit count and coverage (no hits)",
    ),
];

const SEARCH_HIT_OUTPUT: &[Field] = &[
    Field::required("entity_type", SEARCH_ENTITY_TYPE, "Matched entity type"),
    Field::required("entity_id", Type::String, "Matched entity UUIDv7"),
    Field::optional(
        "ref",
        Type::String,
        "Readable reference (T-A1, P-1, @handle)",
    ),
    Field::optional("title", Type::String, "Matched title or name"),
    Field::optional("excerpt", Type::String, "Short match excerpt"),
    Field::optional("score", Type::Json, "Field-weighted BM25 relevance score"),
    Field::optional("updated_at", Type::String, "Last update timestamp"),
    Field::optional("status", Type::String, "Entity status"),
    Field::optional("project_id", Type::String, "Project UUIDv7"),
    Field::optional("conversation_id", Type::String, "Conversation UUIDv7"),
    Field::optional("task_id", Type::String, "Task UUIDv7"),
    Field::optional("author_id", Type::String, "Author UUIDv7"),
];

const SEARCH_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

/// The `search` command rows.
pub const SEARCH_COMMANDS: &[Command] = &[Command {
    resource: "search",
    verb: "query",
    summary: "Search tasks, processes, messages, directory and private notes",
    call: "search.query",
    kind: Kind::List,
    input: SEARCH_QUERY_INPUT,
    content_field: None,
    output: SEARCH_HIT_OUTPUT,
    errors: SEARCH_ERRORS,
    example: Example {
        input: r#"{"q":"auth timeout","types":["task","process"]}"#,
        output: r#"[{"entity_type":"task","entity_id":"0193a5c0-0000-7000-8000-000000000100","ref":"T-A1","title":"Fix auth timeout","excerpt":"...auth timeout...","score":1.5,"updated_at":"2026-11-22T00:00:00.000Z","status":"in_progress"}]"#,
    },
}];

/// Every command row slice of the search domain.
pub const ALL: &[&[Command]] = &[SEARCH_COMMANDS];
