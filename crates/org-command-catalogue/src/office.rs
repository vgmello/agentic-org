//! Declarative command rows for `office list|sync-status` and `inbox list`.

use org_command::{Command, Example, Field, Kind, Type};
use org_proto::ErrorCode;

use crate::shared::{JSON_TYPE, STRING_TYPE};

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const INBOX_KIND_TYPE: Type =
    Type::Enum(&["approval", "question", "decision", "problem", "review"]);

const OFFICE_SUMMARY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Office UUIDv7"),
    Field::required("organization_id", Type::String, "Organization UUIDv7"),
    Field::required("name", Type::String, "Office display name"),
    Field::required("slug", Type::String, "Office slug"),
    Field::required("role", Type::String, "hq or satellite"),
];

const OFFICE_SYNC_OUTPUT: &[Field] = &[
    Field::required("org_id", Type::String, "Organization UUIDv7"),
    Field::required("local_office", Type::String, "Local office UUIDv7"),
    Field::required("hq_office", Type::String, "HQ office UUIDv7"),
    Field::required("epoch", Type::Integer, "Current replication epoch"),
    Field::required("seq", Type::Integer, "Current organization log sequence"),
    Field::required(
        "highest_epoch",
        Type::Integer,
        "Highest observed replication epoch",
    ),
    Field::required("protocol_level", Type::Integer, "Sync protocol level"),
    Field::required(
        "restore_pending",
        Type::Boolean,
        "Whether snapshot restore is pending",
    ),
    Field::required(
        "initial_snapshot_required",
        Type::Boolean,
        "Whether initial HQ snapshot is still required",
    ),
    Field::required(
        "pending_work_count",
        Type::Integer,
        "Queued outbound work mutations",
    ),
    Field::required(
        "pending_message_count",
        Type::Integer,
        "Queued outbound messages",
    ),
];

const INBOX_LIST_INPUT: &[Field] = &[
    Field::optional(
        "state",
        Type::Enum(&["open", "handled"]),
        "Inbox state to list (default open)",
    ),
    Field::optional(
        "kind",
        Type::List(&INBOX_KIND_TYPE),
        "Filter by item kind (repeatable: approval, question, decision, problem)",
    ),
    Field::optional("project", Type::String, "Filter by project ID"),
    Field::optional("team", Type::String, "Filter by team ID"),
    Field::optional(
        "has_image",
        Type::Boolean,
        "Only items with image attachments",
    ),
    Field::optional(
        "has_file",
        Type::Boolean,
        "Only items with file attachments",
    ),
];

const INBOX_ITEM_FIELDS: &[Field] = &[
    Field::required("ref", Type::String, "Stable inbox item reference"),
    Field::required("source", Type::String, "Originating subsystem"),
    Field::required(
        "kind",
        Type::String,
        "approval, question, decision, or problem",
    ),
    Field::optional("member", Type::String, "Related member UUIDv7"),
    Field::optional("project", Type::String, "Related project UUIDv7"),
    Field::optional("team", Type::String, "Related team UUIDv7"),
    Field::required("summary", Type::String, "Concise item summary"),
    Field::optional("context", Type::String, "Supporting context"),
    Field::optional("recommendation", Type::String, "Recommended action"),
    Field::optional(
        "options",
        Type::List(&STRING_TYPE),
        "Structured answer choices, when applicable",
    ),
    Field::optional(
        "created_at_ms",
        Type::Integer,
        "Creation epoch milliseconds",
    ),
    Field::optional("expires_at_ms", Type::Integer, "Expiry epoch milliseconds"),
    Field::required("state", Type::String, "open or handled"),
    Field::optional("handled_at_ms", Type::Integer, "Handled epoch milliseconds"),
    Field::optional("outcome", Type::String, "Recorded resolution outcome"),
    Field::required("clears", Type::String, "Clearing condition"),
    Field::required("dismissable", Type::Boolean, "Whether dismissable"),
    Field::required("actions", Type::List(&JSON_TYPE), "Available item actions"),
    Field::optional(
        "attachments",
        Type::List(&JSON_TYPE),
        "Attached file metadata",
    ),
];

/// The `office` and `inbox` rows.
pub const COMMANDS: &[Command] = &[
    Command {
        resource: "office",
        verb: "list",
        summary: "List registered offices in the organization",
        call: "office.directory",
        kind: Kind::List,
        input: &[],
        content_field: None,
        output: OFFICE_SUMMARY_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000001","organization_id":"0193a5c0-0000-7000-8000-000000000000","name":"HQ","slug":"hq","role":"hq"}]"#,
        },
    },
    Command {
        resource: "office",
        verb: "sync-status",
        summary: "Inspect local office replication and outbox status",
        call: "office.sync_status",
        kind: Kind::Read,
        input: &[],
        content_field: None,
        output: OFFICE_SYNC_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{"org_id":"0193a5c0-0000-7000-8000-000000000000","local_office":"0193a5c0-0000-7000-8000-000000000001","hq_office":"0193a5c0-0000-7000-8000-000000000001","epoch":1,"seq":10,"highest_epoch":1,"protocol_level":1,"restore_pending":false,"initial_snapshot_required":false,"pending_work_count":0,"pending_message_count":0}"#,
        },
    },
    Command {
        resource: "inbox",
        verb: "list",
        summary: "List owner inbox items (owner-only)",
        call: "inbox.list",
        kind: Kind::List,
        input: INBOX_LIST_INPUT,
        content_field: None,
        output: INBOX_ITEM_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"ref":"approval:1","source":"session","kind":"approval","summary":"Approve tool call","state":"open","clears":"answered","dismissable":false,"actions":[]}]"#,
        },
    },
];

/// All command row slices in the office & inbox domain.
pub const ALL: &[&[Command]] = &[COMMANDS];
