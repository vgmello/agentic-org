//! Declarative command rows for `suggestion create|list|inspect|withdraw` and `ui present`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

const TESS_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const TESS_WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Conflict,
    ErrorCode::PolicyViolation,
    ErrorCode::Unavailable,
];

const ENTITY_KIND_ENUM: &[&str] = &[
    "task",
    "project",
    "team",
    "member",
    "role",
    "process",
    "channel",
    "office",
    "budget",
    "hire_request",
];

const UI_ENTITY_LINK_INPUT_FIELDS: &[Field] = &[
    Field::required("kind", Type::Enum(ENTITY_KIND_ENUM), "Entity kind"),
    Field::required("id", Type::String, "Entity UUIDv7, key, handle, or name"),
    Field::optional("label", Type::String, "Optional display label override"),
];
const UI_ENTITY_LINK_INPUT_TYPE: Type = Type::Object(UI_ENTITY_LINK_INPUT_FIELDS);

const UI_ENTITY_LINK_FIELDS: &[Field] = &[
    Field::required("kind", Type::String, "Verified entity kind"),
    Field::required("id", Type::String, "Canonical entity UUIDv7"),
    Field::optional("key", Type::String, "Human-readable key or handle"),
    Field::required("label", Type::String, "Verified display label"),
    Field::required("route", Type::String, "Verified in-app route"),
];
const UI_ENTITY_LINK_TYPE: Type = Type::Object(UI_ENTITY_LINK_FIELDS);

const UI_METRIC_FIELDS: &[Field] = &[
    Field::required("label", Type::String, "Metric label"),
    Field::required("value", Type::String, "Formatted metric value"),
    Field::optional("tone", Type::String, "Optional visual tone"),
    Field::optional("unit", Type::String, "Optional metric unit"),
];
const UI_METRIC_TYPE: Type = Type::Object(UI_METRIC_FIELDS);

const UI_NAVIGATION_FIELDS: &[Field] = &[
    Field::required("label", Type::String, "Navigation action label"),
    Field::required("route", Type::String, "Safe relative in-app route"),
    Field::optional(
        "entity_kind",
        Type::Enum(ENTITY_KIND_ENUM),
        "Optional entity kind to verify",
    ),
    Field::optional(
        "entity_id",
        Type::String,
        "Optional entity ID/key to verify",
    ),
];
const UI_NAVIGATION_TYPE: Type = Type::Object(UI_NAVIGATION_FIELDS);

const UI_FOLLOW_UP_FIELDS: &[Field] = &[
    Field::required("label", Type::String, "Short follow-up chip label"),
    Field::required("prompt", Type::String, "Follow-up prompt text"),
];
const UI_FOLLOW_UP_TYPE: Type = Type::Object(UI_FOLLOW_UP_FIELDS);

const UI_BLOCK_FIELDS: &[Field] = &[
    Field::optional("title", Type::String, "Optional block heading"),
    Field::optional(
        "entity_links",
        Type::List(&UI_ENTITY_LINK_TYPE),
        "Server-verified entity links",
    ),
    Field::optional(
        "metrics",
        Type::List(&UI_METRIC_TYPE),
        "Compact metric cards",
    ),
    Field::optional(
        "navigation",
        Type::List(&UI_NAVIGATION_TYPE),
        "Validated in-app navigation links",
    ),
    Field::optional(
        "follow_ups",
        Type::List(&UI_FOLLOW_UP_TYPE),
        "Suggested follow-up prompts (max 3)",
    ),
    Field::optional(
        "suggestion_id",
        Type::String,
        "Attached pending suggestion ID, when present",
    ),
];

const UI_PRESENT_INPUT: &[Field] = &[
    Field::optional("title", Type::String, "Optional block title"),
    Field::optional(
        "entity_links",
        Type::List(&UI_ENTITY_LINK_INPUT_TYPE),
        "Entities to verify and link (structured JSON)",
    ),
    Field::optional(
        "metrics",
        Type::List(&UI_METRIC_TYPE),
        "Metric items (structured JSON)",
    ),
    Field::optional(
        "navigation",
        Type::List(&UI_NAVIGATION_TYPE),
        "In-app navigation actions (structured JSON)",
    ),
    Field::optional(
        "follow_ups",
        Type::List(&UI_FOLLOW_UP_TYPE),
        "Up to 3 follow-up prompts (structured JSON)",
    ),
];

const UI_PRESENT_OUTPUT: &[Field] = &[Field::required(
    "ui_block",
    Type::Object(UI_BLOCK_FIELDS),
    "Server-verified structured UI block",
)];

