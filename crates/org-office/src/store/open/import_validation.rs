//! Validation of imported organization `state/` files against the shared-state production
//! projectors on a disposable private SQLite projection (`TASK-39` / R1, `TASK-97`).

use org_store::{Id, Projector, StoreError};

use super::super::sqlite::SqliteEngine;
use super::OfficeEngine;
use crate::{
    organization::projector::OrganizationFiles, processes::projector::ProcessFiles,
    projects::projector::ProjectFiles, tasks::projector::TaskFiles,
};

fn structure_doc_id_from_path(path: &org_store::DurablePath) -> Option<Id> {
    use crate::processes::records::StructureDocPath;
    match crate::processes::records::parse_path(path)? {
        StructureDocPath::Organization(_, id)
        | StructureDocPath::Team(_, _, id)
        | StructureDocPath::Project(_, _, id) => Some(id),
    }
}

fn apply_imported_to_engine<E: OfficeEngine>(
    engine: &E,
    organization_id: &Id,
    files: &std::collections::BTreeMap<org_store::DurablePath, Vec<u8>>,
) -> Result<(), crate::organization::import::ImportError> {
    use crate::organization::import::ImportError;
    use org_store::{CorePort, ProjectionError, Sha256};

    // Mount every shared `state/` family projector (`OrganizationFiles`, `ProjectFiles`,
    // `TaskFiles`, `ProcessFiles`). Non-Git families (`MessagingFiles`, `OfficeLocalFiles`)
    // stay intentionally excluded so private/local files smuggled into Git fail closed (`TASK-97`).
    let projectors: Vec<Box<dyn Projector<E>>> = vec![
        Box::new(OrganizationFiles::new(organization_id.clone())),
        Box::new(ProjectFiles::new(organization_id.clone())),
        Box::new(TaskFiles::new()),
        Box::new(ProcessFiles::new()),
    ];
    engine
        .replace(&mut |tx| {
            let mut seen_structure_docs: std::collections::BTreeMap<Id, org_store::DurablePath> =
                std::collections::BTreeMap::new();
            for (path, bytes) in files {
                if let Some(doc_id) = structure_doc_id_from_path(path) {
                    if let Some(prev) = seen_structure_docs.insert(doc_id.clone(), path.clone()) {
                        return Err(ProjectionError::new(
                            path,
                            format!(
                                "duplicate structure document id `{doc_id}` already defined at `{prev}`"
                            ),
                        )
                        .into());
                    }
                }
                let mut owners = projectors.iter().filter(|p| p.owns(path));
                match (owners.next(), owners.next()) {
                    (Some(owner), None) => owner.apply(tx, path, Some(bytes))?,
                    (None, _) => {
                        return Err(ProjectionError::new(
                            path,
                            "unrecognized file in organization state",
                        )
                        .into());
                    }
                    (Some(_), Some(_)) => {
                        return Err(ProjectionError::new(
                            path,
                            "ambiguous file owner in organization state",
                        )
                        .into());
                    }
                }
                tx.set_source(path, Some(Sha256::of(bytes)))?;
            }
            for projector in &projectors {
                projector.validate(tx)?;
            }
            Ok(org_store::Cursor {
                ordinal: 0,
                sha256: Sha256::of(b"import-validation"),
                feed: org_store::FeedPosition(0),
                high_waters: org_store::HighWaters::default(),
            })
        })
        .map_err(|err| match err {
            StoreError::Projection(p) => ImportError::InvalidFile {
                path: p
                    .path
                    .map(|dp| dp.as_str().to_owned())
                    .unwrap_or_else(|| "state/organization.yaml".into()),
                reason: p.reason,
            },
            other => ImportError::Storage(other.to_string()),
        })
}

