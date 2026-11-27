//! The one place a feature slice reads a request's input and builds its answer: every
//! `--flag` read, every refusal of input the office would refuse anyway, the command and entity
//! IDs of a write, and the acknowledged answer of a write. A slice never writes its own copy.
//!
//! Each reader takes the [`Request`] or its input object alike ([`Input`]).

use org_command::{Ack, Answer, Refusal, Request};
use org_proto::hiring::EntityOpResult;
use org_proto::org::{CommandAccepted, CommandStatus};
use org_proto::{Call, ErrorCode};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::runtime::transport::{self, write_ack, OfficeTransport};

/// What a reader reads: a request's input object.
pub(crate) trait Input {
    fn fields(&self) -> &Map<String, Value>;
}

impl Input for Request {
    fn fields(&self) -> &Map<String, Value> {
        &self.input
    }
}

impl Input for Map<String, Value> {
    fn fields(&self) -> &Map<String, Value> {
        self
    }
}

/// The flag a field is given with: `max_results` is `--max-results`.
fn flag(field: &str) -> String {
    field.replace('_', "-")
}

fn invalid(message: String, hint: String) -> Refusal {
    Refusal::new(ErrorCode::InvalidRequest, message, hint)
}

/// The text of `field`, when given.
pub(crate) fn opt_str<'a, I: Input + ?Sized>(input: &'a I, field: &str) -> Option<&'a str> {
    input.fields().get(field).and_then(Value::as_str)
}

/// The text of `field`, owned, when given.
pub(crate) fn opt_string<I: Input + ?Sized>(input: &I, field: &str) -> Option<String> {
    opt_str(input, field).map(str::to_owned)
}

/// The yes-or-no `field`, when given.
pub(crate) fn opt_bool<I: Input + ?Sized>(input: &I, field: &str) -> Option<bool> {
    input.fields().get(field).and_then(Value::as_bool)
}

/// Whether the yes-or-no `field` is given and true.
pub(crate) fn is_set<I: Input + ?Sized>(input: &I, field: &str) -> bool {
    opt_bool(input, field).unwrap_or(false)
}

/// The text of `field`, refused when missing.
pub(crate) fn require_str<'a, I: Input + ?Sized>(
    input: &'a I,
    field: &str,
) -> Result<&'a str, Refusal> {
    opt_str(input, field).ok_or_else(|| {
        let flag = flag(field);
        invalid(
            format!("Missing required --{flag} flag."),
            format!("Pass --{flag} <value>."),
        )
    })
}

/// The non-negative whole number `field`, when given (`null` is not given).
pub(crate) fn opt_u64<I: Input + ?Sized>(input: &I, field: &str) -> Result<Option<u64>, Refusal> {
    match input.fields().get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(|| {
            let flag = flag(field);
            invalid(
                format!("`--{flag}` must be a non-negative integer."),
                format!("Pass a non-negative integer for `--{flag}`."),
            )
        }),
    }
}

/// The non-negative whole number `field` as a byte offset or count, when given.
pub(crate) fn opt_usize<I: Input + ?Sized>(
    input: &I,
    field: &str,
) -> Result<Option<usize>, Refusal> {
    let Some(value) = opt_u64(input, field)? else {
        return Ok(None);
    };
    usize::try_from(value).map(Some).map_err(|_| {
        let flag = flag(field);
        invalid(
            format!("`--{flag}` exceeds supported byte offset range"),
            format!("Pass a smaller non-negative byte offset for `--{flag}`."),
        )
    })
}

/// The texts of the list `field`, when given.
pub(crate) fn opt_str_list<I: Input + ?Sized>(input: &I, field: &str) -> Option<Vec<String>> {
    input
        .fields()
        .get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
}

/// Sets `key` to the text `value` when there is one.
pub(crate) fn insert_opt(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        map.insert(key.into(), Value::String(value.to_owned()));
    }
}

/// Copies the text `src` of `request` into `params` as `dst`, when given.
pub(crate) fn copy_opt_str(
    request: &Request,
    params: &mut Map<String, Value>,
    src: &str,
    dst: &str,
) {
    if let Some(value) = opt_str(request, src) {
        params.insert(dst.into(), json!(value));
    }
}

