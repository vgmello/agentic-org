//! High-level Office facade for organization processes, standards, single-chain resolution,
//! launch instruction rendering, and built-in product skills (`TASK-44`, `C5`, `doc-34`).

use std::collections::{BTreeMap, BTreeSet};

use org_process::{
    find_builtin_skill, list_builtin_skills, render_instructions, resolve_single_chain,
    summarize_body, DocumentKind, DocumentStatus, EffectiveEntry, Layer, LayerOutcome, RenderInput,
    Rendered, Scope, SingleChainInputs, WinnerKind, OVERRIDABLE_CAP_BYTES,
};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, EffectiveProcessEntryWire, ProcessDocumentWire,
    ProcessEffectiveParams, ProcessEffectiveResult, ProcessForWire, ProcessLayerWire,
    ProcessScopeWire, ProcessShowParams, ProcessShowResult, ProcessViewParams, ProcessViewResult,
    ProcessWhyParams, ProcessWhyResult, ProcessWriteParams, ProcessWriteResult, SkillListResult,
    SkillReadResult, SkillSummaryWire,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Sha256};

use crate::error::OfficeError;
use crate::organization::records::{By, Employee, Team};
use crate::processes::records::{
    encode_structure_document, structure_document_path, StructureDocument,
};
use crate::projects::records::Project;
use crate::store::open::process_ops::{
    LaunchProcessSnapshotRead, ProcessSnapshot, ProcessSnapshotRead, WriteStructureDocumentOp,
};
use crate::time::format_rfc3339_ms;

use super::{storage, Office};

/// Maximum content chunk returned by `process.show` (`doc-34` §6: 512 KiB).
const SHOW_CHUNK_MAX_BYTES: usize = 512 * 1024;

fn kind_to_wire(k: DocumentKind) -> DocumentKindWire {
    match k {
        DocumentKind::Process => DocumentKindWire::Process,
        DocumentKind::Standard => DocumentKindWire::Standard,
    }
}

fn status_to_wire(s: DocumentStatus) -> DocumentStatusWire {
    match s {
        DocumentStatus::Draft => DocumentStatusWire::Draft,
        DocumentStatus::Current => DocumentStatusWire::Current,
        DocumentStatus::Deprecated => DocumentStatusWire::Deprecated,
    }
}

fn by_parts(by: &By) -> (String, Option<String>) {
    match by {
        By::Employee(id) => ("employee".to_owned(), Some(id.as_str().to_owned())),
        By::System => ("system".to_owned(), None),
        By::Unknown => ("unknown".to_owned(), None),
    }
}

fn build_team_chains(teams: &[Team]) -> BTreeMap<Id, Vec<Id>> {
    let by_id: BTreeMap<&Id, &Team> = teams.iter().map(|t| (&t.id, t)).collect();
    let mut out = BTreeMap::new();
    for team in teams {
        let mut chain = Vec::new();
        let mut cursor = Some(&team.id);
        let mut guard = 0usize;
        while let Some(id) = cursor {
            if guard >= 32 {
                break;
            }
            guard += 1;
            chain.push(id.clone());
            cursor = by_id.get(id).and_then(|t| t.parent_team_id.as_ref());
        }
        chain.reverse();
        out.insert(team.id.clone(), chain);
    }
    out
}

fn build_scope_names(teams: &[Team], projects: &[Project]) -> BTreeMap<Scope, String> {
    let mut names = BTreeMap::new();
    names.insert(Scope::Organization, "Organization".to_owned());
    for t in teams {
        names.insert(Scope::Team(t.id.as_str().to_owned()), t.name.clone());
    }
    for p in projects {
        names.insert(Scope::Project(p.id.as_str().to_owned()), p.name.clone());
    }
    names
}

