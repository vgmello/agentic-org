//! Dispatch for C5 `process` and `skill` commands over [`OfficeTransport`] (`TASK-44`).

use org_command::{Answer, Refusal, Request};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, ProcessEffective, ProcessEffectiveParams,
    ProcessEffectiveResult, ProcessForWire, ProcessScopeWire, ProcessShow, ProcessShowParams,
    ProcessShowResult, ProcessWhy, ProcessWhyParams, ProcessWhyResult, ProcessWrite,
    ProcessWriteParams, ProcessWriteResult, SkillList, SkillListParams, SkillListResult, SkillRead,
    SkillReadParams, SkillReadResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{Map, Value};

use crate::transport::{command_id, paginate, parse_cursor, write_ack, OfficeTransport};

pub fn try_execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "process" => match request.command.verb {
            "list" => Some(read_process_list(transport, request)),
            "inspect" => Some(read_process_inspect(transport, request)),
            "create" => Some(write_process(transport, request, false)),
            "update" => Some(write_process(transport, request, true)),
            _ => None,
        },
        "skill" => match request.command.verb {
            "list" => Some(read_skill_list(transport, request)),
            "read" => Some(read_skill_read(transport, request)),
            _ => None,
        },
        _ => None,
    }
}

fn opt_str(input: &Map<String, Value>, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn req_str<'a>(input: &'a Map<String, Value>, key: &str) -> Result<&'a str, Refusal> {
    input.get(key).and_then(Value::as_str).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`{key}` is required"),
            format!("Provide `{key}`."),
        )
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

fn parse_for(input: &Map<String, Value>) -> Result<Option<ProcessForWire>, Refusal> {
    match (opt_str(input, "task"), opt_str(input, "project")) {
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
    let kind = match opt_str(&request.input, "kind") {
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
        serde_json::to_value(params).unwrap(),
    )?;
    let res: ProcessEffectiveResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "unreadable process.effective response",
            "Retry.",
        )
    })?;
    let items = match serde_json::to_value(res.entries) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

fn opt_non_neg_u64(input: &Map<String, Value>, key: &str) -> Result<Option<u64>, Refusal> {
    let Some(raw) = input.get(key) else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    raw.as_u64().map(Some).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`--{key}` must be a non-negative integer"),
            format!("Pass a non-negative integer for `--{key}`."),
        )
    })
}

fn opt_non_neg_usize(input: &Map<String, Value>, key: &str) -> Result<Option<usize>, Refusal> {
    let Some(val) = opt_non_neg_u64(input, key)? else {
        return Ok(None);
    };
    usize::try_from(val).map(Some).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`--{key}` exceeds supported byte offset range"),
            format!("Pass a smaller non-negative byte offset for `--{key}`."),
        )
    })
}

