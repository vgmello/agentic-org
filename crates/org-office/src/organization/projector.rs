//! The organization family's projection of its own files: the same function when an
//! operation is applied and when the projection is rebuilt. Generic over the engine: it
//! decodes a file and calls the write port.

use std::collections::{BTreeMap, BTreeSet};

use org_store::{DurablePath, Engine, Id, ProjectionError, Projector};

use super::{
    ports::OrganizationWrite,
    records::{self, By, Kind, Meta, OfficeRole, RecordError, RepositoryKind, Team},
};

/// The projector of one organization's files. Team and employee files do not hold the
/// organization ID, and the counter files are scoped by it, so it comes from the store's
/// identity.
pub struct OrganizationFiles {
    organization_id: Id,
}

impl OrganizationFiles {
    pub fn new(organization_id: Id) -> Self {
        Self { organization_id }
    }
}

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

/// Decoded, or refused with the field that is wrong.
fn decoded<T>(path: &DurablePath, record: Result<T, RecordError>) -> Result<T, ProjectionError> {
    record.map_err(|error| refuse(path, error.to_string()))
}

/// The file and the folder it is in must agree.
fn agree(path: &DurablePath, agrees: bool, what: &str) -> Result<(), ProjectionError> {
    if agrees {
        return Ok(());
    }
    Err(refuse(path, format!("{what} does not match the path")))
}

impl<E: Engine> Projector<E> for OrganizationFiles
where
    E::Tx: OrganizationWrite,
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
            return Err(refuse(path, "not an organization file"));
        };
        let ours = |scope: &Id| agree(path, *scope == self.organization_id, "the organization");
        match (kind, bytes) {
            (Kind::Organization, Some(bytes)) => {
                let record = decoded(path, records::decode_organization(bytes))?;
                agree(path, record.id == self.organization_id, "id")?;
                tx.put_organization(&record)?;
            }
            (Kind::Organization, None) => tx.remove_organization()?,
            (Kind::Repositories, Some(bytes)) => {
                tx.put_repositories(&decoded(path, records::decode_repositories(bytes))?)?;
            }
            (Kind::Repositories, None) => tx.put_repositories(&[])?,
            (Kind::Office(id), Some(bytes)) => {
                let record = decoded(path, records::decode_office(bytes))?;
                agree(path, record.id == id, "id")?;
                tx.put_office(&record)?;
            }
            (Kind::Office(id), None) => tx.remove_office(&id)?,
            (Kind::OfficeCodes, Some(bytes)) => {
                tx.put_office_codes(&decoded(path, records::decode_office_codes(bytes))?)?;
            }
            (Kind::OfficeCodes, None) => tx.put_office_codes(&[])?,
            (Kind::Team(chain), Some(bytes)) => {
                let record = decoded(path, records::decode_team(bytes))?;
                let (id, above) = chain.split_last().expect("a team path names a team");
                agree(path, record.id == *id, "id")?;
                let parent = record.parent_team_id.as_ref();
                agree(path, parent == above.last(), "parent_team_id")?;
                tx.put_team(&record)?;
            }
            (Kind::Team(chain), None) => {
                tx.remove_team(chain.last().expect("a team path names a team"))?;
            }
            (Kind::Employee(chain, kind, id), Some(bytes)) => {
                let record = decoded(path, records::decode_employee(bytes))?;
                agree(path, record.id == id, "id")?;
                agree(path, record.kind == kind, "kind")?;
                let home = Some(&record.home_team_id);
                agree(path, home == chain.last(), "home_team_id")?;
                tx.put_employee(&record)?;
            }
            (Kind::Employee(_, _, id), None) => tx.remove_employee(&id)?,
            (Kind::TeamCounter(scope), Some(bytes)) => {
                ours(&scope)?;
                let record = decoded(path, records::decode_team_counter(bytes))?;
                agree(path, record.organization_id == scope, "scope_id")?;
                tx.put_team_counter(&record)?;
            }
            (Kind::TeamCounter(scope), None) => {
                ours(&scope)?;
                tx.remove_team_counter()?;
            }
            (Kind::TeamAllocation(scope, number), Some(bytes)) => {
                ours(&scope)?;
                let record = decoded(path, records::decode_team_allocation(bytes))?;
                agree(path, record.organization_id == scope, "scope_id")?;
                agree(path, record.number == number, "number")?;
                tx.put_team_allocation(&record)?;
            }
            (Kind::TeamAllocation(scope, number), None) => {
                ours(&scope)?;
                tx.remove_team_allocation(number)?;
            }
        }
        Ok(())
    }

    /// The rules that span files. An engine with constraints enforces some of them
    /// itself; the memory engine has none, so all of them are here.
    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        validate(tx)
    }
}

