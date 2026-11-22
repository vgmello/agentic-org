//! Engine-independent projector for the projects family (`ProjectFiles`).

use std::collections::{BTreeMap, BTreeSet};

use org_store::{DurablePath, Engine, Id, ProjectionError, Projector};

use crate::organization::names::name_key;

use super::{
    ports::ProjectsWrite,
    records::{self, Kind, Project},
};

pub struct ProjectFiles {
    organization_id: Id,
}

impl ProjectFiles {
    pub fn new(organization_id: Id) -> Self {
        Self { organization_id }
    }
}

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn decoded<T>(
    path: &DurablePath,
    record: Result<T, crate::organization::records::RecordError>,
) -> Result<T, ProjectionError> {
    record.map_err(|error| refuse(path, error.to_string()))
}

fn agree(path: &DurablePath, agrees: bool, what: &str) -> Result<(), ProjectionError> {
    if agrees {
        return Ok(());
    }
    Err(refuse(path, format!("{what} does not match the path")))
}

impl<E: Engine> Projector<E> for ProjectFiles
where
    E::Tx: ProjectsWrite,
{
    fn owns(&self, path: &DurablePath) -> bool {
        records::kind_of(path).is_some()
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        let Some(kind) = records::kind_of(path) else {
            return Err(refuse(path, "not a project file"));
        };
        let ours = |scope: &Id| agree(path, *scope == self.organization_id, "the organization");
        match (kind, bytes) {
            (Kind::Project(id), Some(bytes)) => {
                let record = decoded(path, records::decode_project(bytes))?;
                agree(path, record.id == id, "id")?;
                tx.put_project(&record)?;
            }
            (Kind::Project(id), None) => tx.remove_project(&id)?,
            (Kind::ProjectCounter(scope), Some(bytes)) => {
                ours(&scope)?;
                let record = decoded(path, records::decode_project_counter(bytes))?;
                agree(path, record.organization_id == scope, "scope_id")?;
                tx.put_project_counter(&record)?;
            }
            (Kind::ProjectCounter(scope), None) => {
                ours(&scope)?;
                tx.remove_project_counter()?;
            }
            (Kind::ProjectAllocation(scope, number), Some(bytes)) => {
                ours(&scope)?;
                let record = decoded(path, records::decode_project_allocation(bytes))?;
                agree(path, record.organization_id == scope, "scope_id")?;
                agree(path, record.number == number, "number")?;
                tx.put_project_allocation(&record)?;
            }
            (Kind::ProjectAllocation(scope, number), None) => {
                ours(&scope)?;
                tx.remove_project_allocation(number)?;
            }
        }
        Ok(())
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        validate(tx)
    }
}

struct ValidationContext {
    organization_id: Id,
    teams: BTreeSet<Id>,
    active_teams: BTreeSet<Id>,
    employees: BTreeSet<Id>,
    offices: BTreeSet<Id>,
    roles: BTreeSet<Id>,
    repositories: BTreeSet<Id>,
}

fn validate_project_record(
    project: &Project,
    ctx: &ValidationContext,
    by_number: &mut BTreeMap<u64, Id>,
    slugs: &mut BTreeSet<String>,
    names: &mut BTreeSet<String>,
) -> Result<(), ProjectionError> {
    let path = records::project_path(&project.id);
    if project.id == ctx.organization_id
        || ctx.teams.contains(&project.id)
        || ctx.employees.contains(&project.id)
        || ctx.offices.contains(&project.id)
        || ctx.roles.contains(&project.id)
        || ctx.repositories.contains(&project.id)
    {
        return Err(refuse(&path, "project ID collides with another entity"));
    }
    if project.number == 0 {
        return Err(refuse(&path, "project number must be at least 1"));
    }
    if by_number
        .insert(project.number, project.id.clone())
        .is_some()
    {
        return Err(refuse(&path, "duplicate project number"));
    }
    if !slugs.insert(project.slug.clone()) {
        return Err(refuse(&path, "duplicate project slug"));
    }
    if !names.insert(name_key(&project.name)) {
        return Err(refuse(&path, "duplicate project name"));
    }
    validate_project_teams(project, &path, ctx)?;
    validate_project_repositories(project, &path, ctx)?;
    validate_project_actors(project, &path, &ctx.employees)
}

fn validate_project_repositories(
    project: &Project,
    path: &DurablePath,
    ctx: &ValidationContext,
) -> Result<(), ProjectionError> {
    let mut seen = BTreeSet::new();
    for link in &project.repositories {
        if !seen.insert(&link.repository_id) {
            return Err(refuse(path, "duplicate repository attached to project"));
        }
        if !ctx.repositories.contains(&link.repository_id) {
            return Err(refuse(path, "attached repository does not exist"));
        }
        if !crate::organization::records::is_branch(&link.integration_branch) {
            return Err(refuse(path, "invalid repository integration_branch"));
        }
    }
    Ok(())
}