/// Validates a complete map of imported `state/` files against all shared-state production
/// projectors (`OrganizationFiles`, `ProjectFiles`, `TaskFiles`, `ProcessFiles`) and SQLite
/// constraints using a disposable private projection database (`TASK-39` / R1, `TASK-97`).
/// Any invalid or unrecognized file fails closed with its exact path and reason.
pub(crate) fn validate_imported_files(
    organization_id: &Id,
    files: &std::collections::BTreeMap<org_store::DurablePath, Vec<u8>>,
) -> Result<crate::organization::import::ValidatedImport, crate::organization::import::ImportError>
{
    use crate::organization::import::{
        ImportError, ImportedPerson, PrivateTempDir, ValidatedImport,
    };
    use crate::organization::ports::OrganizationRead;
    use crate::organization::records::{EmployeeKind, EmployeeStatus, OfficeRole, TeamStatus};
    use crate::projects::ports::ProjectsRead;
    use org_store::Engine;

    let dir = PrivateTempDir::create("org-validate")?;
    let engine = SqliteEngine::open(&dir.path().join("validate.db"))
        .map_err(|e| ImportError::Storage(e.to_string()))?
        .for_organization(organization_id);
    apply_imported_to_engine(&engine, organization_id, files)?;

    let snap = engine
        .snapshot()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let org = snap
        .organization()
        .map_err(|e| ImportError::Storage(e.to_string()))?
        .ok_or_else(|| ImportError::InvalidFile {
            path: "state/organization.yaml".into(),
            reason: "the organization record is missing".into(),
        })?;
    let repositories = snap
        .repositories()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let canonical_remote_url = repositories
        .iter()
        .find(|r| r.id == org.organization_repository_id)
        .and_then(|r| r.remote_url.clone());
    let offices = snap
        .offices()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let hq = offices
        .iter()
        .find(|o| o.role == OfficeRole::Hq)
        .ok_or_else(|| ImportError::InvalidFile {
            path: "state/organization.yaml".into(),
            reason: "no HQ office record found".into(),
        })?;
    let teams = snap
        .teams()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let root_team = teams.iter().find(|t| t.parent_team_id.is_none());
    let root_team_id = root_team.map(|t| t.id.clone());
    let has_active_root_team = root_team.is_some_and(|t| t.status == TeamStatus::Active);
    let employees = snap
        .employees()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let roles = snap
        .roles()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let projects = snap
        .projects()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let mut reserved_handles: Vec<String> = employees.iter().map(|e| e.handle.clone()).collect();
    reserved_handles.sort();
    reserved_handles.dedup();
    let mut reserved_entity_ids = std::collections::BTreeSet::new();
    reserved_entity_ids.insert(org.id.as_str().to_owned());
    for r in &repositories {
        reserved_entity_ids.insert(r.id.as_str().to_owned());
    }
    for o in &offices {
        reserved_entity_ids.insert(o.id.as_str().to_owned());
    }
    for t in &teams {
        reserved_entity_ids.insert(t.id.as_str().to_owned());
    }
    for e in &employees {
        reserved_entity_ids.insert(e.id.as_str().to_owned());
    }
    for r in &roles {
        reserved_entity_ids.insert(r.id.as_str().to_owned());
    }
    for p in &projects {
        reserved_entity_ids.insert(p.id.as_str().to_owned());
    }
    let mut people: Vec<ImportedPerson> = employees
        .iter()
        .filter(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active)
        .map(|e| ImportedPerson {
            id: e.id.as_str().to_owned(),
            name: e.name.clone(),
            handle: e.handle.clone(),
            home_team_id: e.home_team_id.as_str().to_owned(),
        })
        .collect();
    people.sort_by(|a, b| a.name.cmp(&b.name).then(a.handle.cmp(&b.handle)));
    let agents_count = employees
        .iter()
        .filter(|e| e.kind == EmployeeKind::Agent)
        .count();
    Ok(ValidatedImport {
        organization_id: org.id,
        name: org.name,
        slug: org.slug,
        canonical_remote_url,
        hq_office_id: hq.id.clone(),
        hq_office_name: hq.name.clone(),
        root_team_id,
        has_active_root_team,
        teams_count: teams.len(),
        projects_count: projects.len(),
        agents_count,
        people,
        reserved_handles,
        reserved_entity_ids: reserved_entity_ids.into_iter().collect(),
    })
}