/// Copies the yes-or-no `src` of `request` into `params` as `dst`, when given.
pub(crate) fn copy_opt_bool(
    request: &Request,
    params: &mut Map<String, Value>,
    src: &str,
    dst: &str,
) {
    if let Some(value) = opt_bool(request, src) {
        params.insert(dst.into(), json!(value));
    }
}

/// The `--if-match` revision of an update, refused when missing.
pub(crate) fn require_if_match(request: &Request) -> Result<u64, Refusal> {
    request.if_match.ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "This update requires --if-match <revision>.",
            "Inspect the current record and pass its revision with --if-match.",
        )
    })
}

/// Refuses `--dry-run` on a write the office cannot decide without making: nothing is sent.
pub(crate) fn reject_dry_run(request: &Request) -> Result<(), Refusal> {
    if request.dry_run {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            format!(
                "`--dry-run` is not supported on `{} {}` yet.",
                request.command.resource, request.command.verb
            ),
            "Re-run without `--dry-run` to apply this change.",
        ));
    }
    Ok(())
}

/// Refuses a write that is not a dry run and carries no idempotency key.
pub(crate) fn require_key(request: &Request) -> Result<(), Refusal> {
    if request.idempotency_key.is_none() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Write is missing an idempotency key.",
            "Pass --idempotency-key <key> or let the CLI make one.",
        ));
    }
    Ok(())
}

/// The command ID of a write that has no dry run ([`reject_dry_run`]), derived in `domain`
/// ([`transport::command_id`]): the same key always gives the same ID.
pub(crate) fn command_id(request: &Request, domain: &str) -> Result<String, Refusal> {
    reject_dry_run(request)?;
    transport::command_id(request, domain)
}

/// The ID of the record a create makes: the caller's own in `field` when the command has one,
/// else derived in `domain` ([`transport::entity_id`]).
pub(crate) fn entity_id(
    request: &Request,
    field: Option<&str>,
    domain: &str,
) -> Result<String, Refusal> {
    transport::entity_id(request, field.and_then(|f| opt_str(request, f)), domain)
}

/// The refusal of an office reply this build cannot read.
pub(crate) fn decode_err(method: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned an unreadable `{method}` reply."),
        "Retry the command or check the office version.",
    )
}

/// `value` as the JSON of an answer or a call's parameters; a value that cannot be written is
/// refused instead of answered.
pub(crate) fn encode<T: Serialize + ?Sized>(value: &T, method: &str) -> Result<Value, Refusal> {
    serde_json::to_value(value).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            format!("agent-os could not write the `{method}` data."),
            "Retry the command; if it persists, the office and agent-os versions differ.",
        )
    })
}

/// One typed office call: the parameters written, the reply read as `C::Result`.
pub(crate) fn call_rpc<C: Call, T: OfficeTransport + ?Sized>(
    transport: &mut T,
    params: &C::Params,
) -> Result<C::Result, Refusal> {
    let raw_params = serde_json::to_value(params).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "failed to encode request parameters",
            "Check the command arguments.",
        )
    })?;
    let raw = transport.call_method(C::METHOD, raw_params)?;
    serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            format!("office returned an unreadable `{}` reply", C::METHOD),
            "Retry the command or check the office version.",
        )
    })
}

/// The answer of a completed write: its operation ID (the key), the acknowledgement its row
/// declares ([`write_ack`]) and the revision it made.
pub(crate) fn write_answer(request: &Request, data: Value, revision: Option<u64>) -> Answer {
    Answer {
        operation_id: request.idempotency_key.clone(),
        ack: Some(write_ack(request)),
        revision,
        ..Answer::completed(data)
    }
}

