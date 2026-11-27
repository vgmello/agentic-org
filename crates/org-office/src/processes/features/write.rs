//! Domain write decision for durable `structure_documents` Markdown files (`process.write`,
//! `TASK-44`, `C5`, `doc-34`, `doc-75`).

use std::collections::BTreeMap;

use org_process::{
    validate_write, CallerKind, DocumentKind, DocumentStatus, Scope, WriteCheckInput,
    WriteViolation,
};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Actor, Change, Id, Operation, Refusal, Sha256};
use serde_json::json;

use crate::error::{api_refusal, decode_api_refusal, parse_uuid_v7_refusal, storage_api_refusal};
use crate::organization::authority::require_active_employee;
use crate::organization::records::{By, Employee, EmployeeKind, EmployeeStatus, Meta, Team};
use crate::processes::ports::{KnownProjects, ProcessesRead};
use crate::processes::records::{
    encode_structure_document, structure_document_path, StructureDocument,
};
use crate::time::format_rfc3339_ms;

const INACTIVE_PROCESS_WRITER: &str = "only active employees may write processes";

fn storage_refusal(err: impl std::fmt::Display) -> Refusal {
    storage_api_refusal("process storage error", err)
}

fn wire_kind(k: DocumentKindWire) -> Result<DocumentKind, Refusal> {
    match k {
        DocumentKindWire::Process => Ok(DocumentKind::Process),
        DocumentKindWire::Standard => Ok(DocumentKind::Standard),
        DocumentKindWire::Unknown => Err(api_refusal(
            ErrorCode::InvalidRequest,
            "kind must be `process` or `standard`",
        )),
    }
}

fn wire_status(s: DocumentStatusWire) -> Result<DocumentStatus, Refusal> {
    match s {
        DocumentStatusWire::Draft => Ok(DocumentStatus::Draft),
        DocumentStatusWire::Current => Ok(DocumentStatus::Current),
        DocumentStatusWire::Deprecated => Ok(DocumentStatus::Deprecated),
        DocumentStatusWire::Unknown => Err(api_refusal(
            ErrorCode::InvalidRequest,
            "status must be `draft`, `current`, or `deprecated`",
        )),
    }
}

fn violation_refusal(v: WriteViolation) -> Refusal {
    match v {
        WriteViolation::InvalidField { field, message } => {
            api_refusal(ErrorCode::InvalidRequest, format!("{field}: {message}"))
        }
        WriteViolation::AlreadyExists { key } => api_refusal(
            ErrorCode::AlreadyExists,
            format!("key `{key}` is already used by another document at this level and scope"),
        ),
        WriteViolation::Sealed {
            key,
            sealed_by_scope,
        } => api_refusal(
            ErrorCode::InvalidRequest,
            format!(
                "sealed: key `{key}` is marked non-overridable at {} scope",
                sealed_by_scope.level_str()
            ),
        ),
        WriteViolation::NotAuthorized { reason } => api_refusal(ErrorCode::NotAuthorized, reason),
    }
}

fn derive_doc_uuid_v7(command_id: &str, now_ms: u64) -> Id {
    let digest = Sha256::of_parts(&[b"structure-doc-id\0", command_id.as_bytes()]);
    let hash = digest.as_bytes();
    let mut b = [0u8; 16];
    let ts = now_ms.to_be_bytes();
    b[0..6].copy_from_slice(&ts[2..8]);
    b[6] = (hash[6] & 0x0f) | 0x70;
    b[7] = hash[7];
    b[8] = (hash[8] & 0x3f) | 0x80;
    b[9..16].copy_from_slice(&hash[9..16]);
    let s = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    );
    Id::parse(&s).expect("derived canonical UUIDv7")
}

struct ResolvedTargetScope {
    scope: Scope,
    team_id: Option<Id>,
    team_chain: Option<Vec<Id>>,
    project_id: Option<Id>,
    project_owner_team_id: Option<String>,
    ancestor_team_chain: Vec<String>,
}

