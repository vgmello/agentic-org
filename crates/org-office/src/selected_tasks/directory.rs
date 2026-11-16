//! Host/HQ admission adapters. Directory content is data, never account authority.
//! Unknown JSON content remains in the exact retained source string.

use super::{service, AcceptanceIdentity, OwnerAuthority, TaskSnapshot};
use crate::{store::Store, OfficeError};
use org_work::selected_codec::{self as codec, Json};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}
fn invalid() -> OfficeError {
    OfficeError::Invalid("selected directory source rejected")
}

pub(crate) fn bind_owner(
    store: &Store,
    authority: Arc<dyn OwnerAuthority>,
) -> Result<(), OfficeError> {
    // A verified existing binding is a coherent read, including after ordinal
    // exhaustion. Reopening must not allocate a redundant transaction merely
    // to regain protected receipt reads. Fresh installation remains writer-bound.
    let verification = Arc::clone(&authority);
    if store.selected_read(move |db, id| {
        let prior: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM selected_owner_authority WHERE organization_id=?1)",
                [&id.organization_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if prior {
            service::owner(db, id, &*verification)?;
        }
        Ok(prior)
    })? {
        return Ok(());
    }
    store.selected_transaction(move|db,id| {
        let binding=authority.verify_live(id)?;
        if binding.account_id.is_empty()||binding.member_id.is_empty() {return Err(invalid());}
        let prior:Option<(String,String,i64)>=db.query_row("SELECT account_id,member_id,active FROM selected_owner_authority WHERE organization_id=?1",[&id.organization_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)?;
        if let Some(prior)=prior {
            if prior!=(binding.account_id,binding.member_id,1) {return Err(OfficeError::Invalid("protected owner binding requires recovery"));}
            return Ok(());
        }
        db.execute("INSERT INTO selected_owner_authority VALUES(?1,?2,?3,1,1)",params![id.organization_id,binding.account_id,binding.member_id]).map_err(storage)?;
        Ok(())
    })
}

pub(crate) fn admit(
    store: &Store,
    authority: Arc<dyn OwnerAuthority>,
    expected: Option<i64>,
    source: Vec<u8>,
) -> Result<i64, OfficeError> {
    let parsed = codec::parse(&source).map_err(storage)?;
    codec::canonical(&parsed).map_err(storage)?;
    let source = String::from_utf8(source).map_err(|_| invalid())?;
    store.selected_transaction(move|db,id| {
        service::owner(db,id,&*authority)?;
        let facts=Facts::validate(&parsed,&id.organization_id)?;
        let current:Option<i64>=db.query_row("SELECT generation FROM selected_directory_head WHERE organization_id=?1",[&id.organization_id],|r|r.get(0)).optional().map_err(storage)?;
        if current!=expected {return Err(OfficeError::Invalid("directory generation conflict"));}
        let next=current.unwrap_or(0).checked_add(1).ok_or_else(invalid)?;
        // No assignment can race the generation change. A hierarchy/project
        // owner edit which would relocate an adopted task needs its own workflow.
        validate_adopted_paths(db,id,&facts)?;
        db.execute("INSERT INTO selected_directory VALUES(?1,?2,?3,?4)",params![id.organization_id,next,source,codec::sha256(source.as_bytes())]).map_err(storage)?;
        db.execute("INSERT INTO selected_directory_head VALUES(?1,?2) ON CONFLICT(organization_id) DO UPDATE SET generation=excluded.generation",params![id.organization_id,next]).map_err(storage)?;
        Ok(next)
    })
}

pub(super) struct Facts<'a> {
    teams: &'a BTreeMap<String, Json>,
    projects: &'a BTreeMap<String, Json>,
}
fn map<'a>(j: &'a Json, key: &str) -> Result<&'a BTreeMap<String, Json>, OfficeError> {
    match j.get(key) {
        Some(Json::Object(map)) => Ok(map),
        _ => Err(invalid()),
    }
}
fn text<'a>(j: &'a Json, key: &str) -> Result<&'a str, OfficeError> {
    j.get(key)
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(invalid)
}
fn optional<'a>(j: &'a Json, key: &str) -> Result<Option<&'a str>, OfficeError> {
    match j.get(key) {
        Some(Json::Null) => Ok(None),
        Some(Json::String(s)) if !s.is_empty() => Ok(Some(s)),
        _ => Err(invalid()),
    }
}
impl<'a> Facts<'a> {
    pub(super) fn validate(j: &'a Json, organization: &str) -> Result<Self, OfficeError> {
        if text(j, "organization_id")? != organization
            || j.get("schema_version").and_then(Json::positive_i64) != Some(1)
        {
            return Err(invalid());
        }
        let position = j.get("source_position").ok_or_else(invalid)?;
        for key in ["epoch", "seq"] {
            if !matches!(position.get(key),Some(Json::Number(n)) if n.as_str()=="0")
                && position.get(key).and_then(Json::positive_i64).is_none()
            {
                return Err(invalid());
            }
        }
        let teams = map(j, "teams")?;
        let employees = map(j, "employees")?;
        let projects = map(j, "projects")?;
        let facts = Self { teams, projects };
        for collection in [teams, employees, projects] {
            for (id, fact) in collection {
                validate_record(id, fact, organization)?;
            }
        }
        facts.validate_teams(employees)?;
        validate_employees(teams, employees)?;
        for project in projects.values() {
            if !teams.contains_key(text(project, "owner_team_id")?) {
                return Err(invalid());
            }
        }
        Ok(facts)
    }

