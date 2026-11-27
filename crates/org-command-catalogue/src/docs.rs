//! Declarative command rows for `doc index|ls|show|new|edit|move|put|delete` (`TASK-55`).

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

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

const DOC_STATUSES: &[&str] = &["draft", "current", "deprecated"];
const MEDIA_KINDS: &[&str] = &["image", "diagram"];

const DOC_SUMMARY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Document UUIDv7"),
    Field::required("project_id", Type::String, "Project UUIDv7"),
    Field::required("path", Type::String, "Path relative to docs/"),
    Field::required(
        "section",
        Type::String,
        "Folder below docs/ (empty at root)",
    ),
    Field::required("kind", Type::String, "Document kind slug"),
    Field::required("title", Type::String, "Document title"),
    Field::required("status", Type::Enum(DOC_STATUSES), "Lifecycle status"),
    Field::optional("summary", Type::String, "Optional short summary"),
    Field::optional("owner_employee_id", Type::String, "Owner employee UUIDv7"),
    Field::required("revision", Type::Integer, "Current revision"),
    Field::optional("created_at", Type::String, "RFC 3339 creation time"),
    Field::optional("updated_at", Type::String, "RFC 3339 update time"),
    Field::optional("updated_by_id", Type::String, "Last updater UUIDv7"),
];

const DOC_SECTION_FIELDS: &[Field] = &[
    Field::required("path", Type::String, "Section path below docs/"),
    Field::required("document_count", Type::Integer, "Documents in this section"),
];

const DOC_SECTION_TYPE: Type = Type::Object(DOC_SECTION_FIELDS);
const DOC_SUMMARY_TYPE: Type = Type::Object(DOC_SUMMARY_FIELDS);
const DOC_MEDIA_TYPE: Type = Type::Object(DOC_MEDIA_FIELDS);

const DOC_INDEX_OUTPUT: &[Field] = &[
    Field::required("project_id", Type::String, "Project UUIDv7"),
    Field::required("index_path", Type::String, "Durable docs/index.yaml path"),
    Field::required(
        "sections",
        Type::List(&DOC_SECTION_TYPE),
        "Sections with doc counts",
    ),
    Field::required(
        "documents",
        Type::List(&DOC_SUMMARY_TYPE),
        "Documents in path order",
    ),
    Field::required(
        "media",
        Type::List(&DOC_MEDIA_TYPE),
        "Media assets in path order",
    ),
];

const DOC_INSPECT_OUTPUT: &[Field] = &[
    Field::required(
        "document",
        DOC_SUMMARY_TYPE,
        "Document summary and metadata",
    ),
    Field::required("body_markdown", Type::String, "Full Markdown body"),
];

const DOC_DELETE_OUTPUT: &[Field] = &[
    Field::required("deleted_id", Type::String, "Removed document UUIDv7"),
    Field::required(
        "deleted_path",
        Type::String,
        "Removed path relative to docs/",
    ),
    Field::required("index_path", Type::String, "Regenerated index.yaml path"),
];

const DOC_MEDIA_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Media UUIDv7"),
    Field::required("project_id", Type::String, "Project UUIDv7"),
    Field::required(
        "path",
        Type::String,
        "Path relative to docs/ (media/<name>)",
    ),
    Field::required(
        "type",
        Type::Enum(MEDIA_KINDS),
        "Media kind: image or diagram",
    ),
    Field::required("media_type", Type::String, "MIME type"),
    Field::required("bytes", Type::Integer, "Asset byte size"),
    Field::required("sha256", Type::String, "Lowercase SHA-256 hex digest"),
    Field::required("title", Type::String, "Asset title"),
    Field::required("summary", Type::String, "Asset summary"),
    Field::required("revision", Type::Integer, "Current revision"),
    Field::optional("created_at", Type::String, "RFC 3339 creation time"),
    Field::optional("updated_at", Type::String, "RFC 3339 update time"),
];

const DOC_INDEX_INPUT: &[Field] = &[Field::required(
    "project",
    Type::String,
    "Project UUIDv7, P-<n>, or slug",
)];

const DOC_LS_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::optional("section", Type::String, "Only documents under this section"),
    Field::optional("kind", Type::String, "Filter by document kind"),
    Field::optional("status", Type::Enum(DOC_STATUSES), "Filter by status"),
];

const DOC_SHOW_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::required(
        "document",
        Type::String,
        "Document UUIDv7 or path relative to docs/",
    ),
];

const DOC_NEW_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::required(
        "kind",
        Type::String,
        "Lowercase kind slug (outside brief/changelog)",
    ),
    Field::required("title", Type::String, "Document title"),
    Field::optional(
        "path",
        Type::String,
        "Explicit path relative to docs/ (.md)",
    ),
    Field::optional(
        "section",
        Type::String,
        "Section folder (when --path omitted)",
    ),
    Field::optional("summary", Type::String, "Optional summary (<= 2 KiB)"),
    Field::optional("owner", Type::String, "Owner employee UUIDv7"),
    Field::optional(
        "status",
        Type::Enum(DOC_STATUSES),
        "Status (default: draft)",
    ),
    Field::optional("body", Type::String, "Markdown body"),
];

const DOC_EDIT_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::required(
        "document",
        Type::String,
        "Document UUIDv7 or path relative to docs/",
    ),
    Field::optional("title", Type::String, "Updated title"),
    Field::optional("kind", Type::String, "Updated kind slug"),
    Field::optional("status", Type::Enum(DOC_STATUSES), "Updated status"),
    Field::optional("summary", Type::String, "Updated summary (empty clears)"),
    Field::optional(
        "owner",
        Type::String,
        "Updated owner UUIDv7 (empty clears if not current)",
    ),
    Field::optional("body", Type::String, "Replacement Markdown body"),
];

