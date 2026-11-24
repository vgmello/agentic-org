//! `org-command` table rows for C3 session inspection (`CLI-9`), effective grants (`CLI-10`),
//! and directory lookup (`who` / `B2 Q9`).

use org_command::{Command, Example, Field, Kind, Type};
use org_proto::ErrorCode;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

const STRING_TYPE: Type = Type::String;

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

const DIRECTORY_LIST_INPUT: &[Field] = &[
    Field::optional("team", Type::String, "Optional team ID or name filter"),
    Field::optional(
        "member",
        Type::String,
        "Optional member ID or handle filter",
    ),
];

const DIRECTORY_CARD_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Employee ID"),
    Field::required("name", Type::String, "Display name"),
    Field::optional("handle", Type::String, "Addressing handle"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::required("owner", Type::Boolean, "Whether this member is the owner"),
    Field::optional(
        "agent_status",
        Type::Enum(&["active", "paused", "offboarded", "unknown"]),
        "Agent lifecycle status",
    ),
    Field::optional("role_title", Type::String, "Role title"),
    Field::required("presence", Type::String, "Current presence"),
];

pub static SESSION_COMMANDS: &[Command] = &[
    Command {
        resource: "session",
        verb: "inspect",
        summary: "Inspect the current or named agent session state",
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
        resource: "grant",
        verb: "list",
        summary: "List effective permission grants and sandbox boundaries for an agent",
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
    Command {
        resource: "directory",
        verb: "list",
        summary: "List members in the organization directory (`who`)",
        call: "who",
        kind: Kind::List,
        input: DIRECTORY_LIST_INPUT,
        content_field: None,
        output: DIRECTORY_CARD_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000003","name":"HR","handle":"hr","kind":"agent","owner":false,"agent_status":"active","role_title":"HR Lead","presence":"offline"}]"#,
        },
    },
];