/// The `ui` rows.
pub const UI_COMMANDS: &[Command] = &[Command {
    resource: "ui",
    verb: "present",
    summary: "Validate and emit a structured UI block",
    call: "ui.present",
    kind: Kind::Read,
    input: UI_PRESENT_INPUT,
    content_field: None,
    output: UI_PRESENT_OUTPUT,
    errors: TESS_ERRORS,
    example: Example {
        input: r#"{"title":"Brief summary","metrics":[{"label":"Open tasks","value":"4"}],"follow_ups":[{"label":"Check blockers","prompt":"Show blocked tasks"}]}"#,
        output: r#"{"ui_block":{"title":"Brief summary","metrics":[{"label":"Open tasks","value":"4"}],"follow_ups":[{"label":"Check blockers","prompt":"Show blocked tasks"}]}}"#,
    },
}];

const STATUS_ENUM: &[&str] = &["pending", "withdrawn", "dismissed", "expired"];

const SUGGESTION_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Suggestion UUIDv7"),
    Field::required(
        "status",
        Type::Enum(STATUS_ENUM),
        "pending, withdrawn, dismissed, or expired",
    ),
    Field::required("command", Type::String, "Canonical target agent-os command"),
    Field::required("call", Type::String, "Target office RPC method"),
    Field::required("input", Type::Json, "Validated target command input object"),
    Field::required("summary", Type::String, "Human-facing confirmation summary"),
    Field::optional("rationale", Type::String, "Optional proposal rationale"),
    Field::optional(
        "expected_revision",
        Type::Integer,
        "Pinned expected target entity revision",
    ),
    Field::required(
        "created_by_member_id",
        Type::String,
        "Chief of Staff member UUIDv7",
    ),
    Field::required("for_caller_id", Type::String, "Target human caller UUIDv7"),
    Field::required(
        "preflight_ok",
        Type::Boolean,
        "Server preflight validation result",
    ),
    Field::optional("preflight_note", Type::String, "Server preflight note"),
    Field::required("created_at", Type::String, "RFC 3339 creation timestamp"),
    Field::required("expires_at", Type::String, "RFC 3339 expiry timestamp"),
    Field::optional("resolved_at", Type::String, "RFC 3339 resolution timestamp"),
    Field::optional(
        "close_reason",
        Type::String,
        "Withdrawal or dismissal reason",
    ),
];

const SUGGESTION_CREATE_INPUT: &[Field] = &[
    Field::required(
        "command",
        Type::String,
        "Target agent-os command (for example \"task create\" or \"team update\")",
    ),
    Field::required("summary", Type::String, "Concise human-facing summary"),
    Field::optional(
        "input",
        Type::Json,
        "JSON object matching the target command's input schema",
    ),
    Field::optional("rationale", Type::String, "Why this change is proposed"),
    Field::optional(
        "expected_revision",
        Type::Integer,
        "Expected target entity revision to pin before confirmation",
    ),
    Field::optional(
        "ttl_seconds",
        Type::Integer,
        "Proposal expiry in seconds (30 to 86400; default 900)",
    ),
];

const SUGGESTION_CREATE_OUTPUT: &[Field] = &[
    Field::required(
        "suggestion",
        Type::Object(SUGGESTION_FIELDS),
        "Validated proposal record",
    ),
    Field::required(
        "ui_block",
        Type::Object(UI_BLOCK_FIELDS),
        "Verified UI confirmation card block",
    ),
    Field::optional(
        "dry_run",
        Type::Boolean,
        "True when preflighted without storing",
    ),
];

const SUGGESTION_LIST_INPUT: &[Field] = &[Field::optional(
    "status",
    Type::Enum(STATUS_ENUM),
    "Filter by suggestion status",
)];

const SUGGESTION_INSPECT_INPUT: &[Field] = &[Field::required(
    "suggestion",
    Type::String,
    "Suggestion UUIDv7",
)];

const SUGGESTION_INSPECT_OUTPUT: &[Field] = &[
    Field::required(
        "suggestion",
        Type::Object(SUGGESTION_FIELDS),
        "Stored proposal record",
    ),
    Field::required(
        "ui_block",
        Type::Object(UI_BLOCK_FIELDS),
        "Verified UI presentation block",
    ),
];

const SUGGESTION_WITHDRAW_INPUT: &[Field] = &[
    Field::required("suggestion", Type::String, "Pending suggestion UUIDv7"),
    Field::optional(
        "reason",
        Type::String,
        "Reason for withdrawing the proposal",
    ),
];

