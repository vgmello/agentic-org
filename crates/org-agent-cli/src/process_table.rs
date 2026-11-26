//! `org-command` command table rows for C5 processes, standards, and built-in skills
//! (`TASK-44`, `doc-28` §3.5, `FC-110`, `FC-111`, `FC-112`, `FC-184`).

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const PROCESS_INSPECT_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Conflict,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::AlreadyExists,
    ErrorCode::Conflict,
    ErrorCode::OperationReused,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unsupported,
    ErrorCode::Unavailable,
];

const PROCESS_DOC_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Document UUIDv7"),
    Field::optional("key", Type::String, "Override key"),
    Field::required("revision", Type::Integer, "Document revision"),
    Field::required(
        "kind",
        Type::Enum(&["process", "standard"]),
        "Document kind",
    ),
    Field::required("title", Type::String, "Document title"),
    Field::required(
        "status",
        Type::Enum(&["draft", "current", "deprecated"]),
        "Lifecycle status",
    ),
    Field::required("level", Type::String, "Scope level"),
    Field::optional("team_id", Type::String, "Scoped team UUIDv7"),
    Field::optional("project_id", Type::String, "Scoped project UUIDv7"),
    Field::optional("scope_name", Type::String, "Human-readable scope name"),
    Field::required(
        "overridable",
        Type::Boolean,
        "Whether lower levels may override",
    ),
    Field::required("summary", Type::String, "One-line summary"),
];

const PROCESS_LIST_INPUT: &[Field] = &[
    Field::optional(
        "kind",
        Type::Enum(&["process", "standard"]),
        "Filter by kind",
    ),
    Field::optional("project", Type::String, "Project ID, P-<n>, or slug"),
    Field::optional("task", Type::String, "Task ID or reference"),
];

const PROCESS_LIST_ROW: &[Field] = &[
    Field::required("id", Type::String, "Key or document ID"),
    Field::optional("key", Type::String, "Override key"),
    Field::required(
        "kind",
        Type::Enum(&["process", "standard"]),
        "Document kind",
    ),
    Field::required("applies", Type::Boolean, "Whether the document applies"),
    Field::required(
        "mandatory",
        Type::Boolean,
        "Non-overridable must-follow flag",
    ),
    Field::required(
        "document",
        Type::Object(PROCESS_DOC_FIELDS),
        "Winning document",
    ),
];

const PROCESS_LAYER_FIELDS: &[Field] = &[
    Field::required(
        "document",
        Type::Object(PROCESS_DOC_FIELDS),
        "Document at this scope layer",
    ),
    Field::required("outcome", Type::String, "Resolution outcome at this layer"),
    Field::optional("detail", Type::String, "Optional resolution detail"),
];

const PROCESS_LAYER_TYPE: Type = Type::Object(PROCESS_LAYER_FIELDS);

const PROCESS_INSPECT_INPUT: &[Field] = &[
    Field::optional("key", Type::String, "Document override key"),
    Field::optional("id", Type::String, "Document UUIDv7"),
    Field::optional("why", Type::Boolean, "Show inheritance chain"),
    Field::optional("project", Type::String, "Project ID, P-<n>, or slug"),
    Field::optional("task", Type::String, "Task ID or reference"),
    Field::optional(
        "offset",
        Type::Integer,
        "Byte offset for chunked body reads",
    ),
    Field::optional(
        "revision",
        Type::Integer,
        "Pin document revision across chunked --offset reads",
    ),
];

const PROCESS_INSPECT_OUTPUT: &[Field] = &[
    Field::optional(
        "document",
        Type::Object(PROCESS_DOC_FIELDS),
        "Resolved document",
    ),
    Field::optional("content", Type::String, "Document Markdown body"),
    Field::optional(
        "total_bytes",
        Type::Integer,
        "Total document body size in UTF-8 bytes",
    ),
    Field::optional(
        "next_offset",
        Type::Integer,
        "Byte offset for the next body chunk when truncated",
    ),
    Field::optional(
        "parent",
        Type::Object(PROCESS_DOC_FIELDS),
        "Overridden parent document",
    ),
    Field::optional("key", Type::String, "Queried key (with --why)"),
    Field::optional(
        "chain",
        Type::List(&PROCESS_LAYER_TYPE),
        "Resolution inheritance chain (with --why)",
    ),
    Field::optional(
        "winner",
        Type::Object(PROCESS_DOC_FIELDS),
        "Winning document (with --why)",
    ),
];

const PROCESS_CREATE_INPUT: &[Field] = &[
    Field::required(
        "kind",
        Type::Enum(&["process", "standard"]),
        "process or standard",
    ),
    Field::required("title", Type::String, "Document title"),
    Field::optional("key", Type::String, "Optional lowercase key"),
    Field::optional("project", Type::String, "Project ID, P-<n>, or slug"),
    Field::optional("team", Type::String, "Team UUIDv7 (people only)"),
    Field::optional(
        "status",
        Type::Enum(&["draft", "current", "deprecated"]),
        "Status",
    ),
    Field::optional("owner", Type::String, "Accountable owner UUIDv7"),
    Field::optional(
        "non_overridable",
        Type::Boolean,
        "Make it must-follow: no lower level may replace it (default: overridable)",
    ),
    Field::required("body", Type::String, "Markdown body"),
];