fn doc_path_str(doc: &StructureDocument, team_chains: &BTreeMap<Id, Vec<Id>>) -> String {
    let chain = doc
        .team_id
        .as_ref()
        .and_then(|tid| team_chains.get(tid))
        .map(Vec::as_slice);
    structure_document_path(doc.kind, &doc.id, chain, doc.project_id.as_ref())
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|_| format!("state/{}.md", doc.id.as_str()))
}

fn document_own_chain(
    snap: &ProcessSnapshot,
    team_chains: &BTreeMap<Id, Vec<Id>>,
    rec: &StructureDocument,
) -> (Vec<String>, Option<String>) {
    if let Some(pid) = &rec.project_id {
        let chain = snap
            .projects
            .iter()
            .find(|p| &p.id == pid)
            .and_then(|p| team_chains.get(&p.owner_team_id))
            .map(|c| c.iter().map(|id| id.as_str().to_owned()).collect())
            .unwrap_or_default();
        (chain, Some(pid.as_str().to_owned()))
    } else if let Some(tid) = &rec.team_id {
        let chain = team_chains
            .get(tid)
            .map(|c| c.iter().map(|id| id.as_str().to_owned()).collect())
            .unwrap_or_default();
        (chain, None)
    } else {
        (Vec::new(), None)
    }
}

fn find_parent_for_doc(
    set: &org_process::EffectiveSet,
    wire_by_id: &BTreeMap<String, ProcessDocumentWire>,
    rec: &StructureDocument,
) -> Option<ProcessDocumentWire> {
    let key = rec.key.as_deref()?;
    let entry = set.find_by_key(key)?;
    if let Some(w) = &entry.winner {
        if w.doc.id == rec.id.as_str() {
            return w
                .replaces
                .as_ref()
                .and_then(|p| wire_by_id.get(&p.id))
                .cloned();
        }
    }
    let pos = entry
        .layers
        .iter()
        .position(|l| l.doc.id == rec.id.as_str())?;
    entry.layers[..pos]
        .iter()
        .rev()
        .find(|l| {
            matches!(
                l.outcome,
                org_process::LayerOutcome::Applies | org_process::LayerOutcome::Replaced
            )
        })
        .and_then(|l| wire_by_id.get(&l.doc.id))
        .cloned()
}

fn find_team_id(teams: &[Team], raw: &str) -> Result<Id, ApiError> {
    let trimmed = raw.trim();
    if let Ok(tid) = Id::parse(trimmed) {
        if teams.iter().any(|t| t.id == tid) {
            return Ok(tid);
        }
    }
    if let Some(rest) = trimmed
        .strip_prefix("TEAM-")
        .or_else(|| trimmed.strip_prefix("team-"))
    {
        if !rest.is_empty() && !rest.starts_with('0') && rest.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(num) = rest.parse::<u64>() {
                if let Some(team) = teams.iter().find(|t| t.number == num) {
                    return Ok(team.id.clone());
                }
            }
        }
    }
    Err(ApiError::new(
        ErrorCode::NotFound,
        format!("team `{raw}` not found"),
    ))
}

fn doc_to_wire(
    doc: &StructureDocument,
    team_chains: &BTreeMap<Id, Vec<Id>>,
    scope_names: &BTreeMap<Scope, String>,
    emp_names: &BTreeMap<String, String>,
) -> ProcessDocumentWire {
    let scope = doc.scope();
    let (cb_kind, cb_id) = by_parts(&doc.meta.created_by);
    let (ub_kind, ub_id) = by_parts(&doc.meta.updated_by);
    let owner_id_str = doc
        .owner_employee_id
        .as_ref()
        .map(|i| i.as_str().to_owned());
    let owner_name = owner_id_str
        .as_deref()
        .and_then(|id| emp_names.get(id))
        .cloned();
    let scope_name = match &scope {
        Scope::Organization => None,
        other => scope_names.get(other).cloned(),
    };
    ProcessDocumentWire {
        id: doc.id.as_str().to_owned(),
        key: doc.key.clone(),
        revision: doc.meta.revision,
        kind: kind_to_wire(doc.kind),
        title: doc.title.clone(),
        status: status_to_wire(doc.status),
        owner_employee_id: owner_id_str,
        owner_name,
        level: scope.level_str().to_owned(),
        team_id: doc.team_id.as_ref().map(|i| i.as_str().to_owned()),
        project_id: doc.project_id.as_ref().map(|i| i.as_str().to_owned()),
        scope_name,
        overridable: doc.overridable,
        summary: summarize_body(&doc.body_markdown),
        body_markdown: doc.body_markdown.clone(),
        path: doc_path_str(doc, team_chains),
        created_at: doc.meta.created_at.clone(),
        created_by_kind: cb_kind,
        created_by_id: cb_id,
        updated_at: doc.meta.updated_at.clone(),
        updated_by_kind: ub_kind,
        updated_by_id: ub_id,
    }
}