const SUGGESTION_WITHDRAW_OUTPUT: &[Field] = &[Field::required(
    "suggestion",
    Type::Object(SUGGESTION_FIELDS),
    "Updated proposal record",
)];

/// The `suggestion` rows.
pub const SUGGESTION_COMMANDS: &[Command] = &[
    Command {
        resource: "suggestion",
        verb: "create",
        summary: "Preflight and propose a mutation for caller confirmation",
        call: "suggestion.create",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: SUGGESTION_CREATE_INPUT,
        content_field: None,
        output: SUGGESTION_CREATE_OUTPUT,
        errors: TESS_WRITE_ERRORS,
        example: Example {
            input: r#"{"command":"task create","input":{"title":"Review release"},"summary":"Create release review task"}"#,
            output: r#"{"suggestion":{"id":"0193a5c0-0000-7000-8000-000000000901","status":"pending","command":"task create","call":"tasks.create","input":{"title":"Review release"},"summary":"Create release review task","created_by_member_id":"0193a5c0-0000-7000-8000-000000000010","for_caller_id":"0193a5c0-0000-7000-8000-000000000002","preflight_ok":true,"created_at":"2026-11-27T00:00:00.000Z","expires_at":"2026-11-27T00:15:00.000Z"},"ui_block":{"title":"Create release review task","suggestion_id":"0193a5c0-0000-7000-8000-000000000901"}}"#,
        },
    },
    Command {
        resource: "suggestion",
        verb: "list",
        summary: "List caller-scoped mutation proposals",
        call: "suggestion.list",
        kind: Kind::List,
        input: SUGGESTION_LIST_INPUT,
        content_field: None,
        output: SUGGESTION_FIELDS,
        errors: TESS_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000901","status":"pending","command":"task create","call":"tasks.create","input":{"title":"Review release"},"summary":"Create release review task","created_by_member_id":"0193a5c0-0000-7000-8000-000000000010","for_caller_id":"0193a5c0-0000-7000-8000-000000000002","preflight_ok":true,"created_at":"2026-11-27T00:00:00.000Z","expires_at":"2026-11-27T00:15:00.000Z"}]"#,
        },
    },
    Command {
        resource: "suggestion",
        verb: "inspect",
        summary: "Inspect one caller-scoped mutation proposal",
        call: "suggestion.inspect",
        kind: Kind::Read,
        input: SUGGESTION_INSPECT_INPUT,
        content_field: None,
        output: SUGGESTION_INSPECT_OUTPUT,
        errors: TESS_ERRORS,
        example: Example {
            input: r#"{"suggestion":"0193a5c0-0000-7000-8000-000000000901"}"#,
            output: r#"{"suggestion":{"id":"0193a5c0-0000-7000-8000-000000000901","status":"pending","command":"task create","call":"tasks.create","input":{"title":"Review release"},"summary":"Create release review task","created_by_member_id":"0193a5c0-0000-7000-8000-000000000010","for_caller_id":"0193a5c0-0000-7000-8000-000000000002","preflight_ok":true,"created_at":"2026-11-27T00:00:00.000Z","expires_at":"2026-11-27T00:15:00.000Z"},"ui_block":{"title":"Create release review task","suggestion_id":"0193a5c0-0000-7000-8000-000000000901"}}"#,
        },
    },
    Command {
        resource: "suggestion",
        verb: "withdraw",
        summary: "Withdraw a pending mutation proposal",
        call: "suggestion.withdraw",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: SUGGESTION_WITHDRAW_INPUT,
        content_field: None,
        output: SUGGESTION_WITHDRAW_OUTPUT,
        errors: TESS_WRITE_ERRORS,
        example: Example {
            input: r#"{"suggestion":"0193a5c0-0000-7000-8000-000000000901","reason":"Superseded"}"#,
            output: r#"{"suggestion":{"id":"0193a5c0-0000-7000-8000-000000000901","status":"withdrawn","command":"task create","call":"tasks.create","input":{"title":"Review release"},"summary":"Create release review task","created_by_member_id":"0193a5c0-0000-7000-8000-000000000010","for_caller_id":"0193a5c0-0000-7000-8000-000000000002","preflight_ok":true,"created_at":"2026-11-27T00:00:00.000Z","expires_at":"2026-11-27T00:15:00.000Z","resolved_at":"2026-11-27T00:01:00.000Z","close_reason":"Superseded"}}"#,
        },
    },
];

/// All command row slices in the Chief of Staff (Tess) domain.
pub const ALL: &[&[Command]] = &[SUGGESTION_COMMANDS, UI_COMMANDS];
