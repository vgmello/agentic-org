//! `session inspect` (`CLI-9`) and `grant list` (`CLI-10`): an agent session's state and usage,
//! and its effective permission presets, tool rules and sandbox boundaries.

use org_command::{Answer, Command, Example, Field, Kind, Refusal, Request, Type};
use org_proto::sessions::SessionListResult;
use serde_json::{Map, Value};

use super::{READ_ERRORS, STRING_TYPE};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{decode_err, encode, opt_str};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

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
pub(super) const COMMANDS: &[Command] = &[
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

/// `Use when:` guidance of each `session` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("session", "inspect", "checking your (or a named agent's) live session state, workspace paths, stall status, and token usage."),
    ("session", "list", "listing recorded and live conversation sessions for an agent."),
];

/// `Use when:` guidance of each `grant` command.
pub(super) const GRANT_CUES: &[CommandCue] = &[
    ("grant", "list", "verifying your (or a named agent's) effective permission preset, writable roots, and deny/allow rules."),
];

pub(super) fn read_session_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    if let Some(emp) = opt_str(request, "employee_id") {
        params.insert("employee_id".into(), Value::String(emp.to_owned()));
    }
    let raw = transport.call_method("member.session_inspect", Value::Object(params))?;
    Ok(Answer::completed(raw))
}

pub(super) fn read_session_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let mut params = Map::new();
    if let Some(emp) = opt_str(request, "employee_id") {
        params.insert("employee_id".into(), Value::String(emp.to_owned()));
    }
    let raw = transport.call_method("member.session_list", Value::Object(params))?;
    let res: SessionListResult =
        serde_json::from_value(raw).map_err(|_| decode_err("member.session_list"))?;
    let items = match encode(&res.sessions, "member.session_list")? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

pub(super) fn read_grant_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    if let Some(emp) = opt_str(request, "employee_id") {
        params.insert("employee_id".into(), Value::String(emp.to_owned()));
    }
    let raw = transport.call_method("member.session_grants", Value::Object(params))?;
    Ok(Answer::completed(raw))
}
