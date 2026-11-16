//! Canonical organization facts; task relocation policy stays with task consumers.
use crate::OfficeError;
use org_work::selected_codec::Json;
use std::collections::{BTreeMap, BTreeSet};
fn invalid() -> OfficeError {
    OfficeError::Invalid("selected directory source rejected")
}
pub(super) struct Facts<'a> {
    teams: &'a BTreeMap<String, Json>,
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
        let facts = Self { teams };
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
