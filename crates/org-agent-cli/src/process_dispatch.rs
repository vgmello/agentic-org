//! Dispatch for C5 `process` and `skill` commands over [`OfficeTransport`] (`TASK-44`).

use org_command::{Ack, Answer, Refusal, Request};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, ProcessEffective, ProcessEffectiveParams,
    ProcessEffectiveResult, ProcessForWire, ProcessScopeWire, ProcessShow, ProcessShowParams,
    ProcessShowResult, ProcessWhy, ProcessWhyParams, ProcessWhyResult, ProcessWrite,
    ProcessWriteParams, ProcessWriteResult, SkillList, SkillListParams, SkillListResult, SkillRead,
    SkillReadParams, SkillReadResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{Map, Value};

use crate::dispatch::OfficeTransport;
use crate::keys::try_new_uuid_v7;

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
            "list" => Some(read_skill_list(transport)),
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
    Ok(Answer::completed(
        serde_json::to_value(res.entries).unwrap(),
    ))
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

    if why {
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
        let raw =
            transport.call_method(ProcessWhy::METHOD, serde_json::to_value(params).unwrap())?;
        let res: ProcessWhyResult = serde_json::from_value(raw).map_err(|_| {
            Refusal::new(
                ErrorCode::Unavailable,
                "unreadable process.why response",
                "Retry.",
            )
        })?;
        return Ok(Answer::completed(serde_json::to_value(res).unwrap()));
    }

    if key.is_none() && document_id.is_none() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "provide `--key` or `--id`",
            "Run `agent-os process list` to see available keys.",
        ));
    }
    let params = ProcessShowParams {
        key,
        document_id,
        employee_id: None,
        r#for: target_for,
        offset: None,
    };
    let raw = transport.call_method(ProcessShow::METHOD, serde_json::to_value(params).unwrap())?;
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
    let command_id = match request.idempotency_key.as_deref() {
        Some(k) if org_proto::is_uuid_v7(k) => k.to_owned(),
        Some(k) => crate::dispatch::derive_uuid_v7("process-write", k),
        None => try_new_uuid_v7().ok_or_else(|| {
            Refusal::new(
                ErrorCode::StorageError,
                "could not mint UUIDv7 command_id",
                "Retry.",
            )
        })?,
    };
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
    let overridable = request
        .input
        .get("overridable")
        .and_then(Value::as_bool)
        .unwrap_or(true);
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
    ans.ack = Some(Ack::Committed);
    ans.operation_id = Some(command_id);
    Ok(ans)
}

