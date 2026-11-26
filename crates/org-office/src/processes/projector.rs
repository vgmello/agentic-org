//! Engine-independent projector (`ProcessFiles`) for `structure_documents` (`doc-75`, `doc-34`).

use std::collections::BTreeSet;

use org_store::{CoreRead, DurablePath, Engine, Id, ProjectionError, Projector};

use super::ports::{KnownProjects, ProcessesRead, ProcessesWrite};
use super::records::{self, StructureDocPath};
use crate::organization::ports::OrganizationRead;

/// Projects `state/{processes,standards}/`, `state/teams/.../{processes,standards}/`, and
/// `state/projects/<id>/{processes,standards}/` Markdown files into `structure_documents`.
pub struct ProcessFiles;

impl ProcessFiles {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProcessFiles {
    fn default() -> Self {
        Self::new()
    }
}

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn agree(path: &DurablePath, ok: bool, what: &str) -> Result<(), ProjectionError> {
    if ok {
        Ok(())
    } else {
        Err(refuse(path, format!("{what} does not match the path")))
    }
}

impl<E: Engine> Projector<E> for ProcessFiles
where
    E::Tx: ProcessesWrite + KnownProjects,
{
    fn owns(&self, path: &DurablePath) -> bool {
        records::parse_path(path).is_some()
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        let Some(parsed) = records::parse_path(path) else {
            return Err(refuse(path, "not a structure document file"));
        };
        let doc_id = match &parsed {
            StructureDocPath::Organization(_, id)
            | StructureDocPath::Team(_, _, id)
            | StructureDocPath::Project(_, _, id) => id.clone(),
        };
        let Some(bytes) = bytes else {
            tx.remove_structure_document(&doc_id)?;
            return Ok(());
        };
        let record =
            records::decode_structure_document(bytes).map_err(|e| refuse(path, e.to_string()))?;
        agree(path, record.id == doc_id, "id")?;
        match parsed {
            StructureDocPath::Organization(kind, _) => {
                agree(path, record.kind == kind, "kind")?;
                agree(path, record.team_id.is_none(), "team_id")?;
                agree(path, record.project_id.is_none(), "project_id")?;
            }
            StructureDocPath::Team(chain, kind, _) => {
                agree(path, record.kind == kind, "kind")?;
                agree(path, record.team_id.as_ref() == chain.last(), "team_id")?;
                agree(path, record.project_id.is_none(), "project_id")?;
            }
            StructureDocPath::Project(pid, kind, _) => {
                agree(path, record.kind == kind, "kind")?;
                agree(path, record.team_id.is_none(), "team_id")?;
                agree(path, record.project_id.as_ref() == Some(&pid), "project_id")?;
            }
        }
        tx.put_structure_document(&record)?;
        Ok(())
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        let docs = tx.structure_documents()?;
        let team_rows = tx.teams()?;
        let ctx = StructureValidationCtx {
            teams: team_rows.iter().map(|t| t.id.clone()).collect(),
            team_parents: team_rows
                .iter()
                .map(|t| (t.id.clone(), t.parent_team_id.clone()))
                .collect(),
            projects: tx.project_ids()?.into_iter().collect(),
            employees: tx.employees()?.into_iter().map(|e| e.id).collect(),
        };
        let mut seen_keys: BTreeSet<(Option<Id>, Option<Id>, String)> = BTreeSet::new();
        for doc in &docs {
            validate_single_structure_doc(tx, &ctx, doc, &mut seen_keys)?;
        }
        Ok(())
    }
}

struct StructureValidationCtx {
    teams: BTreeSet<Id>,
    team_parents: std::collections::BTreeMap<Id, Option<Id>>,
    projects: BTreeSet<Id>,
    employees: BTreeSet<Id>,
}

fn resolve_team_ancestry(
    team_id: &Id,
    team_parents: &std::collections::BTreeMap<Id, Option<Id>>,
) -> Vec<Id> {
    let mut chain = Vec::new();
    let mut cursor = Some(team_id.clone());
    let mut visited = BTreeSet::new();
    while let Some(cur) = cursor {
        if !visited.insert(cur.clone()) {
            break;
        }
        chain.push(cur.clone());
        cursor = team_parents.get(&cur).cloned().flatten();
    }
    chain.reverse();
    chain
}

fn validate_single_structure_doc<Tx>(
    tx: &Tx,
    ctx: &StructureValidationCtx,
    doc: &records::StructureDocument,
    seen_keys: &mut BTreeSet<(Option<Id>, Option<Id>, String)>,
) -> Result<(), ProjectionError>
where
    Tx: ProcessesRead + KnownProjects + CoreRead,
{
    let team_chain = doc
        .team_id
        .as_ref()
        .map(|tid| resolve_team_ancestry(tid, &ctx.team_parents));
    let err_path = records::structure_document_path(
        doc.kind,
        &doc.id,
        team_chain.as_deref(),
        doc.project_id.as_ref(),
    )
    .map_err(|e| {
        ProjectionError::new(
            &DurablePath::parse("state/processes").expect("static fallback path"),
            e.to_string(),
        )
    })?;
    if doc.team_id.is_some() && doc.project_id.is_some() {
        return Err(refuse(
            &err_path,
            "structure document cannot belong to both a team and a project",
        ));
    }
    if let Some(tid) = &doc.team_id {
        if !ctx.teams.contains(tid) {
            return Err(refuse(&err_path, "referenced team_id does not exist"));
        }
        // Verify the document file actually resides at the full canonical team ancestry path
        // (`state/teams/<root>/teams/<child>/...`), rejecting imports placed under a truncated
        // team path (`TASK-97` / Final Gate Finding #6).
        if tx.source_hash(&err_path)?.is_none() {
            return Err(refuse(
                &err_path,
                "team structure document is not stored at its canonical team ancestry path",
            ));
        }
    }
    if let Some(pid) = &doc.project_id {
        if !ctx.projects.contains(pid) {
            return Err(refuse(&err_path, "referenced project_id does not exist"));
        }
    }
    if let Some(oid) = &doc.owner_employee_id {
        if !ctx.employees.contains(oid) {
            return Err(refuse(
                &err_path,
                "referenced owner_employee_id does not exist",
            ));
        }
    }
    if let Some(key) = &doc.key {
        if !seen_keys.insert((doc.team_id.clone(), doc.project_id.clone(), key.clone())) {
            return Err(refuse(
                &err_path,
                format!("duplicate structure document key `{key}` in the same scope"),
            ));
        }
    }
    Ok(())
}
