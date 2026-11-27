//! Operator rows `agent approve|sessions|revoke` on the shared `org-command` grammar and their
//! dispatch (`TASK-137`). They run as `agentic-org agent ...` over a signed-in (grant-bound)
//! channel; an agent's own `agent-os` connection is refused by the office.
//!
//! `agentic-org agent approve <code>`, `agent sessions @<handle>` and `agent revoke @<handle>
//! [<session_id>]` are accepted as shorthand for the named flags ([`rewrite_positional`]).

use org_command::{Answer, CommandTable, Refusal, Request};
use org_proto::external_auth::{
    MemberExternalApprove, MemberExternalPending, MemberExternalRevoke, MemberExternalSessions,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};

use crate::runtime::domain::CommandCue;
use crate::runtime::help::ExtraVerb;
use crate::runtime::input::{committed_answer, opt_str, parse_ttl, reject_dry_run, require_str};
use crate::runtime::transport::OfficeTransport;

/// Operator-only `agent` verbs handled directly by `agentic-org` outside the `agent-os` table.
pub(crate) const OPERATOR_VERBS: &[ExtraVerb] = &[
    ExtraVerb {
        resource: "agent",
        verb: "bind",
        summary: "Bind a local workspace directly to an agent without an approval code",
        use_when: "binding a local folder or Git repo to an agent in one operator step (`--member <id> --workspace <dir> [--takeover]`).",
    },
    ExtraVerb {
        resource: "agent",
        verb: "renew",
        summary: "Extend an agent's active external workspace lease without re-login",
        use_when: "renewing an active external workspace lease up to 7 days (`--member @<handle> [--ttl 7d]`).",
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
pub(crate) fn rewrite_positional(table: &CommandTable, args: &[String]) -> Option<Vec<String>> {
    let [resource, verb, rest @ ..] = args else {
        return None;
    };
    let names: &[&str] = match (resource.as_str(), verb.as_str()) {
        ("agent", "approve") => &["--code"],
        ("agent", "sessions") => &["--member"],
        ("agent", "revoke") => &["--member", "--session"],
        _ => return None,
    };
    let command = table.find(resource, verb)?;
    let (bare, flags, aliased) = partition_positional(verb, command, rest);
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

fn partition_positional<'a>(
    verb: &str,
    command: &org_command::Command,
    rest: &'a [String],
) -> (Vec<&'a String>, Vec<String>, bool) {
    let mut bare = Vec::new();
    let mut flags = Vec::new();
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
        aliased |= push_flag_token(verb, command, arg, rest, &mut at, &mut flags);
    }
    (bare, flags, aliased)
}

fn push_flag_token(
    verb: &str,
    command: &org_command::Command,
    arg: &str,
    rest: &[String],
    at: &mut usize,
    flags: &mut Vec<String>,
) -> bool {
    let name = arg
        .strip_prefix("--")
        .filter(|n| !n.is_empty() && !n.contains('='));
    let aliased = if verb == "sessions" {
        if let Some(value) = arg.strip_prefix("--limit=") {
            flags.push(format!("--history-limit={value}"));
            true
        } else if name == Some("limit") {
            flags.push("--history-limit".to_owned());
            true
        } else {
            flags.push(arg.to_owned());
            false
        }
    } else {
        flags.push(arg.to_owned());
        false
    };
    let takes_value = name.is_some_and(|n| command.flag_takes_value(n) == Some(true));
    if let Some(value) = rest.get(*at).filter(|_| takes_value) {
        flags.push(value.clone());
        *at += 1;
    }
    aliased
}

/// `Use when:` guidance of each `agent` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("agent", "approve", "approving an external workspace's `agent-os login` code so it acts as the agent (`--takeover` parks a live managed turn)."),
    ("agent", "revoke", "ending an agent's external workspace session at once (its connections close and held wakes run again)."),
    ("agent", "sessions", "checking pending external workspace sign-in requests (`--pending`) or an agent's active external lease and sign-in history."),
];

/// Runs `agent approve|sessions|revoke`; `None` for any other `agent` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
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
    reject_dry_run(request)?;
    let mut params = Map::new();
    params.insert("code".into(), json!(require_str(request, "code")?));
    if let Some(ttl) = opt_str(request, "ttl") {
        params.insert("ttl_secs".into(), json!(parse_ttl(ttl)?));
    }
    if request.input.get("takeover").and_then(Value::as_bool) == Some(true) {
        params.insert("takeover".into(), json!(true));
    }
    if let Some(key) = &request.idempotency_key {
        params.insert("command_id".into(), json!(key));
    }
    let raw = transport.call_method(MemberExternalApprove::METHOD, Value::Object(params))?;
    Ok(committed_answer(request, raw))
}

fn sessions<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let pending = request
        .input
        .get("pending")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let member = opt_str(request, "member");
    if pending {
        if request.input.get("history_limit").is_some() {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "--history-limit cannot be used with --pending.",
                "Drop --limit / --history-limit when listing pending sign-in requests.",
            ));
        }
        let mut params = Map::new();
        if let Some(member) = member {
            params.insert("employee_id".into(), json!(member));
        }
        let raw = transport.call_method(MemberExternalPending::METHOD, Value::Object(params))?;
        return Ok(Answer::completed(raw));
    }
    let member = member.ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "--member is required unless --pending is given.",
            "Pass --member @<handle>, or pass --pending to list pending sign-in requests.",
        )
    })?;
    let mut params = Map::new();
    params.insert("employee_id".into(), json!(member));
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
    reject_dry_run(request)?;
    let mut params = Map::new();
    params.insert("employee_id".into(), json!(require_str(request, "member")?));
    if let Some(session) = opt_str(request, "session") {
        params.insert("session_id".into(), json!(session));
    }
    if let Some(key) = &request.idempotency_key {
        params.insert("command_id".into(), json!(key));
    }
    let raw = transport.call_method(MemberExternalRevoke::METHOD, Value::Object(params))?;
    Ok(committed_answer(request, raw))
}