fn resolve_team_ref<S: ProcessesRead>(read: &S, raw: &str) -> Result<Team, Refusal> {
    let trimmed = raw.trim();
    if let Ok(tid) = Id::parse(trimmed) {
        if let Some(team) = read.team(&tid).map_err(storage_refusal)? {
            return Ok(team);
        }
    }
    let key_rest = trimmed
        .strip_prefix("TEAM-")
        .or_else(|| trimmed.strip_prefix("team-"));
    if let Some(num) = key_rest
        .filter(|r| !r.is_empty() && !r.starts_with('0') && r.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|r| r.parse::<u64>().ok())
    {
        if let Some(team) = read
            .teams()
            .map_err(storage_refusal)?
            .into_iter()
            .find(|t| t.number == num)
        {
            return Ok(team);
        }
    }
    Err(api_refusal(
        ErrorCode::NotFound,
        format!("team `{raw}` not found"),
    ))
}

fn resolve_target_scope<S: ProcessesRead + KnownProjects>(
    read: &S,
    wire: &ProcessScopeWire,
) -> Result<ResolvedTargetScope, Refusal> {
    match wire {
        ProcessScopeWire::Organization => Ok(ResolvedTargetScope {
            scope: Scope::Organization,
            team_id: None,
            team_chain: None,
            project_id: None,
            project_owner_team_id: None,
            ancestor_team_chain: Vec::new(),
        }),
        ProcessScopeWire::Team { team_id } => {
            let team = resolve_team_ref(read, team_id)?;
            if !team.is_active() {
                return Err(api_refusal(
                    ErrorCode::InvalidRequest,
                    format!("cannot write processes for archived team `{team_id}`"),
                ));
            }
            let tid = team.id;
            let chain = read
                .team_chain(&tid)
                .map_err(storage_refusal)?
                .ok_or_else(|| {
                    api_refusal(ErrorCode::NotFound, format!("team `{team_id}` not found"))
                })?;
            let ancestors = chain
                .iter()
                .take(chain.len().saturating_sub(1))
                .map(|id| id.as_str().to_owned())
                .collect();
            Ok(ResolvedTargetScope {
                scope: Scope::Team(tid.as_str().to_owned()),
                team_id: Some(tid),
                team_chain: Some(chain),
                project_id: None,
                project_owner_team_id: None,
                ancestor_team_chain: ancestors,
            })
        }
        ProcessScopeWire::Project { project_id } => {
            let project = read
                .resolve_process_project(project_id)
                .map_err(storage_refusal)?
                .ok_or_else(|| {
                    api_refusal(
                        ErrorCode::NotFound,
                        format!("project `{project_id}` not found"),
                    )
                })?;
            if project.archived {
                return Err(api_refusal(
                    ErrorCode::InvalidRequest,
                    format!("cannot write processes for archived project `{project_id}`"),
                ));
            }
            let owner_chain = read
                .team_chain(&project.owner_team_id)
                .map_err(storage_refusal)?
                .unwrap_or_else(|| vec![project.owner_team_id.clone()]);
            let ancestors = owner_chain
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect();
            Ok(ResolvedTargetScope {
                scope: Scope::Project(project.id.as_str().to_owned()),
                team_id: None,
                team_chain: None,
                project_id: Some(project.id.clone()),
                project_owner_team_id: Some(project.owner_team_id.as_str().to_owned()),
                ancestor_team_chain: ancestors,
            })
        }
    }
}

fn doc_path_from_teams(
    doc: &StructureDocument,
    team_chains: &BTreeMap<Id, Vec<Id>>,
) -> Result<org_store::DurablePath, Refusal> {
    let chain = doc
        .team_id
        .as_ref()
        .and_then(|tid| team_chains.get(tid))
        .map(Vec::as_slice);
    structure_document_path(doc.kind, &doc.id, chain, doc.project_id.as_ref()).map_err(|_| {
        api_refusal(
            ErrorCode::InvalidRequest,
            "structure document path exceeds maximum team nesting depth",
        )
    })
}

fn resolve_owner_employee<S: ProcessesRead>(
    read: &S,
    raw_owner: Option<&str>,
) -> Result<Option<Id>, Refusal> {
    let Some(raw) = raw_owner.filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let oid = parse_uuid_v7_refusal(raw, "owner_employee_id")?;
    let emp = read
        .employee(&oid)
        .map_err(storage_refusal)?
        .ok_or_else(|| {
            api_refusal(
                ErrorCode::InvalidRequest,
                format!("owner_employee_id `{raw}` does not exist"),
            )
        })?;
    if emp.status != EmployeeStatus::Active {
        return Err(api_refusal(
            ErrorCode::InvalidRequest,
            "owner_employee_id must be an active employee",
        ));
    }
    Ok(Some(oid))
}