fn validate_project_teams(
    project: &Project,
    path: &DurablePath,
    ctx: &ValidationContext,
) -> Result<(), ProjectionError> {
    if !ctx.teams.contains(&project.owner_team_id) {
        return Err(refuse(path, "owner_team_id does not name a team"));
    }
    if !project.archived && !ctx.active_teams.contains(&project.owner_team_id) {
        return Err(refuse(
            path,
            "active project owner_team_id must name an active team",
        ));
    }
    let mut seen_contrib = BTreeSet::new();
    for team_id in &project.contributing_team_ids {
        if *team_id == project.owner_team_id {
            return Err(refuse(
                path,
                "contributing_team_ids may not contain owner_team_id",
            ));
        }
        if !seen_contrib.insert(team_id) {
            return Err(refuse(path, "duplicate contributing team"));
        }
        if !ctx.teams.contains(team_id) {
            return Err(refuse(path, "contributing team does not exist"));
        }
        if !project.archived && !ctx.active_teams.contains(team_id) {
            return Err(refuse(
                path,
                "active project contributing team must be active",
            ));
        }
    }
    Ok(())
}

fn validate_project_actors(
    project: &Project,
    path: &DurablePath,
    employees: &BTreeSet<Id>,
) -> Result<(), ProjectionError> {
    for actor in [
        project.meta.created_by.employee(),
        project.meta.updated_by.employee(),
    ]
    .into_iter()
    .flatten()
    {
        if !employees.contains(actor) {
            return Err(refuse(path, "attribution employee does not exist"));
        }
    }
    Ok(())
}

fn validate_counter(
    organization_id: &Id,
    counter: Option<&records::ProjectCounter>,
    max_number: u64,
    has_records: bool,
) -> Result<(), ProjectionError> {
    let cpath = records::project_counter_path(organization_id);
    match counter {
        Some(counter) => {
            if counter.organization_id != *organization_id {
                return Err(refuse(&cpath, "counter organization_id mismatch"));
            }
            if counter.last_number < max_number {
                return Err(refuse(
                    &cpath,
                    "project counter is behind highest allocated project number",
                ));
            }
            Ok(())
        }
        None if has_records => Err(refuse(&cpath, "project counter is missing")),
        None => Ok(()),
    }
}

fn validate_allocations(
    organization_id: &Id,
    counter: Option<&records::ProjectCounter>,
    projects: &[Project],
    allocations: &[records::ProjectAllocation],
    by_number: &BTreeMap<u64, Id>,
) -> Result<(), ProjectionError> {
    let mut alloc_by_number: BTreeMap<u64, &Id> = BTreeMap::new();
    let mut alloc_by_project: BTreeSet<&Id> = BTreeSet::new();
    for alloc in allocations {
        let apath = records::project_allocation_path(organization_id, alloc.number);
        if alloc.organization_id != *organization_id {
            return Err(refuse(&apath, "allocation organization_id mismatch"));
        }
        if counter.is_some_and(|c| alloc.number > c.last_number) {
            return Err(refuse(&apath, "allocation number exceeds counter"));
        }
        if alloc_by_number
            .insert(alloc.number, &alloc.project_id)
            .is_some()
        {
            return Err(refuse(&apath, "duplicate project number allocation"));
        }
        if !alloc_by_project.insert(&alloc.project_id) {
            return Err(refuse(&apath, "project has multiple number allocations"));
        }
        if by_number.get(&alloc.number) != Some(&alloc.project_id) {
            return Err(refuse(
                &apath,
                "allocation does not match an existing project",
            ));
        }
    }
    for project in projects {
        if alloc_by_number.get(&project.number).copied() != Some(&project.id) {
            return Err(refuse(
                &records::project_path(&project.id),
                "project has no matching number allocation",
            ));
        }
    }
    Ok(())
}

fn validate(tx: &impl ProjectsWrite) -> Result<(), ProjectionError> {
    let projects = tx.projects()?;
    let allocations = tx.project_allocations()?;
    let counter = tx.project_counter()?;
    if projects.is_empty() && allocations.is_empty() && counter.is_none() {
        return Ok(());
    }
    let Some(organization) = tx.organization()? else {
        return Err(refuse(
            &DurablePath::parse("state/organization.yaml").expect("valid path"),
            "projects exist without an organization",
        ));
    };
    let all_teams = tx.teams()?;
    let ctx = ValidationContext {
        organization_id: organization.id.clone(),
        teams: all_teams.iter().map(|t| t.id.clone()).collect(),
        active_teams: all_teams
            .iter()
            .filter(|t| t.is_active())
            .map(|t| t.id.clone())
            .collect(),
        employees: tx.employees()?.into_iter().map(|e| e.id).collect(),
        offices: tx.offices()?.into_iter().map(|o| o.id).collect(),
        roles: tx.roles()?.into_iter().map(|r| r.id).collect(),
        repositories: tx.repositories()?.into_iter().map(|r| r.id).collect(),
    };

    let mut by_number: BTreeMap<u64, Id> = BTreeMap::new();
    let mut slugs: BTreeSet<String> = BTreeSet::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut max_number = 0_u64;
    for project in &projects {
        max_number = max_number.max(project.number);
        validate_project_record(project, &ctx, &mut by_number, &mut slugs, &mut names)?;
    }

    validate_counter(
        &organization.id,
        counter.as_ref(),
        max_number,
        !projects.is_empty() || !allocations.is_empty(),
    )?;
    validate_allocations(
        &organization.id,
        counter.as_ref(),
        &projects,
        &allocations,
        &by_number,
    )
}
