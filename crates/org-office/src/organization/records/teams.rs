//! Team records, the per-scope team counter and team allocations.

use org_store::{DurablePath, Id, StoreError};
use serde_json::json;

use super::yaml::RecordError;
use super::yaml::{Fields, Reader};
use super::TeamStatus;
use super::{id, line, optional_id, path, time, yaml, Meta};

// ---- teams/<id>/teams/<id>/.../team.yaml ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Team {
    pub id: Id,
    pub name: String,
    /// The organization-wide `TEAM-<number>`.
    pub number: u64,
    pub parent_team_id: Option<Id>,
    pub lead_employee_id: Option<Id>,
    pub status: TeamStatus,
    /// How many employees the team takes by hiring (document 04); at least 1.
    pub max_members: u64,
    pub meta: Meta,
}

/// The member limit a new team has (document 01).
pub const DEFAULT_MAX_MEMBERS: u64 = 6;

impl Team {
    pub fn is_active(&self) -> bool {
        self.status == TeamStatus::Active
    }
}

/// The readable reference of the team numbered `number`.
pub fn team_key(number: u64) -> String {
    format!("TEAM-{number}")
}

/// The folder of the team at the end of `chain`: a top-level team first, then each child.
pub(super) fn team_folder(chain: &[Id]) -> String {
    let mut folder = String::from("state");
    for team in chain {
        folder.push_str("/teams/");
        folder.push_str(team.as_str());
    }
    folder
}

/// The file of the team at the end of `chain`. Refused when the tree is too deep for a
/// path.
pub fn team_path(chain: &[Id]) -> Result<DurablePath, StoreError> {
    if chain.is_empty() {
        return Err(StoreError::invalid("a team path names a team"));
    }
    DurablePath::parse(&format!("{}/team.yaml", team_folder(chain)))
}

pub fn encode_team(record: &Team) -> Vec<u8> {
    let id = |id: &Option<Id>| json!(id.as_ref().map(Id::as_str));
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("name".into(), json!(record.name));
    fields.insert("number".into(), json!(record.number));
    fields.insert("parent_team_id".into(), id(&record.parent_team_id));
    fields.insert("lead_employee_id".into(), id(&record.lead_employee_id));
    fields.insert("status".into(), json!(record.status.as_str()));
    fields.insert("max_members".into(), json!(record.max_members));
    yaml::encode(&fields)
}

pub fn decode_team(bytes: &[u8]) -> Result<Team, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = Team {
        meta: Meta::read(&mut reader)?,
        id: id(&mut reader, "id")?,
        name: line(&mut reader, "name")?,
        number: reader.number("number")?,
        parent_team_id: optional_id(&mut reader, "parent_team_id")?,
        lead_employee_id: optional_id(&mut reader, "lead_employee_id")?,
        status: TeamStatus::read(&mut reader, "status")?,
        max_members: reader.number("max_members")?,
    };
    if record.max_members == 0 {
        return Err(reader.error("max_members", "below 1"));
    }
    if record.number == 0 {
        return Err(reader.error("number", "below 1"));
    }
    // The key is derived from the number. A file may repeat it, and then it must agree.
    if reader.has("key") && reader.text("key")? != team_key(record.number) {
        return Err(reader.error("key", "not TEAM-<number>"));
    }
    if record.parent_team_id.as_ref() == Some(&record.id) {
        return Err(reader.error("parent_team_id", "a team is not its own parent"));
    }
    reader.finish()?;
    Ok(record)
}

// ---- identity/counters/organization/<org-id>/team.yaml ----

/// The organization's team counter: HQ's high-water of issued team numbers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TeamCounter {
    pub organization_id: Id,
    pub last_number: u64,
}

impl TeamCounter {
    /// The counter after issuing the next number.
    pub fn advance(&self) -> Result<Self, StoreError> {
        if self.last_number >= i64::MAX as u64 {
            return Err(StoreError::Exhausted("team numbers"));
        }
        Ok(Self {
            organization_id: self.organization_id.clone(),
            last_number: self.last_number + 1,
        })
    }
}

pub fn team_counter_path(organization: &Id) -> DurablePath {
    path(&format!(
        "state/identity/counters/organization/{organization}/team.yaml"
    ))
}

pub fn encode_team_counter(record: &TeamCounter) -> Vec<u8> {
    let fields = json!({
        "scope_kind": "organization",
        "scope_id": record.organization_id.as_str(),
        "entity_kind": "team",
        "last_number": record.last_number,
    });
    yaml::encode(fields.as_object().expect("an object"))
}

/// The scope every team counter and allocation has: this organization, teams.
fn team_scope(reader: &mut Reader) -> Result<Id, RecordError> {
    if reader.text("scope_kind")? != "organization" {
        return Err(reader.error("scope_kind", "not organization"));
    }
    let organization = id(reader, "scope_id")?;
    if reader.text("entity_kind")? != "team" {
        return Err(reader.error("entity_kind", "not team"));
    }
    Ok(organization)
}

pub fn decode_team_counter(bytes: &[u8]) -> Result<TeamCounter, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = TeamCounter {
        organization_id: team_scope(&mut reader)?,
        last_number: reader.number("last_number")?,
    };
    reader.finish()?;
    Ok(record)
}

// ---- identity/allocations/organization/<org-id>/team/<number>.yaml ----

/// The immutable record that a team number was issued to a team.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TeamAllocation {
    pub organization_id: Id,
    pub number: u64,
    pub team_id: Id,
    pub allocated_at: String,
}

pub fn team_allocation_path(organization: &Id, number: u64) -> DurablePath {
    path(&format!(
        "state/identity/allocations/organization/{organization}/team/{number}.yaml"
    ))
}

pub fn encode_team_allocation(record: &TeamAllocation) -> Vec<u8> {
    let fields = json!({
        "scope_kind": "organization",
        "scope_id": record.organization_id.as_str(),
        "entity_kind": "team",
        "number": record.number,
        "entity_id": record.team_id.as_str(),
        "allocated_at": record.allocated_at,
    });
    yaml::encode(fields.as_object().expect("an object"))
}

pub fn decode_team_allocation(bytes: &[u8]) -> Result<TeamAllocation, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = TeamAllocation {
        organization_id: team_scope(&mut reader)?,
        number: reader.number("number")?,
        team_id: id(&mut reader, "entity_id")?,
        allocated_at: time(&mut reader, "allocated_at")?,
    };
    if record.number == 0 {
        return Err(reader.error("number", "below 1"));
    }
    reader.finish()?;
    Ok(record)
}