fn resolve_target_doc_id(
    params: &ProcessWriteParams,
    existing_docs: &[StructureDocument],
    now_ms: u64,
) -> Result<(Id, Option<StructureDocument>), Refusal> {
    match &params.document_id {
        Some(raw_id) => {
            let parsed = parse_uuid_v7_refusal(raw_id, "document_id")?;
            let found = existing_docs.iter().find(|d| d.id == parsed).cloned();
            match (&found, params.expected_revision) {
                (None, Some(_)) => Err(api_refusal(
                    ErrorCode::NotFound,
                    format!("structure document `{raw_id}` not found"),
                )),
                (Some(ex), None) => Err(api_refusal(
                    ErrorCode::Conflict,
                    format!(
                        "document `{}` already exists at revision {}; pass expected_revision to update",
                        ex.id.as_str(),
                        ex.meta.revision
                    ),
                )),
                (Some(ex), Some(exp)) if ex.meta.revision != exp => Err(api_refusal(
                    ErrorCode::Conflict,
                    format!(
                        "expected revision {exp}, but document is at revision {}",
                        ex.meta.revision
                    ),
                )),
                _ => Ok((parsed, found)),
            }
        }
        None => {
            if params.expected_revision.is_some() {
                return Err(api_refusal(
                    ErrorCode::InvalidRequest,
                    "expected_revision requires document_id",
                ));
            }
            Ok((derive_doc_uuid_v7(&params.command_id, now_ms), None))
        }
    }
}

fn build_caller_kind<S: ProcessesRead>(
    read: &S,
    caller: &Employee,
    owner_id: &str,
) -> Result<CallerKind, Refusal> {
    match caller.kind {
        EmployeeKind::Person => Ok(CallerKind::Person {
            is_owner: caller.id.as_str() == owner_id,
        }),
        EmployeeKind::Agent => {
            let chain = read
                .team_chain(&caller.home_team_id)
                .map_err(storage_refusal)?
                .unwrap_or_else(|| vec![caller.home_team_id.clone()]);
            Ok(CallerKind::Agent {
                home_team_id: caller.home_team_id.as_str().to_owned(),
                home_team_chain: chain.iter().map(|id| id.as_str().to_owned()).collect(),
            })
        }
    }
}

pub fn build_write_operation(
    actor: &str,
    params: &ProcessWriteParams,
) -> Result<Operation, ApiError> {
    let op_id = parse_uuid_v7_refusal(&params.command_id, "command_id")
        .map_err(|r| decode_api_refusal(&r, "invalid process.write command_id"))?;
    let actor_val = Id::parse(actor)
        .map(Actor::Employee)
        .unwrap_or(Actor::System);
    let fp_bytes = serde_json::to_vec(&json!({
        "document_id": params.document_id,
        "kind": params.kind,
        "scope": params.scope,
        "key": params.key,
        "title": params.title,
        "status": params.status,
        "owner_employee_id": params.owner_employee_id,
        "overridable": params.overridable,
        "body_markdown": params.body_markdown,
        "expected_revision": params.expected_revision,
    }))
    .expect("fingerprint encodes");
    Ok(Operation {
        id: op_id,
        kind: "process.write",
        actor: actor_val,
        fingerprint: Sha256::of_parts(&[b"process.write\0", &fp_bytes]),
    })
}

fn collect_team_chains<S: ProcessesRead>(view: &S) -> Result<BTreeMap<Id, Vec<Id>>, Refusal> {
    let mut team_chains = BTreeMap::new();
    for team in view.teams().map_err(storage_refusal)? {
        if let Ok(Some(ch)) = view.team_chain(&team.id) {
            team_chains.insert(team.id, ch);
        }
    }
    Ok(team_chains)
}

