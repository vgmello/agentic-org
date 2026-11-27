//! Declarative command rows for `session context|inspect|list` and `grant list`, plus the
//! `context` shorthand alias.

use org_command::{Alias, Command, Example, Field, Kind, Type};
use org_proto::ErrorCode;

use crate::shared::STRING_TYPE;

/// `context` is `session context`.
pub const CONTEXT_ALIAS: Alias = Alias {
    words: &["context"],
    command: &["session", "context"],
};

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

const CONTEXT_INPUT: &[Field] = &[
    Field::optional(
        "project",
        Type::String,
        "Also the must-follow processes of this project (UUIDv7, P-<number> key or slug)",
    ),
    Field::optional(
        "offline",
        Type::Boolean,
        "Answer from this build alone, without the office: rules, catalogue and skills",
    ),
];

const IDENTITY_TEAM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("title_in_team", Type::String, "Title in the team"),
];

const IDENTITY_TEAM_TYPE: Type = Type::Object(IDENTITY_TEAM_FIELDS);

const IDENTITY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Your employee ID"),
    Field::required("name", Type::String, "Your display name"),
    Field::optional("handle", Type::String, "Your handle (@handle)"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::optional("role_title", Type::String, "Your role title"),
    Field::required(
        "teams",
        Type::List(&IDENTITY_TEAM_TYPE),
        "Your teams, home team first",
    ),
];

const SKILL_INDEX_FIELDS: &[Field] = &[
    Field::required("key", Type::String, "Pass to `skill read --name`"),
    Field::required("description", Type::String, "What the guide covers"),
];

const SKILL_INDEX_TYPE: Type = Type::Object(SKILL_INDEX_FIELDS);

const MANDATORY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Document ID"),
    Field::optional("key", Type::String, "Document key"),
    Field::required(
        "kind",
        Type::Enum(&["process", "standard"]),
        "Process or standard",
    ),
    Field::required(
        "level",
        Type::String,
        "Where it is set: organization, team or project",
    ),
    Field::optional(
        "scope_name",
        Type::String,
        "The team or project it is set for",
    ),
    Field::required("title", Type::String, "Title"),
    Field::required("revision", Type::Integer, "Document revision"),
    Field::required("body", Type::String, "The whole Markdown text, never cut"),
];

const MANDATORY_TYPE: Type = Type::Object(MANDATORY_FIELDS);

const CONTEXT_OUTPUT: &[Field] = &[
    Field::required(
        "mode",
        Type::Enum(&["offline", "managed", "external"]),
        "offline: this build alone; managed: office-started session; external: bound external workspace",
    ),
    Field::optional(
        "identity",
        Type::Object(IDENTITY_FIELDS),
        "Who you are (bound only)",
    ),
    Field::required(
        "rules",
        Type::String,
        "The standing rules for your session mode",
    ),
    Field::required(
        "catalogue",
        Type::String,
        "Every command, grouped by resource",
    ),
    Field::required(
        "skills",
        Type::List(&SKILL_INDEX_TYPE),
        "Built-in guides: read one with `skill read --name <key>`",
    ),
    Field::optional(
        "mandatory",
        Type::List(&MANDATORY_TYPE),
        "Must-follow (non-overridable) processes and standards that apply to you, whole (bound only)",
    ),
    Field::optional(
        "mandatory_truncated",
        Type::Boolean,
        "Whether must-follow documents were left out to stay within 8 KiB (bound only)",
    ),
    Field::optional(
        "mandatory_omitted",
        Type::List(&STRING_TYPE),
        "The key (or ID) of each must-follow document left out; read each with `process inspect`",
    ),
    Field::required("next_action", Type::String, "What to do next"),
];