const PROCESS_UPDATE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Document UUIDv7"),
    Field::required(
        "kind",
        Type::Enum(&["process", "standard"]),
        "process or standard",
    ),
    Field::required("title", Type::String, "Document title"),
    Field::optional("key", Type::String, "Optional lowercase key"),
    Field::optional("project", Type::String, "Project ID, P-<n>, or slug"),
    Field::optional("team", Type::String, "Team UUIDv7 (people only)"),
    Field::optional(
        "status",
        Type::Enum(&["draft", "current", "deprecated"]),
        "Status",
    ),
    Field::optional("owner", Type::String, "Accountable owner UUIDv7"),
    Field::optional(
        "non_overridable",
        Type::Boolean,
        "Make it must-follow: no lower level may replace it (default: overridable)",
    ),
    Field::required("body", Type::String, "Markdown body"),
];

const PROCESS_WRITE_OUTPUT: &[Field] = &[
    Field::required("document_id", Type::String, "Document UUIDv7"),
    Field::required("revision", Type::Integer, "New document revision"),
    Field::required(
        "document",
        Type::Object(PROCESS_DOC_FIELDS),
        "Saved document",
    ),
];

const SKILL_SUMMARY_FIELDS: &[Field] = &[
    Field::required("key", Type::String, "Skill key"),
    Field::required("title", Type::String, "Skill title"),
    Field::required("description", Type::String, "One-line description"),
    Field::required("source", Type::String, "Origin (built_in)"),
];

const SKILL_READ_INPUT: &[Field] = &[Field::required(
    "name",
    Type::String,
    "Skill key from `skill list`",
)];

const SKILL_READ_OUTPUT: &[Field] = &[
    Field::required("key", Type::String, "Skill key"),
    Field::required("title", Type::String, "Skill title"),
    Field::required("description", Type::String, "One-line description"),
    Field::required("source", Type::String, "Origin (built_in)"),
    Field::required("body_markdown", Type::String, "Full Markdown guidance"),
];

/// C5 processes, standards, and built-in skills commands (`doc-28` §3.5).
pub const PROCESS_COMMANDS: &[Command] = &[
    Command {
        resource: "process",
        verb: "list",
        summary: "List effective processes/standards",
        call: "process.effective",
        kind: Kind::List,
        input: PROCESS_LIST_INPUT,
        content_field: None,
        output: PROCESS_LIST_ROW,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"kind":"standard"}"#,
            output: r#"[{"id":"sec-baseline","key":"sec-baseline","kind":"standard","applies":true,"mandatory":true,"document":{"id":"0193a5c0-0000-7000-8000-000000000090","key":"sec-baseline","revision":1,"kind":"standard","title":"Security baseline","status":"current","level":"organization","overridable":false,"summary":"Never leak secrets."}}]"#,
        },
    },
    Command {
        resource: "process",
        verb: "inspect",
        summary: "Read process/standard or --why chain",
        call: "process.show",
        kind: Kind::Read,
        input: PROCESS_INSPECT_INPUT,
        content_field: None,
        output: PROCESS_INSPECT_OUTPUT,
        errors: PROCESS_INSPECT_ERRORS,
        example: Example {
            input: r#"{"key":"sec-baseline"}"#,
            output: r#"{"document":{"id":"0193a5c0-0000-7000-8000-000000000090","key":"sec-baseline","revision":1,"kind":"standard","title":"Security baseline","status":"current","level":"organization","overridable":false,"summary":"Never leak secrets."},"content":"Never leak secrets.","total_bytes":19}"#,
        },
    },
    Command {
        resource: "process",
        verb: "create",
        summary: "Create process/standard",
        call: "process.write",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: PROCESS_CREATE_INPUT,
        content_field: Some("body"),
        output: PROCESS_WRITE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"kind":"standard","title":"API errors","key":"api-errors","project":"P-1","body":"Use canonical codes."}"#,
            output: r#"{"document_id":"0193a5c0-0000-7000-8000-000000000091","revision":1,"document":{"id":"0193a5c0-0000-7000-8000-000000000091","key":"api-errors","revision":1,"kind":"standard","title":"API errors","status":"current","level":"project","overridable":true,"summary":"Use canonical codes."}}"#,
        },
    },
    Command {
        resource: "process",
        verb: "update",
        summary: "Update process/standard",
        call: "process.write",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: PROCESS_UPDATE_INPUT,
        content_field: Some("body"),
        output: PROCESS_WRITE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000091","kind":"standard","title":"API errors","key":"api-errors","project":"P-1","body":"Updated rules."}"#,
            output: r#"{"document_id":"0193a5c0-0000-7000-8000-000000000091","revision":2,"document":{"id":"0193a5c0-0000-7000-8000-000000000091","key":"api-errors","revision":2,"kind":"standard","title":"API errors","status":"current","level":"project","overridable":true,"summary":"Updated rules."}}"#,
        },
    },
    Command {
        resource: "skill",
        verb: "list",
        summary: "List built-in skills",
        call: "skill.list",
        kind: Kind::List,
        input: &[],
        content_field: None,
        output: SKILL_SUMMARY_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"key":"agent-os-basics","title":"Using agent-os","description":"Grammar, envelopes, and session attach.","source":"built_in"}]"#,
        },
    },
    Command {
        resource: "skill",
        verb: "read",
        summary: "Read built-in skill",
        call: "skill.read",
        kind: Kind::Read,
        input: SKILL_READ_INPUT,
        content_field: None,
        output: SKILL_READ_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"name":"agent-os-basics"}"#,
            output: r##"{"key":"agent-os-basics","title":"Using agent-os","description":"Grammar, envelopes, and session attach.","source":"built_in","body_markdown":"# Using agent-os"}"##,
        },
    },
];
