//! Operator rows `agent approve|sessions|revoke` on the shared `org-command` grammar and their
//! dispatch (`TASK-137`). They run as `agentic-org agent ...` over a signed-in (grant-bound)
//! channel; an agent's own `agent-os` connection is refused by the office.
//!
//! `agentic-org agent approve <code>`, `agent sessions @<handle>` and `agent revoke @<handle>
//! [<session_id>]` are accepted as shorthand for the named flags ([`rewrite_positional`]).

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Refusal, Request, Type};
use org_proto::external_auth::{
    MemberExternalApprove, MemberExternalRevoke, MemberExternalSessions,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};

use crate::transport::OfficeTransport;

const ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::Conflict,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const SESSION_VIEW: &[Field] = &[
    Field::required("session_id", Type::String, "External session (lease) ID"),
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::optional("handle", Type::String, "Agent handle"),
    Field::required("workspace_label", Type::String, "Signed-in workspace"),
    Field::required(
        "pubkey_fingerprint",
        Type::String,
        "Workspace key fingerprint",
    ),
    Field::required("approved_by", Type::String, "Approving member UUIDv7"),
    Field::required("approved_at", Type::String, "Approval timestamp"),
    Field::required("expires_at", Type::String, "Lease expiry"),
    Field::optional("revoked_at", Type::String, "Revocation timestamp"),
    Field::required("active", Type::Boolean, "Whether the lease is active"),
    Field::optional(
        "superseded_session_id",
        Type::String,
        "Lease this approval replaced",
    ),
    Field::optional(
        "took_over_managed",
        Type::Boolean,
        "Whether a live managed turn was parked",
    ),
];

const HISTORY_ENTRY: &[Field] = &[
    Field::required("at", Type::String, "When"),
    Field::required("event", Type::String, "What happened"),
    Field::optional("session_id", Type::String, "External session ID"),
    Field::optional("actor_id", Type::String, "Who acted"),
    Field::optional("detail", Type::String, "Detail"),
];

const APPROVE_INPUT: &[Field] = &[
    Field::required(
        "code",
        Type::String,
        "Approval code printed by `agent-os login`",
    ),
    Field::optional("ttl", Type::String, "Lease lifetime (e.g. 8h, 24h, 7d)"),
    Field::optional(
        "takeover",
        Type::Boolean,
        "Park a live managed turn instead of refusing",
    ),
];

const SESSIONS_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Agent @handle or employee UUIDv7"),
    Field::optional(
        "history_limit",
        Type::Integer,
        "Most recent history entries, 1 to 200 (default 20; `--limit` works too)",
    ),
];

const REVOKE_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Agent @handle or employee UUIDv7"),
    Field::optional("session", Type::String, "Only this external session ID"),
];

const SESSIONS_OUTPUT: &[Field] = &[
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::optional("active", Type::Object(SESSION_VIEW), "Active lease"),
    Field::required(
        "history",
        Type::List(&Type::Object(HISTORY_ENTRY)),
        "Recent sign-in history",
    ),
];

const REVOKE_OUTPUT: &[Field] = &[
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::required("revoked", Type::Boolean, "Whether a lease was ended"),
    Field::optional("session_id", Type::String, "Ended external session ID"),
];