/// The `session context` row slice.
pub const CONTEXT_COMMANDS: &[Command] = &[Command {
    resource: "session",
    verb: "context",
    summary: "Load rules, catalogue, skills and must-follow processes",
    call: "session.context",
    kind: Kind::Read,
    input: CONTEXT_INPUT,
    content_field: None,
    output: CONTEXT_OUTPUT,
    errors: READ_ERRORS,
    example: Example {
        input: r#"{"offline":true}"#,
        output: r###"{"mode":"offline","rules":"# How you work here","catalogue":"## Commands","skills":[{"key":"agent-os-basics","description":"Command grammar."}],"next_action":"Run `agent-os context` without --offline."}"###,
    },
}];

const SESSION_INSPECT_INPUT: &[Field] = &[Field::optional(
    "employee_id",
    Type::String,
    "Target agent employee ID (omit to inspect your own bound session)",
)];

const SESSION_USAGE_FIELDS: &[Field] = &[
    Field::required("input_tokens", Type::Integer, "Cumulative input tokens"),
    Field::required("output_tokens", Type::Integer, "Cumulative output tokens"),
    Field::required(
        "cache_read_tokens",
        Type::Integer,
        "Cumulative cache-read tokens",
    ),
    Field::required(
        "subagent_invocations",
        Type::Integer,
        "Sub-agent invocations in this session",
    ),
];

const SESSION_VIEW_FIELDS: &[Field] = &[
    Field::required("employee_id", Type::String, "Agent employee ID"),
    Field::required("office_id", Type::String, "Office ID"),
    Field::required("state", Type::String, "Current session lifecycle state"),
    Field::optional("run_id", Type::String, "Active or last run ID"),
    Field::optional("session_id", Type::String, "Logical session ID"),
    Field::optional("runtime_id", Type::String, "Runtime adapter ID"),
    Field::optional("runtime_session_id", Type::String, "Vendor session ID"),
    Field::required("private_home", Type::String, "Agent private home directory"),
    Field::required(
        "same_user_isolation_notice",
        Type::String,
        "Host isolation boundary notice",
    ),
    Field::required(
        "permission_preset",
        Type::String,
        "Effective permission preset",
    ),
    Field::required(
        "sandbox_enabled",
        Type::Boolean,
        "Whether sandbox is enabled",
    ),
    Field::required(
        "consecutive_stalls",
        Type::Integer,
        "Consecutive turn stalls",
    ),
    Field::required(
        "stall_locked",
        Type::Boolean,
        "Whether stall lock is active",
    ),
    Field::required(
        "recovery_required",
        Type::Boolean,
        "Whether manual recovery is required",
    ),
    Field::optional("last_reply", Type::String, "Last assistant reply"),
    Field::optional("last_error", Type::String, "Last error notice"),
    Field::required(
        "usage",
        Type::Object(SESSION_USAGE_FIELDS),
        "Cumulative session and sub-agent usage",
    ),
];

const SESSION_INSPECT_OUTPUT: &[Field] = &[
    Field::required(
        "session",
        Type::Object(SESSION_VIEW_FIELDS),
        "Session state view",
    ),
    Field::required(
        "accepted_commands_count",
        Type::Integer,
        "Accepted agent-os commands recorded in this run",
    ),
    Field::required("spec_hash", Type::String, "Canonical session spec hash"),
];

const GRANT_RULE_FIELDS: &[Field] = &[
    Field::required("effect", Type::Enum(&["allow", "deny"]), "Rule effect"),
    Field::required("tool", Type::String, "Tool name or wildcard"),
    Field::required("scope", Type::String, "Rule scope kind"),
    Field::optional("pattern", Type::String, "Optional scope pattern or prefix"),
];
const GRANT_RULE_TYPE: Type = Type::Object(GRANT_RULE_FIELDS);

const GRANT_LIST_INPUT: &[Field] = &[Field::optional(
    "employee_id",
    Type::String,
    "Target agent employee ID (omit for your own effective grants)",
)];