fn layer_to_wire(
    layer: &Layer,
    by_id: &BTreeMap<String, ProcessDocumentWire>,
) -> Option<ProcessLayerWire> {
    let document = by_id.get(&layer.doc.id)?.clone();
    let (outcome, detail) = match &layer.outcome {
        LayerOutcome::Applies => ("applies".to_owned(), None),
        LayerOutcome::Replaced => ("replaced".to_owned(), None),
        LayerOutcome::Refused {
            sealed_by_name,
            sealed_by_title,
            ..
        } => (
            "refused".to_owned(),
            Some(format!(
                "sealed (non-overridable) by {sealed_by_name}: {sealed_by_title}"
            )),
        ),
        LayerOutcome::Invalid { field } => (
            "invalid".to_owned(),
            Some(format!("invalid field `{field}`")),
        ),
        LayerOutcome::Clash { scope_name } => (
            "clash".to_owned(),
            Some(format!("duplicate key clash in {scope_name}")),
        ),
    };
    Some(ProcessLayerWire {
        document,
        outcome,
        detail,
    })
}

fn entry_to_wire(
    entry: &EffectiveEntry,
    by_id: &BTreeMap<String, ProcessDocumentWire>,
) -> Option<EffectiveProcessEntryWire> {
    let chain: Vec<ProcessLayerWire> = entry
        .layers
        .iter()
        .filter_map(|l| layer_to_wire(l, by_id))
        .collect();
    let (applies, mandatory, winner_kind, document, replaces) = match &entry.winner {
        Some(w) => {
            let doc = by_id.get(&w.doc.id)?.clone();
            let rep = w.replaces.as_ref().and_then(|r| by_id.get(&r.id)).cloned();
            let wk = match w.kind {
                WinnerKind::Inherited => "inherited",
                WinnerKind::Override => "override",
                WinnerKind::Addition => "addition",
            };
            (true, !w.doc.overridable, Some(wk.to_owned()), doc, rep)
        }
        None => {
            let first = chain.first()?.document.clone();
            (false, false, None, first, None)
        }
    };
    let problem = chain.iter().find_map(|l| {
        (!matches!(l.outcome.as_str(), "applies" | "replaced"))
            .then(|| l.detail.clone())
            .flatten()
    });
    Some(EffectiveProcessEntryWire {
        id: entry.entry_id.clone(),
        key: entry.key.as_ref().map(|k| k.as_str().to_owned()),
        kind: kind_to_wire(entry.kind),
        applies,
        mandatory,
        winner_kind,
        document,
        replaces,
        problem,
        chain,
    })
}