const DOC_MOVE_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::required(
        "document",
        Type::String,
        "Document UUIDv7 or path relative to docs/",
    ),
    Field::optional(
        "new_path",
        Type::String,
        "Destination path relative to docs/",
    ),
    Field::optional(
        "new_section",
        Type::String,
        "Destination section keeping file name",
    ),
];

const DOC_DELETE_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::required(
        "document",
        Type::String,
        "Document UUIDv7 or path relative to docs/",
    ),
];

const DOC_PUT_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project UUIDv7, P-<n>, or slug"),
    Field::required(
        "path",
        Type::String,
        "Local image path inside an allowed root",
    ),
    Field::required("title", Type::String, "Media asset title"),
    Field::optional(
        "file_name",
        Type::String,
        "Target lowercase file name under docs/media/",
    ),
    Field::optional(
        "kind",
        Type::Enum(MEDIA_KINDS),
        "image or diagram (default: image)",
    ),
    Field::optional("summary", Type::String, "Optional summary"),
];

const DOC_SUMMARY_EXAMPLE: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000600","project_id":"0193a5c0-0000-7000-8000-000000000010","path":"guides/setup.md","section":"guides","kind":"guide","title":"Setup","status":"draft","revision":1}"#;

const DOC_MEDIA_EXAMPLE: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000601","project_id":"0193a5c0-0000-7000-8000-000000000010","path":"media/arch.png","type":"diagram","media_type":"image/png","bytes":128,"sha256":"0000000000000000000000000000000000000000000000000000000000000000","title":"Architecture","summary":"","revision":1}"#;

/// The `doc` command rows.
pub const COMMANDS: &[Command] = &[
    Command {
        resource: "doc",
        verb: "index",
        summary: "Read a project's full docs/index.yaml tree",
        call: "docs.index",
        kind: Kind::Read,
        input: DOC_INDEX_INPUT,
        content_field: None,
        output: DOC_INDEX_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"project":"core"}"#,
            output: r#"{"project_id":"0193a5c0-0000-7000-8000-000000000010","index_path":"state/projects/0193a5c0-0000-7000-8000-000000000010/docs/index.yaml","sections":[],"documents":[],"media":[]}"#,
        },
    },
    Command {
        resource: "doc",
        verb: "ls",
        summary: "List documents in a project tree",
        call: "docs.list",
        kind: Kind::List,
        input: DOC_LS_INPUT,
        content_field: None,
        output: DOC_SUMMARY_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"project":"core","section":"guides"}"#,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000600","project_id":"0193a5c0-0000-7000-8000-000000000010","path":"guides/setup.md","section":"guides","kind":"guide","title":"Setup","status":"draft","revision":1}]"#,
        },
    },
    Command {
        resource: "doc",
        verb: "show",
        summary: "Show one project document and its Markdown body",
        call: "docs.inspect",
        kind: Kind::Read,
        input: DOC_SHOW_INPUT,
        content_field: None,
        output: DOC_INSPECT_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"project":"core","document":"guides/setup.md"}"#,
            output: r##"{"document":{"id":"0193a5c0-0000-7000-8000-000000000600","project_id":"0193a5c0-0000-7000-8000-000000000010","path":"guides/setup.md","section":"guides","kind":"guide","title":"Setup","status":"draft","revision":1},"body_markdown":"# Setup\n"}"##,
        },
    },
    Command {
        resource: "doc",
        verb: "new",
        summary: "Create a new document in a project's docs/ tree",
        call: "docs.create",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: DOC_NEW_INPUT,
        content_field: Some("body"),
        output: DOC_SUMMARY_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r##"{"project":"core","kind":"guide","title":"Setup","section":"guides","body":"# Setup\n"}"##,
            output: DOC_SUMMARY_EXAMPLE,
        },
    },
    Command {
        resource: "doc",
        verb: "edit",
        summary: "Edit a project document at --if-match revision",
        call: "docs.update",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: DOC_EDIT_INPUT,
        content_field: Some("body"),
        output: DOC_SUMMARY_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"project":"core","document":"guides/setup.md","title":"Setup Guide"}"#,
            output: DOC_SUMMARY_EXAMPLE,
        },
    },
    Command {
        resource: "doc",
        verb: "move",
        summary: "Move a project document to another path or section",
        call: "docs.move",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: DOC_MOVE_INPUT,
        content_field: None,
        output: DOC_SUMMARY_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"project":"core","document":"guides/setup.md","new_section":"runbooks"}"#,
            output: DOC_SUMMARY_EXAMPLE,
        },
    },
    Command {
        resource: "doc",
        verb: "delete",
        summary: "Delete a project document and regenerate index.yaml",
        call: "docs.delete",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: DOC_DELETE_INPUT,
        content_field: None,
        output: DOC_DELETE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"project":"core","document":"guides/setup.md"}"#,
            output: r#"{"deleted_id":"0193a5c0-0000-7000-8000-000000000600","deleted_path":"guides/setup.md","index_path":"state/projects/0193a5c0-0000-7000-8000-000000000010/docs/index.yaml"}"#,
        },
    },
    Command {
        resource: "doc",
        verb: "put",
        summary: "Upload or replace an image asset under docs/media/",
        call: "docs.put_media",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Optional,
        },
        input: DOC_PUT_INPUT,
        content_field: None,
        output: DOC_MEDIA_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"project":"core","path":"arch.png","title":"Architecture","kind":"diagram"}"#,
            output: DOC_MEDIA_EXAMPLE,
        },
    },
];

/// All command row slices in the project docs domain.
pub const ALL: &[&[Command]] = &[COMMANDS];