const GRANT_LIST_OUTPUT: &[Field] = &[
    Field::required("employee_id", Type::String, "Agent employee ID"),
    Field::required(
        "permission_preset",
        Type::String,
        "Active permission preset",
    ),
    Field::required("permission_mode", Type::String, "Runtime permission mode"),
    Field::required(
        "sandbox_enabled",
        Type::Boolean,
        "Whether sandbox is enabled",
    ),
    Field::required(
        "writable_roots",
        Type::List(&STRING_TYPE),
        "Writable filesystem roots",
    ),
    Field::required(
        "read_only_dirs",
        Type::List(&STRING_TYPE),
        "Read-only mounted directories",
    ),
    Field::required(
        "denied_write_patterns",
        Type::List(&STRING_TYPE),
        "Denied write path patterns",
    ),
    Field::required(
        "denied_read_patterns",
        Type::List(&STRING_TYPE),
        "Denied read path patterns",
    ),
    Field::optional(
        "allowed_tools",
        Type::List(&STRING_TYPE),
        "Optional explicit tool allowlist",
    ),
    Field::required(
        "rules",
        Type::List(&GRANT_RULE_TYPE),
        "Effective deny and allow rules",
    ),
];

const SESSION_SUMMARY_FIELDS: &[Field] = &[
    Field::required("session_id", Type::String, "Logical or runtime session ID"),
    Field::required("format", Type::String, "Transcript source format"),
    Field::required(
        "is_live",
        Type::Boolean,
        "Whether the session is currently live",
    ),
    Field::optional("started_at", Type::String, "RFC 3339 UTC start time"),
    Field::optional("updated_at", Type::String, "RFC 3339 UTC last update time"),
    Field::required("size_bytes", Type::Integer, "Transcript size in bytes"),
    Field::required("subagent_count", Type::Integer, "Sub-agent count"),
    Field::optional("preview", Type::String, "Short redacted preview"),
];

/// The `session inspect`, `session list`, and `grant list` rows.
pub const INSPECT_COMMANDS: &[Command] = &[
    Command {
        resource: "session",
        verb: "inspect",
        summary: "Inspect current or named agent session",
        call: "member.session_inspect",
        kind: Kind::Read,
        input: SESSION_INSPECT_INPUT,
        content_field: None,
        output: SESSION_INSPECT_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{"session":{"employee_id":"0193a5c0-0000-7000-8000-000000000003","office_id":"0193a5c0-0000-7000-8000-000000000001","state":"running","private_home":"/tmp/home","same_user_isolation_notice":"notice","permission_preset":"safe_ask","sandbox_enabled":true,"consecutive_stalls":0,"stall_locked":false,"recovery_required":false,"usage":{"input_tokens":0,"output_tokens":0,"cache_read_tokens":0,"subagent_invocations":0}},"accepted_commands_count":0,"spec_hash":"0123456789abcdef"}"#,
        },
    },
    Command {
        resource: "session",
        verb: "list",
        summary: "List recorded sessions for an agent",
        call: "member.session_list",
        kind: Kind::List,
        input: SESSION_INSPECT_INPUT,
        content_field: None,
        output: SESSION_SUMMARY_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"session_id":"sess-1","format":"claude","is_live":true,"size_bytes":1024,"subagent_count":0}]"#,
        },
    },
    Command {
        resource: "grant",
        verb: "list",
        summary: "List effective grants and sandbox rules",
        call: "member.session_grants",
        kind: Kind::Read,
        input: GRANT_LIST_INPUT,
        content_field: None,
        output: GRANT_LIST_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{"employee_id":"0193a5c0-0000-7000-8000-000000000003","permission_preset":"safe_ask","permission_mode":"ask","sandbox_enabled":true,"writable_roots":["/tmp/home"],"read_only_dirs":[],"denied_write_patterns":[],"denied_read_patterns":[],"rules":[]}"#,
        },
    },
];

/// All command row slices in the session domain.
pub const ALL: &[&[Command]] = &[CONTEXT_COMMANDS, INSPECT_COMMANDS];