fn resolve_chain_target(
    snap: &ProcessSnapshot,
    team_chains: &BTreeMap<Id, Vec<Id>>,
    employee_id: Option<&str>,
    target_for: Option<&ProcessForWire>,
) -> Result<(Vec<String>, Option<String>), ApiError> {
    match target_for {
        Some(ProcessForWire::Task { .. }) => {
            let task = snap.task.as_ref().ok_or_else(|| {
                ApiError::new(ErrorCode::NotFound, "requested task was not found")
            })?;
            if let Some(pid) = &task.project_id {
                let proj = snap.projects.iter().find(|p| p.id == *pid).ok_or_else(|| {
                    ApiError::new(ErrorCode::NotFound, "task's project not found")
                })?;
                let chain = team_chains
                    .get(&proj.owner_team_id)
                    .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                    .unwrap_or_else(|| vec![proj.owner_team_id.as_str().to_owned()]);
                return Ok((chain, Some(proj.id.as_str().to_owned())));
            }
            // Projectless work resolves against the task's accountable `team_id` (`doc-34` §3.1).
            let acct_team = &task.team_id;
            let chain = team_chains
                .get(acct_team)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![acct_team.as_str().to_owned()]);
            Ok((chain, None))
        }
        Some(ProcessForWire::Project { project_id }) => {
            let proj = snap
                .projects
                .iter()
                .find(|p| {
                    p.id.as_str() == project_id
                        || p.key().eq_ignore_ascii_case(project_id)
                        || p.slug.eq_ignore_ascii_case(project_id)
                })
                .ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::NotFound,
                        format!("project `{project_id}` not found"),
                    )
                })?;
            let chain = team_chains
                .get(&proj.owner_team_id)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![proj.owner_team_id.as_str().to_owned()]);
            Ok((chain, Some(proj.id.as_str().to_owned())))
        }
        Some(ProcessForWire::Team { team_id }) => {
            let tid = find_team_id(&snap.teams, team_id)?;
            let chain = team_chains
                .get(&tid)
                .ok_or_else(|| {
                    ApiError::new(ErrorCode::NotFound, format!("team `{team_id}` not found"))
                })?
                .iter()
                .map(|i| i.as_str().to_owned())
                .collect();
            Ok((chain, None))
        }
        None | Some(ProcessForWire::Current) => {
            let target_emp: &Employee = match employee_id {
                Some(eid) => snap
                    .employees
                    .iter()
                    .find(|e| e.id.as_str() == eid || e.handle.eq_ignore_ascii_case(eid))
                    .ok_or_else(|| {
                        ApiError::new(ErrorCode::NotFound, format!("employee `{eid}` not found"))
                    })?,
                None => &snap.caller,
            };
            let chain = team_chains
                .get(&target_emp.home_team_id)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![target_emp.home_team_id.as_str().to_owned()]);
            Ok((chain, None))
        }
    }
}

fn kind_matches(filter: Option<DocumentKindWire>, kind: DocumentKind) -> bool {
    match filter {
        None => true,
        Some(DocumentKindWire::Process) => kind == DocumentKind::Process,
        Some(DocumentKindWire::Standard) => kind == DocumentKind::Standard,
        Some(DocumentKindWire::Unknown) => false,
    }
}

fn slice_utf8_chunk(text: &str, offset: usize) -> (String, Option<usize>) {
    let total = text.len();
    if offset >= total {
        return (String::new(), None);
    }
    let start = text.floor_char_boundary(offset);
    let max_end = (start + SHOW_CHUNK_MAX_BYTES).min(total);
    let end = text.floor_char_boundary(max_end);
    let next = (end < total).then_some(end);
    (text[start..end].to_owned(), next)
}