/// The operator rows of the external lifecycle.
pub const EXTERNAL_COMMANDS: &[Command] = &[
    Command {
        resource: "agent",
        verb: "approve",
        summary: "Approve an external workspace sign-in",
        call: "member.external_approve",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: APPROVE_INPUT,
        content_field: None,
        output: SESSION_VIEW,
        errors: ERRORS,
        example: Example {
            input: r#"{"code":"K7QF-9XM2","ttl":"24h"}"#,
            output: r#"{"session_id":"ext-0193a5c0-0000-7000-8000-000000000070","employee_id":"0193a5c0-0000-7000-8000-000000000051","handle":"ada","workspace_label":"~/src/app","pubkey_fingerprint":"3f9c2a7d10b4e8f1","approved_by":"0193a5c0-0000-7000-8000-000000000001","approved_at":"2026-11-26T10:00:00.000Z","expires_at":"2026-11-27T10:00:00.000Z","active":true}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "sessions",
        summary: "List an agent's external sessions",
        call: "member.external_sessions",
        kind: Kind::Read,
        input: SESSIONS_INPUT,
        content_field: None,
        output: SESSIONS_OUTPUT,
        errors: ERRORS,
        example: Example {
            input: r#"{"member":"@ada"}"#,
            output: r#"{"employee_id":"0193a5c0-0000-7000-8000-000000000051","history":[{"at":"2026-11-26T10:00:00.000Z","event":"approved","session_id":"ext-0193a5c0-0000-7000-8000-000000000070"}]}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "revoke",
        summary: "Revoke an agent's external session",
        call: "member.external_revoke",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: REVOKE_INPUT,
        content_field: None,
        output: REVOKE_OUTPUT,
        errors: ERRORS,
        example: Example {
            input: r#"{"member":"@ada"}"#,
            output: r#"{"employee_id":"0193a5c0-0000-7000-8000-000000000051","revoked":true,"session_id":"ext-0193a5c0-0000-7000-8000-000000000070"}"#,
        },
    },
];

/// Rewrites the positional shorthand of the operator verbs into their named flags:
/// `agent approve <code>`, `agent sessions <member>`, `agent revoke <member> [<session>]`, and
/// `agent sessions --limit <n>` into `--history-limit <n>` (`--limit` is a list's option).
///
/// The bare words may stand anywhere before `--`: a caller (`agentic-org`'s office proxy) puts
/// `--format json|compact` and `--dry-run` right after the command path, before them. Flags are
/// stepped over the way the table's parser reads them ([`org_command::Command::flag_takes_value`]),
/// so a flag's value (`--ttl 8h`) is never taken for a bare word. Any other command line, and one
/// with no bare word or more bare words than the verb has positions, is returned unchanged (`None`)
/// for the table to refuse as written.
pub fn rewrite_positional(args: &[String]) -> Option<Vec<String>> {
    let [resource, verb, rest @ ..] = args else {
        return None;
    };
    if resource != "agent" {
        return None;
    }
    let names: &[&str] = match verb.as_str() {
        "approve" => &["--code"],
        "sessions" => &["--member"],
        "revoke" => &["--member", "--session"],
        _ => return None,
    };
    let command = crate::table::runtime_table().find(resource, verb)?;
    let mut bare: Vec<&String> = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    let mut aliased = false;
    let mut at = 0;
    while let Some(arg) = rest.get(at) {
        at += 1;
        if arg == "--" {
            flags.extend_from_slice(&rest[at - 1..]);
            break;
        }
        if !arg.starts_with('-') {
            bare.push(arg);
            continue;
        }
        let name = arg
            .strip_prefix("--")
            .filter(|n| !n.is_empty() && !n.contains('='));
        let inline_limit = arg.strip_prefix("--limit=");
        if verb == "sessions" && (name == Some("limit") || inline_limit.is_some()) {
            aliased = true;
            flags.push(inline_limit.map_or_else(
                || "--history-limit".to_owned(),
                |value| format!("--history-limit={value}"),
            ));
        } else {
            flags.push(arg.clone());
        }
        let takes_value = name.is_some_and(|n| command.flag_takes_value(n) == Some(true));
        if let Some(value) = rest.get(at).filter(|_| takes_value) {
            flags.push(value.clone());
            at += 1;
        }
    }
    if (bare.is_empty() && !aliased) || bare.len() > names.len() {
        return None;
    }
    let mut out = vec![resource.clone(), verb.clone()];
    for (name, value) in names.iter().zip(bare) {
        out.push((*name).to_owned());
        out.push(value.clone());
    }
    out.extend(flags);
    Some(out)
}

fn input_str<'a>(request: &'a Request, field: &str) -> Option<&'a str> {
    request.input.get(field).and_then(Value::as_str)
}

fn required<'a>(request: &'a Request, field: &str) -> Result<&'a str, Refusal> {
    input_str(request, field).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Missing required --{field} flag."),
            format!("Pass --{field} <value>."),
        )
    })
}

fn no_dry_run(request: &Request) -> Result<(), Refusal> {
    if request.dry_run {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            "This command has no dry run.",
            "Run it without --dry-run.",
        ));
    }
    Ok(())
}

fn committed(request: &Request, raw: Value) -> Answer {
    let mut answer = Answer::completed(raw);
    answer.operation_id = request.idempotency_key.clone();
    answer.ack = Some(Ack::Committed);
    answer
}

/// Dispatches `agent approve|sessions|revoke`; `None` for any other command.
pub fn try_execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    if request.command.resource != "agent" {
        return None;
    }
    match request.command.verb {
        "approve" => Some(approve(transport, request)),
        "sessions" => Some(sessions(transport, request)),
        "revoke" => Some(revoke(transport, request)),
        _ => None,
    }
}

fn approve<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    no_dry_run(request)?;
    let mut params = Map::new();
    params.insert("code".into(), json!(required(request, "code")?));
    if let Some(ttl) = input_str(request, "ttl") {
        params.insert("ttl_secs".into(), json!(super::client::parse_ttl(ttl)?));
    }
    if request.input.get("takeover").and_then(Value::as_bool) == Some(true) {
        params.insert("takeover".into(), json!(true));
    }
    if let Some(key) = &request.idempotency_key {
        params.insert("command_id".into(), json!(key));
    }
    let raw = transport.call_method(MemberExternalApprove::METHOD, Value::Object(params))?;
    Ok(committed(request, raw))
}

fn sessions<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    params.insert("employee_id".into(), json!(required(request, "member")?));
    if let Some(limit) = request.input.get("history_limit") {
        let limit = limit
            .as_u64()
            .filter(|n| (1..=200).contains(n))
            .ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    "--history-limit is a number from 1 to 200.",
                    "Pass --limit 50.",
                )
            })?;
        params.insert("limit".into(), json!(limit));
    }
    let raw = transport.call_method(MemberExternalSessions::METHOD, Value::Object(params))?;
    Ok(Answer::completed(raw))
}

fn revoke<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    no_dry_run(request)?;
    let mut params = Map::new();
    params.insert("employee_id".into(), json!(required(request, "member")?));
    if let Some(session) = input_str(request, "session") {
        params.insert("session_id".into(), json!(session));
    }
    if let Some(key) = &request.idempotency_key {
        params.insert("command_id".into(), json!(key));
    }
    let raw = transport.call_method(MemberExternalRevoke::METHOD, Value::Object(params))?;
    Ok(committed(request, raw))
}