fn validate(tx: &impl OrganizationWrite) -> Result<(), ProjectionError> {
    let repositories = tx.repositories()?;
    let offices = tx.offices()?;
    let codes = tx.office_codes()?;
    let teams = tx.teams()?;
    let employees = tx.employees()?;
    let counter = tx.team_counter()?;
    let allocations = tx.team_allocations()?;
    let organization_path = records::organization_path();

    let Some(organization) = tx.organization()? else {
        let empty = repositories.is_empty()
            && offices.is_empty()
            && codes.is_empty()
            && teams.is_empty()
            && employees.is_empty()
            && counter.is_none()
            && allocations.is_empty();
        if empty {
            return Ok(());
        }
        let reason = "the organization record is missing";
        return Err(refuse(&organization_path, reason));
    };

    let is_home = |repository: &records::Repository| {
        repository.id == organization.organization_repository_id
            && repository.kind == RepositoryKind::Remote
    };
    if !repositories.iter().any(is_home) {
        let reason = "organization_repository_id names no remote repository";
        return Err(refuse(&organization_path, reason));
    }
    let headquarters = offices.iter().filter(|o| o.role == OfficeRole::Hq).count();
    if headquarters != 1 {
        let reason = format!("{headquarters} offices have the role hq; exactly one does");
        return Err(refuse(&organization_path, reason));
    }
    let mut slugs = BTreeSet::new();
    for office in &offices {
        if !slugs.insert(&office.slug) {
            let path = records::office_path(&office.id);
            return Err(refuse(&path, "another office has this slug"));
        }
    }
    for code in &codes {
        if !offices.iter().any(|office| office.id == code.office_id) {
            let reason = format!("code {} names no office", code.code);
            return Err(refuse(&records::office_codes_path(), reason));
        }
    }

    // Teams: a tree, numbered once each, led from inside.
    let by_id: BTreeMap<&Id, &Team> = teams.iter().map(|team| (&team.id, team)).collect();
    let home_of: BTreeMap<&Id, &Id> = employees
        .iter()
        .map(|employee| (&employee.id, &employee.home_team_id))
        .collect();
    let mut chains: BTreeMap<&Id, Vec<Id>> = BTreeMap::new();
    let mut numbers = BTreeSet::new();
    for team in &teams {
        // Its own file may be unknown when a parent is missing: name the row instead.
        let Some(chain) = tx.team_chain(&team.id)? else {
            let reason = format!(
                "team {} has no chain of parents to a top-level team",
                team.id
            );
            return Err(refuse(&organization_path, reason));
        };
        let path =
            records::team_path(&chain).map_err(|e| refuse(&organization_path, e.to_string()))?;
        if !numbers.insert(team.number) {
            return Err(refuse(&path, "another team has this number"));
        }
        let allocated = allocations.iter().find(|a| a.number == team.number);
        if allocated.is_none_or(|allocation| allocation.team_id != team.id) {
            return Err(refuse(&path, "its number is not allocated to it"));
        }
        if let Some(lead) = &team.lead_employee_id {
            if home_of.get(lead) != Some(&&team.id) {
                return Err(refuse(&path, "its lead is not an employee of the team"));
            }
        }
        chains.insert(&team.id, chain);
    }

    // The counter is never behind what it issued, and every allocation names its team.
    let allocations_path = |number: u64| records::team_allocation_path(&organization.id, number);
    let counter_path = records::team_counter_path(&organization.id);
    let highest = allocations.iter().map(|a| a.number).max().unwrap_or(0);
    match &counter {
        Some(counter) if counter.organization_id != organization.id => {
            return Err(refuse(&counter_path, "scoped to another organization"));
        }
        Some(counter) if counter.last_number < highest => {
            return Err(refuse(&counter_path, "behind an allocation"));
        }
        None if highest > 0 || !teams.is_empty() => {
            return Err(refuse(&counter_path, "the team counter is missing"));
        }
        _ => {}
    }
    for allocation in &allocations {
        let team = by_id.get(&allocation.team_id);
        if team.is_none_or(|team| team.number != allocation.number) {
            let reason = "names no team with this number";
            return Err(refuse(&allocations_path(allocation.number), reason));
        }
    }

    // Employees: a home team, one handle each, reporting lines that end.
    let profile = |employee: &records::Employee| {
        let chain = chains.get(&employee.home_team_id)?;
        records::employee_path(chain, employee.kind, &employee.id).ok()
    };
    let mut handles = BTreeSet::new();
    for employee in &employees {
        let Some(path) = profile(employee) else {
            let reason = format!("employee {} has no home team", employee.id);
            return Err(refuse(&organization_path, reason));
        };
        if !handles.insert(&employee.handle) {
            return Err(refuse(&path, "another employee has this handle"));
        }
        let mut above = employee.reports_to_employee_id.as_ref();
        let mut steps = 0;
        while let Some(id) = above {
            let Some(next) = employees.iter().find(|e| e.id == *id) else {
                return Err(refuse(&path, "reports to nobody on record"));
            };
            steps += 1;
            if steps > employees.len() {
                return Err(refuse(&path, "its reporting line is a cycle"));
            }
            above = next.reports_to_employee_id.as_ref();
        }
    }

    // Every record is attributed to an employee on record.
    let known = |by: &By| by.employee().is_none_or(|id| home_of.contains_key(id));
    let attributed = |meta: &Meta| known(&meta.created_by) && known(&meta.updated_by);
    let reason = "attributed to nobody on record";
    if !attributed(&organization.meta) {
        return Err(refuse(&organization_path, reason));
    }
    if !repositories.iter().all(|r| attributed(&r.meta)) {
        return Err(refuse(&records::repositories_path(), reason));
    }
    if let Some(office) = offices.iter().find(|o| !attributed(&o.meta)) {
        return Err(refuse(&records::office_path(&office.id), reason));
    }
    if let Some(team) = teams.iter().find(|t| !attributed(&t.meta)) {
        let path = records::team_path(&chains[&team.id]);
        return Err(refuse(&path.expect("checked above"), reason));
    }
    if let Some(employee) = employees.iter().find(|e| !attributed(&e.meta)) {
        return Err(refuse(&profile(employee).expect("checked above"), reason));
    }

    // Folders match the fields: every file is where its record's parent chain puts it.
    let mut expected: BTreeSet<DurablePath> = BTreeSet::new();
    expected.extend(
        chains
            .values()
            .filter_map(|chain| records::team_path(chain).ok()),
    );
    expected.extend(employees.iter().filter_map(profile));
    let teams_folder = DurablePath::parse("state/teams").expect("a fixed path");
    for path in tx.sources_under(&teams_folder)? {
        if records::kind_of(&path).is_some() && !expected.contains(&path) {
            return Err(refuse(&path, "not in the folder its parent chain names"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