fn read_skill_list<T: OfficeTransport + ?Sized>(transport: &mut T) -> Result<Answer, Refusal> {
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
    Ok(Answer::completed(serde_json::to_value(res.skills).unwrap()))
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
mod tests {
    use super::*;
    use crate::dispatch::OfficeTransport;
    use org_proto::processes::{
        EffectiveProcessEntryWire, ProcessDocumentWire, ProcessLayerWire, SkillSummaryWire,
    };

    struct StubTransport {
        seen: Vec<(String, Value)>,
    }

    impl OfficeTransport for StubTransport {
        fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
            self.seen.push((method.to_owned(), params));
            match method {
                "process.effective" => Ok(serde_json::to_value(ProcessEffectiveResult {
                    scope_chain: vec!["Organization".into()],
                    entries: vec![EffectiveProcessEntryWire {
                        id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into(),
                        key: Some("code-review".into()),
                        kind: DocumentKindWire::Process,
                        applies: true,
                        mandatory: false,
                        winner_kind: Some("defined".into()),
                        document: sample_doc("code-review"),
                        replaces: None,
                        problem: None,
                        chain: vec![],
                    }],
                    problems: vec![],
                    computed_at: "2026-11-22T00:00:00Z".into(),
                    stale_since: None,
                })
                .unwrap()),
                "process.why" => Ok(serde_json::to_value(ProcessWhyResult {
                    key: "code-review".into(),
                    chain: vec![ProcessLayerWire {
                        document: sample_doc("code-review"),
                        outcome: "wins".into(),
                        detail: None,
                    }],
                    winner: Some(sample_doc("code-review")),
                })
                .unwrap()),
                "process.show" => Ok(serde_json::to_value(ProcessShowResult {
                    document: sample_doc("code-review"),
                    blob: "sha256:abc".into(),
                    content: "# Review".into(),
                    total_bytes: 8,
                    next_offset: None,
                    parent: None,
                })
                .unwrap()),
                "process.write" => Ok(serde_json::to_value(ProcessWriteResult {
                    document_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into(),
                    document: sample_doc("new-proc"),
                    revision: 1,
                })
                .unwrap()),
                "skill.list" => Ok(serde_json::to_value(SkillListResult {
                    skills: vec![SkillSummaryWire {
                        key: "agent-os-basics".into(),
                        title: "Using agent-os".into(),
                        description: "Basics".into(),
                        source: "built_in".into(),
                    }],
                })
                .unwrap()),
                "skill.read" => Ok(serde_json::to_value(SkillReadResult {
                    key: "agent-os-basics".into(),
                    title: "Using agent-os".into(),
                    description: "Basics".into(),
                    source: "built_in".into(),
                    body_markdown: "# Basics".into(),
                })
                .unwrap()),
                other => panic!("unexpected method {other}"),
            }
        }
    }

    fn sample_doc(key: &str) -> ProcessDocumentWire {
        ProcessDocumentWire {
            id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into(),
            key: Some(key.into()),
            revision: 1,
            kind: DocumentKindWire::Process,
            title: "Code Review".into(),
            status: DocumentStatusWire::Current,
            owner_employee_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2".into()),
            owner_name: Some("Owner".into()),
            level: "organization".into(),
            team_id: None,
            project_id: None,
            scope_name: Some("Organization".into()),
            overridable: true,
            summary: "Review all PRs".into(),
            body_markdown: "# Review".into(),
            path: "state/processes/0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1.md".into(),
            created_at: None,
            created_by_kind: "employee".into(),
            created_by_id: None,
            updated_at: None,
            updated_by_kind: "employee".into(),
            updated_by_id: None,
        }
    }

    struct StubOffice {
        tr: StubTransport,
    }

    impl org_command::Office for StubOffice {
        fn call(&mut self, request: &org_command::Request) -> Result<Answer, Refusal> {
            try_execute(&mut self.tr, request).unwrap_or_else(|| {
                panic!(
                    "unhandled command {} {}",
                    request.command.resource, request.command.verb
                )
            })
        }
    }

    #[test]
    fn process_and_skill_cli_commands_dispatch_expected_rpcs() {
        let mut office = StubOffice {
            tr: StubTransport { seen: vec![] },
        };

        for args in [
            vec!["process", "list"],
            vec!["process", "inspect", "--id", "code-review"],
            vec!["process", "inspect", "--key", "code-review", "--why"],
            vec![
                "process",
                "create",
                "--project",
                "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a9",
                "--kind",
                "process",
                "--title",
                "Local Setup",
                "--body",
                "# Setup",
            ],
            vec!["skill", "list"],
            vec!["skill", "read", "--name", "agent-os-basics"],
        ] {
            let owned: Vec<String> = args.into_iter().map(str::to_owned).collect();
            let out = crate::execute_with_office("agent-os", &owned, &mut office);
            assert_eq!(out.exit_code, 0, "command {owned:?} failed: {}", out.stdout);
        }

        let methods: Vec<&str> = office.tr.seen.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            vec![
                "process.effective",
                "process.show",
                "process.why",
                "process.write",
                "skill.list",
                "skill.read",
            ]
        );

        // Passing both --task and --project is refused cleanly.
        let conflict_args: Vec<String> =
            vec!["process", "list", "--task", "T-1", "--project", "P-1"]
                .into_iter()
                .map(str::to_owned)
                .collect();
        let bad = crate::execute_with_office("agent-os", &conflict_args, &mut office);
        assert_ne!(bad.exit_code, 0);
        assert!(bad.stdout.contains("mutually exclusive"));
    }
}
