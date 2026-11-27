//! `process list|create|update`: the organization, team and project processes and standards
//! that apply, and writing them (`process inspect` reads one: [`super::process_inspect`]).

use org_command::{Answer, Refusal, Request};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, ProcessEffective, ProcessEffectiveParams,
    ProcessEffectiveResult, ProcessForWire, ProcessScopeWire, ProcessWrite, ProcessWriteParams,
    ProcessWriteResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{Map, Value};

use super::process_inspect::read_process_inspect;
use super::{unreadable, unreadable_write};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{command_id, encode, opt_string, require_str};
use crate::runtime::transport::{paginate, parse_cursor, write_ack, OfficeTransport};

/// `Use when:` guidance of each `process` command.
pub(super) const CUES: &[CommandCue] = &[
    ("process", "create", "authoring a new organization, team, or project process or standard document."),
    ("process", "list", "checking which processes and mandatory/overridable standards apply to your current project or task."),
    ("process", "update", "publishing a new revision of an existing process or standard (requires `--if-match`)."),
];

/// Runs a `process` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "list" => read_process_list(transport, request),
        "inspect" => read_process_inspect(transport, request),
        "create" => write_process(transport, request, false),
        "update" => write_process(transport, request, true),
        _ => return None,
    })
}

fn parse_kind(raw: &str) -> Result<DocumentKindWire, Refusal> {
    match raw {
        "process" => Ok(DocumentKindWire::Process),
        "standard" => Ok(DocumentKindWire::Standard),
        other => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("invalid kind `{other}`"),
            "Use `process` or `standard`.",
        )),
    }
}

fn parse_status(raw: &str) -> Result<DocumentStatusWire, Refusal> {
    match raw {
        "draft" => Ok(DocumentStatusWire::Draft),
        "current" => Ok(DocumentStatusWire::Current),
        "deprecated" => Ok(DocumentStatusWire::Deprecated),
        other => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("invalid status `{other}`"),
            "Use `draft`, `current`, or `deprecated`.",
        )),
    }
}

pub(super) fn parse_for(input: &Map<String, Value>) -> Result<Option<ProcessForWire>, Refusal> {
    match (opt_string(input, "task"), opt_string(input, "project")) {
        (Some(_), Some(_)) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--task` and `--project` are mutually exclusive",
            "Pass either `--task` or `--project`, not both.",
        )),
        (Some(task_id), None) => Ok(Some(ProcessForWire::Task { task_id })),
        (None, Some(project_id)) => Ok(Some(ProcessForWire::Project { project_id })),
        (None, None) => Ok(None),
    }
}

fn read_process_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let kind = match opt_string(&request.input, "kind") {
        Some(raw) => Some(parse_kind(&raw)?),
        None => None,
    };
    let params = ProcessEffectiveParams {
        employee_id: None,
        r#for: parse_for(&request.input)?,
        kind,
    };
    let raw = transport.call_method(
        ProcessEffective::METHOD,
        encode(&params, ProcessEffective::METHOD)?,
    )?;
    let res: ProcessEffectiveResult =
        serde_json::from_value(raw).map_err(|_| unreadable("process.effective", "process list"))?;
    let items = match serde_json::to_value(res.entries) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

fn write_process<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    is_update: bool,
) -> Result<Answer, Refusal> {
    let command_id = command_id(request, "process-write")?;
    let document_id = if is_update {
        Some(require_str(&request.input, "id")?.to_owned())
    } else {
        None
    };
    let kind = parse_kind(require_str(&request.input, "kind")?)?;
    let title = require_str(&request.input, "title")?.to_owned();
    let body_markdown = require_str(&request.input, "body")?.to_owned();
    let key = opt_string(&request.input, "key");
    let status = match opt_string(&request.input, "status") {
        Some(raw) => parse_status(&raw)?,
        None => DocumentStatusWire::Current,
    };
    let owner_employee_id = opt_string(&request.input, "owner");
    // A process is overridable unless `--non-overridable` says otherwise: a yes-or-no flag
    // only ever turns its meaning on (`doc-28` §2.1).
    let overridable = !request
        .input
        .get("non_overridable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let project_opt = opt_string(&request.input, "project");
    let team_opt = opt_string(&request.input, "team");
    let scope = match (project_opt, team_opt) {
        (Some(_), Some(_)) => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "provide at most one of `--project` or `--team`",
                "Omit both for organization scope.",
            ));
        }
        (Some(project_id), None) => ProcessScopeWire::Project { project_id },
        (None, Some(team_id)) => ProcessScopeWire::Team { team_id },
        (None, None) => ProcessScopeWire::Organization,
    };

    let params = ProcessWriteParams {
        command_id: command_id.clone(),
        document_id,
        kind,
        scope,
        key,
        title,
        status,
        owner_employee_id,
        overridable,
        body_markdown,
        expected_revision: request.if_match,
    };
    let raw =
        transport.call_method(ProcessWrite::METHOD, encode(&params, ProcessWrite::METHOD)?)?;
    let res: ProcessWriteResult =
        serde_json::from_value(raw).map_err(|_| unreadable_write(ProcessWrite::METHOD))?;
    let mut ans = Answer::completed(encode(&res, ProcessWrite::METHOD)?);
    ans.ack = Some(write_ack(request));
    ans.operation_id = request.idempotency_key.clone().or(Some(command_id));
    Ok(ans)
}