fn build_write_meta(existing: Option<&StructureDocument>, caller_id: &Id, now_str: String) -> Meta {
    match existing {
        Some(ex) => Meta {
            revision: ex.meta.revision + 1,
            created_at: ex.meta.created_at.clone(),
            created_by: ex.meta.created_by.clone(),
            updated_at: Some(now_str),
            updated_by: By::Employee(caller_id.clone()),
        },
        None => Meta {
            revision: 1,
            created_at: Some(now_str.clone()),
            created_by: By::Employee(caller_id.clone()),
            updated_at: Some(now_str),
            updated_by: By::Employee(caller_id.clone()),
        },
    }
}

pub fn decide_structure_write<S: ProcessesRead + KnownProjects>(
    view: &S,
    now_ms: u64,
    actor: &str,
    owner_id: &str,
    params: &ProcessWriteParams,
) -> Result<Change, Refusal> {
    let caller = require_active_employee(view, actor, INACTIVE_PROCESS_WRITER)?;
    let kind = wire_kind(params.kind)?;
    let status = wire_status(params.status)?;
    let target = resolve_target_scope(view, &params.scope)?;
    let caller_kind = build_caller_kind(view, &caller, owner_id)?;

    let existing_docs = view.structure_documents().map_err(storage_refusal)?;
    let team_chains = collect_team_chains(view)?;
    let (doc_id, existing_opt) = resolve_target_doc_id(params, &existing_docs, now_ms)?;
    if existing_opt
        .as_ref()
        .is_some_and(|ex| ex.scope() != target.scope)
    {
        return Err(api_refusal(
            ErrorCode::InvalidRequest,
            "cannot change a document's scope in place; create a new document in the target scope",
        ));
    }

    let mut owner_emp_id = resolve_owner_employee(view, params.owner_employee_id.as_deref())?;
    if owner_emp_id.is_none() && status == DocumentStatus::Current {
        owner_emp_id = existing_opt
            .as_ref()
            .and_then(|ex| ex.owner_employee_id.clone())
            .or_else(|| Some(caller.id.clone()));
    }

    let existing_inputs = existing_docs
        .iter()
        .map(|d| Ok(d.to_process_input(doc_path_from_teams(d, &team_chains)?.as_str().to_owned())))
        .collect::<Result<Vec<_>, Refusal>>()?;

    let validated_key = validate_write(&WriteCheckInput {
        caller: caller_kind,
        target_scope: target.scope.clone(),
        existing_document_id: existing_opt.as_ref().map(|d| d.id.as_str()),
        key: params.key.as_deref(),
        kind,
        title: &params.title,
        status,
        owner_employee_id: owner_emp_id.as_ref().map(Id::as_str),
        overridable: params.overridable,
        body_markdown: &params.body_markdown,
        project_owner_team_id: target.project_owner_team_id.as_deref(),
        ancestor_team_chain: &target.ancestor_team_chain,
        existing_docs: &existing_inputs,
    })
    .map_err(violation_refusal)?;

    let record = StructureDocument {
        id: doc_id,
        key: validated_key.map(|k| k.as_str().to_owned()),
        kind,
        title: params.title.trim().to_owned(),
        status,
        owner_employee_id: owner_emp_id,
        team_id: target.team_id,
        project_id: target.project_id,
        overridable: params.overridable,
        body_markdown: params.body_markdown.clone(),
        meta: build_write_meta(existing_opt.as_ref(), &caller.id, format_rfc3339_ms(now_ms)),
    };
    let new_path = structure_document_path(
        record.kind,
        &record.id,
        target.team_chain.as_deref(),
        record.project_id.as_ref(),
    )
    .map_err(|_| {
        api_refusal(
            ErrorCode::InvalidRequest,
            "structure document path exceeds maximum team nesting depth",
        )
    })?;
    let mut change = Change::new(org_store::Ack::Durable);
    if let Some(ex) = &existing_opt {
        let old_path = doc_path_from_teams(ex, &team_chains)?;
        if old_path != new_path {
            change = change.remove(old_path);
        }
    }
    let reply_bytes = serde_json::to_vec(&json!({
        "document_id": record.id.as_str(),
        "revision": record.meta.revision,
    }))
    .expect("reply json encodes");
    Ok(change
        .put(new_path, encode_structure_document(&record))
        .reply(reply_bytes))
}
