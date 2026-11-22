//! Engine-independent projector (`ProcessFiles`) for `structure_documents` (`doc-75`, `doc-34`).

use std::collections::BTreeSet;

use org_store::{DurablePath, Engine, Id, ProjectionError, Projector};

use super::ports::{ProcessesRead, ProcessesWrite};
use super::records::{self, StructureDocPath};
use crate::organization::ports::OrganizationRead;
use crate::projects::ports::ProjectsRead;

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
    E::Tx: ProcessesWrite + ProjectsRead,
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
        let teams: BTreeSet<Id> = tx.teams()?.into_iter().map(|t| t.id).collect();
        let projects: BTreeSet<Id> = tx.projects()?.into_iter().map(|p| p.id).collect();
        let employees: BTreeSet<Id> = tx.employees()?.into_iter().map(|e| e.id).collect();
        let mut seen_keys: BTreeSet<(Option<Id>, Option<Id>, String)> = BTreeSet::new();

        for doc in &docs {
            let err_path = records::structure_document_path(
                doc.kind,
                &doc.id,
                doc.team_id.as_ref().map(std::slice::from_ref),
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
                if !teams.contains(tid) {
                    return Err(refuse(&err_path, "referenced team_id does not exist"));
                }
            }
            if let Some(pid) = &doc.project_id {
                if !projects.contains(pid) {
                    return Err(refuse(&err_path, "referenced project_id does not exist"));
                }
            }
            if let Some(oid) = &doc.owner_employee_id {
                if !employees.contains(oid) {
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
        }
        Ok(())
    }
}