    fn validate_teams(&self, employees: &BTreeMap<String, Json>) -> Result<(), OfficeError> {
        for (id, team) in self.teams {
            if !matches!(text(team, "status")?, "active" | "archived") {
                return Err(invalid());
            }
            optional(team, "parent_team_id")?;
            self.team_path(id)?;
            if let Some(lead) = optional(team, "lead_employee_id")? {
                if !employees.contains_key(lead) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
    pub(super) fn team_path(&self, team: &str) -> Result<String, OfficeError> {
        let mut seen = BTreeSet::new();
        let mut chain = Vec::new();
        let mut next = Some(team);
        while let Some(id) = next {
            if !seen.insert(id) {
                return Err(invalid());
            }
            path_component(id)?;
            chain.push(format!("teams/{id}"));
            let fact = self.teams.get(id).ok_or_else(invalid)?;
            next = optional(fact, "parent_team_id")?;
        }
        chain.reverse();
        Ok(chain.join("/"))
    }
    pub(super) fn task_path(&self, snapshot: &TaskSnapshot<'_>) -> Result<String, OfficeError> {
        let facts = snapshot.immutable();
        path_component(&facts.task_id)?;
        if !self.teams.contains_key(&facts.team_id) {
            return Err(invalid());
        }
        let root = if let Some(project) = &facts.project_id {
            path_component(project)?;
            let project_fact = self.projects.get(project).ok_or_else(invalid)?;
            if text(project_fact, "owner_team_id")? != facts.team_id {
                return Err(invalid());
            }
            format!("projects/{project}")
        } else {
            self.team_path(&facts.team_id)?
        };
        Ok(format!("{root}/work/tasks/{}.md", facts.task_id))
    }
}

fn validate_record(id: &str, fact: &Json, organization: &str) -> Result<(), OfficeError> {
    if id.is_empty()
        || text(fact, "id")? != id
        || text(fact, "organization_id")? != organization
        || fact.get("schema_version").and_then(Json::positive_i64) != Some(1)
        || fact.get("revision").and_then(Json::positive_i64).is_none()
    {
        return Err(invalid());
    }
    text(fact, "name")?;
    Ok(())
}

fn validate_employees(
    teams: &BTreeMap<String, Json>,
    employees: &BTreeMap<String, Json>,
) -> Result<(), OfficeError> {
    let mut handles = BTreeSet::new();
    for employee in employees.values() {
        if !matches!(text(employee, "status")?, "active" | "inactive")
            || !matches!(text(employee, "kind")?, "person" | "agent")
            || !teams.contains_key(text(employee, "home_team_id")?)
            || !handles.insert(text(employee, "handle")?)
        {
            return Err(invalid());
        }
        if let Some(manager) = optional(employee, "reports_to_employee_id")? {
            if !employees.contains_key(manager) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
pub(super) fn path_component(id: &str) -> Result<(), OfficeError> {
    let path = std::path::Path::new(id);
    if id.is_empty()
        || id.len() > 255
        || path.components().count() != 1
        || path.file_name().and_then(|s| s.to_str()) != Some(id)
        || !matches!(
            path.components().next(),
            Some(std::path::Component::Normal(_))
        )
        || id.chars().any(char::is_control)
    {
        return Err(OfficeError::Invalid(
            "unsupported identity path representation",
        ));
    }
    Ok(())
}
pub(super) fn validate_adopted_paths(
    db: &Connection,
    id: &AcceptanceIdentity,
    facts: &Facts<'_>,
) -> Result<(), OfficeError> {
    let mut query = db
        .prepare("SELECT markdown,canonical_path FROM selected_tasks WHERE organization_id=?1")
        .map_err(storage)?;
    let mut rows = query.query([&id.organization_id]).map_err(storage)?;
    while let Some(row) = rows.next().map_err(storage)? {
        let bytes: Vec<u8> = row.get(0).map_err(storage)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
        let snapshot = TaskSnapshot::parse(&id.organization_id, text).map_err(storage)?;
        if facts.task_path(&snapshot)? != row.get::<_, String>(1).map_err(storage)? {
            return Err(OfficeError::Invalid(
                "directory change requires task relocation",
            ));
        }
    }
    Ok(())
}
