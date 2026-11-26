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

use crate::keys::try_new_uuid_v7;
use crate::transport::OfficeTransport;

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
    let command_id = match request.idempotency_key.as_deref() {
        Some(k) if org_proto::is_uuid_v7(k) => k.to_owned(),
        Some(k) => crate::transport::derive_uuid_v7("process-write", k),
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
    use crate::transport::OfficeTransport;
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

    #[test]
    fn process_inspect_why_preserves_chain_and_winner_through_shape_conform() {
        struct WhyTransport;
        impl OfficeTransport for WhyTransport {
            fn call_method(&mut self, method: &str, _params: Value) -> Result<Value, Refusal> {
                assert_eq!(method, "process.why");
                let mut org_doc = sample_doc("code-review");
                org_doc.level = "organization".into();
                org_doc.title = "Org Code Review".into();
                org_doc.scope_name = Some("Organization".into());
                let mut team_doc = sample_doc("code-review");
                team_doc.id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into();
                team_doc.level = "team".into();
                team_doc.team_id = Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f607110".into());
                team_doc.scope_name = Some("Platform".into());
                team_doc.title = "Team Code Review".into();
                let mut proj_doc = sample_doc("code-review");
                proj_doc.id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071bb".into();
                proj_doc.level = "project".into();
                proj_doc.project_id = Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f607120".into());
                proj_doc.scope_name = Some("Billing".into());
                proj_doc.title = "Project Code Review".into();
                proj_doc.revision = 2;
                Ok(serde_json::to_value(ProcessWhyResult {
                    key: "code-review".into(),
                    chain: vec![
                        ProcessLayerWire {
                            document: org_doc,
                            outcome: "replaced".into(),
                            detail: None,
                        },
                        ProcessLayerWire {
                            document: team_doc,
                            outcome: "replaced".into(),
                            detail: Some("replaced by project layer".into()),
                        },
                        ProcessLayerWire {
                            document: proj_doc.clone(),
                            outcome: "applies".into(),
                            detail: None,
                        },
                    ],
                    winner: Some(proj_doc),
                })
                .unwrap())
            }
        }

        let mut office = crate::transport::TransportOffice(WhyTransport);
        let args: Vec<String> = vec!["process", "inspect", "--key", "code-review", "--why"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let out = crate::execute_with_office("agent-os", &args, &mut office);
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["ok"], Value::Bool(true));
        assert_eq!(env["data"]["key"], "code-review");
        let chain = env["data"]["chain"].as_array().expect("chain preserved");
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0]["document"]["level"], "organization");
        assert_eq!(chain[1]["outcome"], "replaced");
        assert_eq!(chain[1]["detail"], "replaced by project layer");
        assert_eq!(
            chain[1]["document"]["team_id"],
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f607110"
        );
        assert_eq!(chain[1]["document"]["scope_name"], "Platform");
        assert_eq!(chain[2]["outcome"], "applies");
        assert_eq!(
            chain[2]["document"]["project_id"],
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f607120"
        );
        assert_eq!(chain[2]["document"]["scope_name"], "Billing");
        assert_eq!(
            env["data"]["winner"]["id"],
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071bb"
        );
        assert_eq!(env["data"]["winner"]["title"], "Project Code Review");

        // Combining --why with --offset or combining --key with --id fails closed before any RPC.
        for bad_args in [
            vec![
                "process",
                "inspect",
                "--key",
                "code-review",
                "--why",
                "--offset",
                "10",
            ],
            vec![
                "process",
                "inspect",
                "--key",
                "code-review",
                "--id",
                "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071bb",
            ],
        ] {
            let owned: Vec<String> = bad_args.into_iter().map(str::to_owned).collect();
            let out_bad = crate::execute_with_office("agent-os", &owned, &mut office);
            assert_eq!(out_bad.exit_code, 1, "case {owned:?}: {}", out_bad.stdout);
        }
    }

    #[test]
    fn process_inspect_offset_paginates_chunked_doc_and_preserves_parent() {
        const CHUNK_LIMIT: usize = 512 * 1024;
        let full_body = format!("{}END", "a".repeat(600_000));
        let total_len = full_body.len();

        struct ChunkedTransport {
            body: String,
            seen_offsets: Vec<Option<usize>>,
        }
        impl OfficeTransport for ChunkedTransport {
            fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
                assert_eq!(method, "process.show");
                let parsed: ProcessShowParams = serde_json::from_value(params).unwrap();
                self.seen_offsets.push(parsed.offset);
                let start = parsed.offset.unwrap_or(0).min(self.body.len());
                let end = (start + CHUNK_LIMIT).min(self.body.len());
                let next_offset = (end < self.body.len()).then_some(end);
                let mut parent = sample_doc("code-review");
                parent.id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607100".into();
                parent.level = "organization".into();
                let mut doc = sample_doc("code-review");
                doc.level = "project".into();
                Ok(serde_json::to_value(ProcessShowResult {
                    document: doc,
                    blob: "sha256:chunked".into(),
                    content: self.body[start..end].to_owned(),
                    total_bytes: self.body.len(),
                    next_offset,
                    parent: Some(parent),
                })
                .unwrap())
            }
        }

        let mut office = crate::transport::TransportOffice(ChunkedTransport {
            body: full_body.clone(),
            seen_offsets: vec![],
        });

        let p1_args: Vec<String> = vec!["process", "inspect", "--key", "code-review"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let out1 = crate::execute_with_office("agent-os", &p1_args, &mut office);
        assert_eq!(out1.exit_code, 0, "{}", out1.stdout);
        let env1: Value = serde_json::from_str(&out1.stdout).unwrap();
        assert_eq!(env1["data"]["total_bytes"], total_len);
        assert_eq!(env1["data"]["next_offset"], CHUNK_LIMIT);
        assert_eq!(
            env1["data"]["parent"]["id"],
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f607100"
        );
        let chunk1 = env1["data"]["content"].as_str().unwrap();
        assert_eq!(chunk1.len(), CHUNK_LIMIT);

        let next_str = CHUNK_LIMIT.to_string();
        let p2_args: Vec<String> = vec![
            "process",
            "inspect",
            "--key",
            "code-review",
            "--offset",
            &next_str,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let out2 = crate::execute_with_office("agent-os", &p2_args, &mut office);
        assert_eq!(out2.exit_code, 0, "{}", out2.stdout);
        let env2: Value = serde_json::from_str(&out2.stdout).unwrap();
        assert_eq!(env2["data"]["total_bytes"], total_len);
        assert!(env2["data"].get("next_offset").is_none());
        let chunk2 = env2["data"]["content"].as_str().unwrap();
        assert_eq!(format!("{chunk1}{chunk2}"), full_body);
        assert_eq!(office.0.seen_offsets, vec![None, Some(CHUNK_LIMIT)]);

        // Negative --offset fails closed with zero RPC calls.
        let neg_args: Vec<String> = vec![
            "process",
            "inspect",
            "--key",
            "code-review",
            "--offset",
            "-1",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let out_neg = crate::execute_with_office("agent-os", &neg_args, &mut office);
        assert_eq!(out_neg.exit_code, 1);
        assert_eq!(office.0.seen_offsets.len(), 2);
    }
}