/// Sends an organization command (`team.*`, `member.*`, `project.create`) whose reply is the
/// office's [`CommandAccepted`], answering `{id, status, position}` for `target_id`. A dry run is
/// sent as one (`dry_run`); any other write carries the command ID of its key.
pub(crate) fn send_command<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    method: &str,
    mut params: Map<String, Value>,
    target_id: &str,
) -> Result<Answer, Refusal> {
    if request.dry_run {
        params.insert("dry_run".into(), json!(true));
    } else {
        require_key(request)?;
        params.insert(
            "command_id".into(),
            json!(transport::command_id(request, "org-command")?),
        );
    }
    let raw = transport.call_method(method, Value::Object(params))?;
    let accepted: CommandAccepted = serde_json::from_value(raw).map_err(|_| decode_err(method))?;
    let status = match accepted.status {
        CommandStatus::Committed => "committed",
        CommandStatus::Checked => "checked",
        CommandStatus::Queued => "queued",
        _ => return Err(decode_err(method)),
    };
    let mut data = Map::new();
    data.insert("id".into(), json!(target_id));
    data.insert("status".into(), json!(status));
    if let Some(pos) = &accepted.position {
        data.insert(
            "position".into(),
            json!({ "epoch": pos.epoch, "seq": pos.seq }),
        );
    }
    let mut answer = Answer::completed(Value::Object(data));
    if !request.dry_run {
        answer.operation_id = request.idempotency_key.clone();
        answer.ack = Some(write_ack(request));
    }
    Ok(answer)
}

/// The command ID of a hiring-family write (role, hire, agent, permission and team-limit
/// changes), which has no dry run and always carries a key.
pub(crate) fn hiring_command_id(request: &Request) -> Result<String, Refusal> {
    reject_dry_run(request)?;
    require_key(request)?;
    transport::command_id(request, "hiring-command")
}

/// Sends a hiring-family write whose reply is the office's [`EntityOpResult`], answered with
/// the revision it made.
pub(crate) fn send_entity_write<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    method: &str,
    mut params: Map<String, Value>,
) -> Result<Answer, Refusal> {
    params.insert("command_id".into(), json!(hiring_command_id(request)?));
    let raw = transport.call_method(method, Value::Object(params))?;
    let out: EntityOpResult = serde_json::from_value(raw).map_err(|_| decode_err(method))?;
    Ok(revised_answer(request, encode(&out, method)?, out.revision))
}

/// The answer of a write that made `revision`, acknowledged as its row declares.
pub(crate) fn revised_answer(request: &Request, data: Value, revision: u64) -> Answer {
    let mut answer = Answer::completed(data);
    answer.operation_id = request.idempotency_key.clone();
    answer.revision = Some(revision);
    answer.ack = Some(write_ack(request));
    answer
}

/// The answer of a write the office acknowledges at commit, with its key as operation ID.
pub(crate) fn committed_answer(request: &Request, data: Value) -> Answer {
    let mut answer = Answer::completed(data);
    answer.operation_id = request.idempotency_key.clone();
    answer.ack = Some(Ack::Committed);
    answer
}

/// Parses a duration: `90`, `90s`, `45m`, `24h` or `7d`, in seconds (`--ttl`).
pub(crate) fn parse_ttl(text: &str) -> Result<u64, Refusal> {
    let text = text.trim();
    let (digits, unit) = match text.char_indices().last() {
        Some((at, c)) if c.is_ascii_alphabetic() => (&text[..at], c),
        _ => (text, 's'),
    };
    let scale = match unit.to_ascii_lowercase() {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => 0,
    };
    digits
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && scale > 0)
        .and_then(|n| n.checked_mul(scale))
        .ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                format!("`{text}` is not a duration."),
                "Pass --ttl like 90m, 24h or 7d.",
            )
        })
}

/// Rewrites legacy `note get ...` (from `task-note get` / `task note get`) to `note inspect ...`
/// after shorthand alias expansion, preserving backward compatibility without extra catalogue bytes.
pub(crate) fn rewrite_note_args(args: &[String]) -> Option<Vec<String>> {
    if args.first().map(String::as_str) == Some("note")
        && args.get(1).map(String::as_str) == Some("get")
    {
        let mut rewritten = Vec::with_capacity(args.len());
        rewritten.push("note".to_owned());
        rewritten.push("inspect".to_owned());
        rewritten.extend_from_slice(&args[2..]);
        return Some(rewritten);
    }
    None
}

#[cfg(test)]
mod tests;