fn read_process_why<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    key: Option<String>,
    document_id: Option<String>,
    offset: Option<usize>,
    expected_revision: Option<u64>,
    target_for: Option<ProcessForWire>,
) -> Result<Answer, Refusal> {
    if offset.is_some() || expected_revision.is_some() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--offset` and `--revision` cannot be combined with `--why`",
            "Omit `--why` to paginate body chunks, or omit `--offset`/`--revision` to inspect the inheritance chain.",
        ));
    }
    if document_id.is_some() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--id` cannot be combined with `--why`",
            "Pass `--key <key> --why` to inspect the inheritance chain for an override key.",
        ));
    }
    let Some(k) = key else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--why` requires `--key <key>`",
            "Pass `--key <key> --why`.",
        ));
    };
    let params = ProcessWhyParams {
        key: k,
        employee_id: None,
        r#for: target_for,
    };
    let raw = transport.call_method(ProcessWhy::METHOD, serde_json::to_value(params).unwrap())?;
    let res: ProcessWhyResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "unreadable process.why response",
            "Retry.",
        )
    })?;
    Ok(Answer::completed(serde_json::to_value(res).unwrap()))
}

fn read_process_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let why = request
        .input
        .get("why")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let key = opt_str(&request.input, "key");
    let document_id = opt_str(&request.input, "id");
    let target_for = parse_for(&request.input)?;
    let offset = opt_non_neg_usize(&request.input, "offset")?;
    let expected_revision = opt_non_neg_u64(&request.input, "revision")?;

    if why {
        return read_process_why(
            transport,
            key,
            document_id,
            offset,
            expected_revision,
            target_for,
        );
    }

    if key.is_some() && document_id.is_some() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--key` and `--id` are mutually exclusive",
            "Pass either `--key <key>` or `--id <document_id>`, not both.",
        ));
    }
    if key.is_none() && document_id.is_none() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "provide `--key` or `--id`",
            "Run `agent-os process list` to see available keys.",
        ));
    }
    if expected_revision.is_some() && document_id.is_none() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--revision` requires `--id <document_id>`",
            "Pass `--id <document.id> --revision <document.revision> --offset <next_offset>` from the first chunk.",
        ));
    }
    let params = ProcessShowParams {
        key,
        document_id,
        employee_id: None,
        r#for: target_for,
        offset,
        revision: expected_revision,
    };
    let raw = transport
        .call_method(ProcessShow::METHOD, serde_json::to_value(params).unwrap())
        .map_err(|mut refusal| {
            if refusal.code == ErrorCode::Conflict {
                refusal.hint = "The document changed while paginating. Discard accumulated chunks and re-run `process inspect` from offset 0.".to_owned();
            }
            refusal
        })?;
    let res: ProcessShowResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "unreadable process.show response",
            "Retry.",
        )
    })?;
    Ok(Answer::completed(serde_json::to_value(res).unwrap()))
}

fn write_process<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    is_update: bool,
) -> Result<Answer, Refusal> {
    if request.dry_run {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            "`--dry-run` is not supported on `process create|update` yet",
            "Re-run without `--dry-run`.",
        ));
    }
    let command_id = command_id(request, "process-write")?;
    let document_id = if is_update {
        Some(req_str(&request.input, "id")?.to_owned())
    } else {
        None
    };
    let kind = parse_kind(req_str(&request.input, "kind")?)?;
    let title = req_str(&request.input, "title")?.to_owned();
    let body_markdown = req_str(&request.input, "body")?.to_owned();
    let key = opt_str(&request.input, "key");
    let status = match opt_str(&request.input, "status") {
        Some(raw) => parse_status(&raw)?,
        None => DocumentStatusWire::Current,
    };
    let owner_employee_id = opt_str(&request.input, "owner");
    // A process is overridable unless `--non-overridable` says otherwise: a yes-or-no flag
    // only ever turns its meaning on (`doc-28` §2.1).
    let overridable = !request
        .input
        .get("non_overridable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let project_opt = opt_str(&request.input, "project");
    let team_opt = opt_str(&request.input, "team");
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
    let raw = transport.call_method(ProcessWrite::METHOD, serde_json::to_value(params).unwrap())?;
    let res: ProcessWriteResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "unreadable process.write response",
            "Retry.",
        )
    })?;
    let mut ans = Answer::completed(serde_json::to_value(res).unwrap());
    ans.ack = Some(write_ack(request));
    ans.operation_id = Some(command_id);
    Ok(ans)
}

fn read_skill_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let raw = transport.call_method(
        SkillList::METHOD,
        serde_json::to_value(SkillListParams {}).unwrap(),
    )?;
    let res: SkillListResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "unreadable skill.list response",
            "Retry.",
        )
    })?;
    let items = match serde_json::to_value(res.skills) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

fn read_skill_read<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = req_str(&request.input, "name")?.to_owned();
    let raw = transport.call_method(
        SkillRead::METHOD,
        serde_json::to_value(SkillReadParams { name }).unwrap(),
    )?;
    let res: SkillReadResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "unreadable skill.read response",
            "Retry.",
        )
    })?;
    Ok(Answer::completed(serde_json::to_value(res).unwrap()))
}

#[cfg(test)]
mod tests;
