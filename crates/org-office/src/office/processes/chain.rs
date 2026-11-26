//! Team chains, scope names and the single-chain target a process request resolves to.

use std::collections::BTreeMap;

use org_process::{DocumentKind, Scope};
use org_proto::processes::{DocumentKindWire, ProcessDocumentWire, ProcessForWire};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::organization::records::{Employee, Team};
use crate::processes::records::StructureDocument;
use crate::projects::records::Project;
use crate::store::open::process_ops::ProcessSnapshot;

pub(super) fn build_team_chains(teams: &[Team]) -> BTreeMap<Id, Vec<Id>> {
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

pub(super) fn build_scope_names(teams: &[Team], projects: &[Project]) -> BTreeMap<Scope, String> {
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

pub(super) fn document_own_chain(
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

pub(super) fn find_parent_for_doc(
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

pub(super) fn find_team_id(teams: &[Team], raw: &str) -> Result<Id, ApiError> {
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

pub(super) fn resolve_chain_target(
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

pub(super) fn kind_matches(filter: Option<DocumentKindWire>, kind: DocumentKind) -> bool {
    match filter {
        None => true,
        Some(DocumentKindWire::Process) => kind == DocumentKind::Process,
        Some(DocumentKindWire::Standard) => kind == DocumentKind::Standard,
        Some(DocumentKindWire::Unknown) => false,
    }
}