impl Office {
    fn compute_effective_from_snapshot(
        snap: &ProcessSnapshot,
        team_chain: Vec<String>,
        project_id: Option<String>,
    ) -> (
        org_process::EffectiveSet,
        BTreeMap<String, ProcessDocumentWire>,
        BTreeMap<Scope, String>,
    ) {
        let team_chains = build_team_chains(&snap.teams);
        let scope_names = build_scope_names(&snap.teams, &snap.projects);
        let emp_names: BTreeMap<String, String> = snap
            .employees
            .iter()
            .map(|e| (e.id.as_str().to_owned(), e.name.clone()))
            .collect();
        let wire_by_id: BTreeMap<String, ProcessDocumentWire> = snap
            .documents
            .iter()
            .map(|d| {
                (
                    d.id.as_str().to_owned(),
                    doc_to_wire(d, &team_chains, &scope_names, &emp_names),
                )
            })
            .collect();
        let inputs = SingleChainInputs {
            team_chain,
            project_id,
            documents: snap
                .documents
                .iter()
                .map(|d| d.to_process_input(doc_path_str(d, &team_chains)))
                .collect(),
            scope_names: scope_names.clone(),
        };
        let set = resolve_single_chain(&inputs);
        (set, wire_by_id, scope_names)
    }

    /// `process.effective` (`doc-34` §6): resolves single-chain processes & standards.
    pub fn process_effective(
        &self,
        caller: &str,
        params: ProcessEffectiveParams,
    ) -> Result<Result<ProcessEffectiveResult, ApiError>, OfficeError> {
        let task_ref = match &params.r#for {
            Some(ProcessForWire::Task { task_id }) => Some(task_id.clone()),
            _ => None,
        };
        let snap = match self
            .store
            .read(ProcessSnapshotRead {
                caller_id: caller.to_owned(),
                task_ref,
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let (team_chain, project_id) = match resolve_chain_target(
            &snap,
            &team_chains,
            params.employee_id.as_deref(),
            params.r#for.as_ref(),
        ) {
            Ok(pair) => pair,
            Err(e) => return Ok(Err(e)),
        };
        let (set, wire_by_id, scope_names) =
            Self::compute_effective_from_snapshot(&snap, team_chain, project_id);
        let scope_chain = set
            .chain
            .iter()
            .map(|s| {
                scope_names
                    .get(s)
                    .cloned()
                    .unwrap_or_else(|| s.level_str().to_owned())
            })
            .collect();
        let entries = set
            .entries
            .iter()
            .filter(|e| kind_matches(params.kind, e.kind))
            .filter_map(|e| entry_to_wire(e, &wire_by_id))
            .collect();
        let problems = set
            .problems
            .iter()
            .filter_map(|l| layer_to_wire(l, &wire_by_id))
            .collect();
        Ok(Ok(ProcessEffectiveResult {
            scope_chain,
            entries,
            problems,
            computed_at: format_rfc3339_ms(self.clock.now().0),
            stale_since: None,
        }))
    }

    /// `process.why` (`doc-34` §6, `PROC-8`): explains single-chain resolution for one key.
    pub fn process_why(
        &self,
        caller: &str,
        params: ProcessWhyParams,
    ) -> Result<Result<ProcessWhyResult, ApiError>, OfficeError> {
        let task_ref = match &params.r#for {
            Some(ProcessForWire::Task { task_id }) => Some(task_id.clone()),
            _ => None,
        };
        let snap = match self
            .store
            .read(ProcessSnapshotRead {
                caller_id: caller.to_owned(),
                task_ref,
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let (team_chain, project_id) = match resolve_chain_target(
            &snap,
            &team_chains,
            params.employee_id.as_deref(),
            params.r#for.as_ref(),
        ) {
            Ok(pair) => pair,
            Err(e) => return Ok(Err(e)),
        };
        let (set, wire_by_id, _) =
            Self::compute_effective_from_snapshot(&snap, team_chain, project_id);
        let Some(entry) = set.find_by_key(&params.key) else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                format!("no process or standard found for key `{}`", params.key),
            )));
        };
        let chain = entry
            .layers
            .iter()
            .filter_map(|l| layer_to_wire(l, &wire_by_id))
            .collect();
        let winner = entry
            .winner
            .as_ref()
            .and_then(|w| wire_by_id.get(&w.doc.id))
            .cloned();
        Ok(Ok(ProcessWhyResult {
            key: params.key,
            chain,
            winner,
        }))
    }

    /// `process.show` (`doc-34` §6): returns a document's full text (chunked <= 512 KiB) and
    fn show_document_by_id<'a>(
        snap: &'a ProcessSnapshot,
        team_chains: &BTreeMap<Id, Vec<Id>>,
        params: &ProcessShowParams,
        doc_id: &str,
    ) -> Result<
        (
            &'a StructureDocument,
            ProcessDocumentWire,
            Option<ProcessDocumentWire>,
        ),
        ApiError,
    > {
        let rec = snap
            .documents
            .iter()
            .find(|d| d.id.as_str() == doc_id)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::NotFound,
                    format!("structure document `{doc_id}` not found"),
                )
            })?;
        let (doc_chain, doc_proj) = if params.r#for.is_some() {
            resolve_chain_target(
                snap,
                team_chains,
                params.employee_id.as_deref(),
                params.r#for.as_ref(),
            )?
        } else {
            document_own_chain(snap, team_chains, rec)
        };
        let (set, wire_by_id, _) = Self::compute_effective_from_snapshot(snap, doc_chain, doc_proj);
        let parent_doc = find_parent_for_doc(&set, &wire_by_id, rec);
        let wire = wire_by_id
            .get(rec.id.as_str())
            .expect("wire exists")
            .clone();
        Ok((rec, wire, parent_doc))
    }

    fn show_document_by_key<'a>(
        snap: &'a ProcessSnapshot,
        team_chains: &BTreeMap<Id, Vec<Id>>,
        params: &ProcessShowParams,
    ) -> Result<
        (
            &'a StructureDocument,
            ProcessDocumentWire,
            Option<ProcessDocumentWire>,
        ),
        ApiError,
    > {
        let (team_chain, project_id) = resolve_chain_target(
            snap,
            team_chains,
            params.employee_id.as_deref(),
            params.r#for.as_ref(),
        )?;
        let (set, wire_by_id, _) =
            Self::compute_effective_from_snapshot(snap, team_chain, project_id);
        let key = params.key.as_deref().unwrap_or("");
        let winner = set.winner_by_key_or_id(key).ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("no active process or standard matches `{key}`"),
            )
        })?;
        let rec = snap
            .documents
            .iter()
            .find(|d| d.id.as_str() == winner.doc.id)
            .expect("winner exists in snapshot");
        let parent_doc = winner
            .replaces
            .as_ref()
            .and_then(|p| wire_by_id.get(&p.id))
            .cloned();
        let wire = wire_by_id
            .get(rec.id.as_str())
            .expect("wire exists")
            .clone();
        Ok((rec, wire, parent_doc))
    }

    /// `process.show` (`doc-34` §6, `PROC-6`): returns the document, body chunk, and
    /// optional parent document when overriding a higher scope.
    pub fn process_show(
        &self,
        caller: &str,
        params: ProcessShowParams,
    ) -> Result<Result<ProcessShowResult, ApiError>, OfficeError> {
        if params.key.is_none() && params.document_id.is_none() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "provide either `key` or `document_id`",
            )));
        }
        let task_ref = match &params.r#for {
            Some(ProcessForWire::Task { task_id }) => Some(task_id.clone()),
            _ => None,
        };
        let snap = match self
            .store
            .read(ProcessSnapshotRead {
                caller_id: caller.to_owned(),
                task_ref,
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let resolved = match params.document_id.as_deref() {
            Some(doc_id) => Self::show_document_by_id(&snap, &team_chains, &params, doc_id),
            None => Self::show_document_by_key(&snap, &team_chains, &params),
        };
        let (doc_rec, wire, parent) = match resolved {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        };

        let encoded = encode_structure_document(doc_rec);
        let blob = Sha256::of(&encoded).to_hex();
        let total_bytes = doc_rec.body_markdown.len();
        let (content, next_offset) =
            slice_utf8_chunk(&doc_rec.body_markdown, params.offset.unwrap_or(0));
        Ok(Ok(ProcessShowResult {
            document: wire,
            blob,
            content,
            total_bytes,
            next_offset,
            parent,
        }))
    }

    /// `process.view` (`doc-34` §6, `PROC-9`, `PROC-10`): scoped catalog view for Settings > Processes.
    pub fn process_view(
        &self,
        caller: &str,
        params: ProcessViewParams,
    ) -> Result<Result<ProcessViewResult, ApiError>, OfficeError> {
        let snap = match self
            .store
            .read(ProcessSnapshotRead {
                caller_id: caller.to_owned(),
                task_ref: None,
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let (target_scope, team_chain, project_id) = match &params.scope {
            ProcessScopeWire::Organization => (Scope::Organization, Vec::new(), None),
            ProcessScopeWire::Team { team_id } => {
                let tid = match find_team_id(&snap.teams, team_id) {
                    Ok(id) => id,
                    Err(e) => return Ok(Err(e)),
                };
                let Some(chain) = team_chains.get(&tid) else {
                    return Ok(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("team `{team_id}` not found"),
                    )));
                };
                (
                    Scope::Team(tid.as_str().to_owned()),
                    chain.iter().map(|i| i.as_str().to_owned()).collect(),
                    None,
                )
            }
            ProcessScopeWire::Project { project_id } => {
                let Some(proj) = snap.projects.iter().find(|p| {
                    p.id.as_str() == project_id
                        || p.key().eq_ignore_ascii_case(project_id)
                        || p.slug.eq_ignore_ascii_case(project_id)
                }) else {
                    return Ok(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("project `{project_id}` not found"),
                    )));
                };
                let chain = team_chains
                    .get(&proj.owner_team_id)
                    .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                    .unwrap_or_else(|| vec![proj.owner_team_id.as_str().to_owned()]);
                (
                    Scope::Project(proj.id.as_str().to_owned()),
                    chain,
                    Some(proj.id.as_str().to_owned()),
                )
            }
        };

        let (set, wire_by_id, scope_names) =
            Self::compute_effective_from_snapshot(&snap, team_chain, project_id);
        let scope_name = scope_names
            .get(&target_scope)
            .cloned()
            .unwrap_or_else(|| target_scope.level_str().to_owned());
        let rows = set
            .entries
            .iter()
            .filter(|e| kind_matches(params.kind, e.kind))
            .filter_map(|e| entry_to_wire(e, &wire_by_id))
            .collect();
        let inactive = snap
            .documents
            .iter()
            .filter(|d| {
                d.scope() == target_scope
                    && d.status != DocumentStatus::Current
                    && kind_matches(params.kind, d.kind)
            })
            .filter_map(|d| wire_by_id.get(d.id.as_str()).cloned())
            .collect();
        let problems = set
            .problems
            .iter()
            .filter_map(|l| layer_to_wire(l, &wire_by_id))
            .collect();

        Ok(Ok(ProcessViewResult {
            scope: params.scope,
            scope_name,
            rows,
            inactive,
            problems,
            computed_at: format_rfc3339_ms(self.clock.now().0),
            stale_since: None,
        }))
    }

    /// `process.write` (`doc-34` §5 & §6, `FC-112`, `FC-184`): creates or updates a process/standard.
    pub fn process_write(
        &self,
        caller: &str,
        params: ProcessWriteParams,
    ) -> Result<Result<ProcessWriteResult, ApiError>, OfficeError> {
        let (doc_id, revision) = match self
            .store
            .run(WriteStructureDocumentOp {
                actor: caller.to_owned(),
                owner_id: self.owner.as_str().to_owned(),
                params,
            })
            .map_err(storage)?
        {
            Ok(pair) => pair,
            Err(e) => return Ok(Err(e)),
        };

        let snap = match self
            .store
            .read(ProcessSnapshotRead {
                caller_id: caller.to_owned(),
                task_ref: None,
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let scope_names = build_scope_names(&snap.teams, &snap.projects);
        let emp_names: BTreeMap<String, String> = snap
            .employees
            .iter()
            .map(|e| (e.id.as_str().to_owned(), e.name.clone()))
            .collect();
        let rec = snap
            .documents
            .iter()
            .find(|d| d.id.as_str() == doc_id)
            .ok_or_else(|| {
                OfficeError::Invalid("written structure document missing from projection")
            })?;
        let document = doc_to_wire(rec, &team_chains, &scope_names, &emp_names);
        Ok(Ok(ProcessWriteResult {
            document_id: doc_id,
            revision,
            document,
        }))
    }

    /// Renders effective single-chain processes and standards for agent session launch (`doc-34` §3.2).
    /// Returns `Err(ApiError::InvalidRequest)` if non-overridable documents alone exceed `total_budget_bytes`.
    pub fn render_agent_process_instructions(
        &self,
        employee_id: &str,
        project_ref: Option<&str>,
        task_ref: Option<&str>,
        read_blobs: BTreeMap<String, BTreeSet<String>>,
        total_budget_bytes: usize,
    ) -> Result<Result<Rendered, ApiError>, OfficeError> {
        let snap = match self
            .store
            .read(LaunchProcessSnapshotRead {
                employee_id: employee_id.to_owned(),
                project_ref: project_ref.map(ToOwned::to_owned),
                task_ref: task_ref.map(ToOwned::to_owned),
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let scope_names = build_scope_names(&snap.teams, &snap.projects);

        let (team_chain, project_id) = if let Some(proj) = &snap.project {
            let chain = team_chains
                .get(&proj.owner_team_id)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![proj.owner_team_id.as_str().to_owned()]);
            (chain, Some(proj.id.as_str().to_owned()))
        } else if let Some(task) = &snap.task {
            // Projectless work resolves against the task's accountable `team_id` (`doc-34` §3.1).
            let acct = &task.team_id;
            let chain = team_chains
                .get(acct)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![acct.as_str().to_owned()]);
            (chain, None)
        } else {
            let chain = team_chains
                .get(&snap.employee.home_team_id)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![snap.employee.home_team_id.as_str().to_owned()]);
            (chain, None)
        };

        let set = resolve_single_chain(&SingleChainInputs {
            team_chain,
            project_id,
            documents: snap
                .documents
                .iter()
                .map(|d| d.to_process_input(doc_path_str(d, &team_chains)))
                .collect(),
            scope_names,
        });

        match render_instructions(
            &set,
            &RenderInput {
                read_blobs,
                overridable_cap_bytes: OVERRIDABLE_CAP_BYTES,
                total_budget_bytes,
            },
        ) {
            Ok(rendered) => Ok(Ok(rendered)),
            Err(err) => Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                err.to_string(),
            ))),
        }
    }

    /// `skill.list` (`doc-28` §3.5): lists built-in product skills.
    pub fn skill_list(&self) -> SkillListResult {
        SkillListResult {
            skills: list_builtin_skills()
                .iter()
                .map(|s| SkillSummaryWire {
                    key: s.key.to_owned(),
                    title: s.title.to_owned(),
                    description: s.description.to_owned(),
                    source: s.source.to_owned(),
                })
                .collect(),
        }
    }

    /// `skill.read` (`doc-28` §3.5): reads one built-in product skill by key.
    pub fn skill_read(&self, name: &str) -> Result<SkillReadResult, ApiError> {
        let Some(skill) = find_builtin_skill(name) else {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("skill `{name}` not found"),
            ));
        };
        Ok(SkillReadResult {
            key: skill.key.to_owned(),
            title: skill.title.to_owned(),
            description: skill.description.to_owned(),
            source: skill.source.to_owned(),
            body_markdown: skill.body_markdown.to_owned(),
        })
    }
}

#[cfg(test)]
#[path = "processes_tests.rs"]
mod tests;
