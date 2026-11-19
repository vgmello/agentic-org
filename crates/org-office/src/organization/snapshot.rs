//! The organization's structure as the native protocol's `org.snapshot` gives it
//! (`org_proto::OrgSnapshot`), from one consistent read of the organization family. Pure:
//! it reads through the read port and names no store and no engine.
//!
//! What the records do not hold yet, the snapshot leaves empty until the family that owns
//! it is built: projects, project teams and areas; an agent's profile and its office
//! (agents are hired through HR from B2 on, so the built-in HR lead has neither, and no
//! reader takes it for an agent waiting to be placed). Every employee is a member of one
//! team, its home team (data contract document 01), so each has one membership.

use org_proto::org::{Member, MemberKind, Membership, OrgSnapshot, Position, Team};
use org_store::{EngineError, Id};

use super::{
    ports::OrganizationRead,
    records::{self, EmployeeKind},
};

/// The protocol position of directory generation `generation`. The organization's
/// positions are one sequence, the generation, in the one epoch the store has: `epoch` is
/// always 1. A rebuild keeps the generation, so it keeps the position.
pub fn position(generation: u64) -> Position {
    Position {
        epoch: 1,
        seq: generation,
    }
}

/// The snapshot `read` shows. `owner` is the person whose account owns the organization:
/// the creator, as the creation binding names them (document 04).
pub fn project(read: &impl OrganizationRead, owner: &Id) -> Result<OrgSnapshot, EngineError> {
    let generation = read.generation()?;
    let teams = read.teams()?.into_iter().map(team).collect();
    let employees = read.employees()?;
    let memberships = employees.iter().map(membership).collect();
    let members = employees.iter().map(|e| member(e, owner)).collect();
    Ok(OrgSnapshot {
        position: position(generation),
        teams,
        members,
        memberships,
        projects: Vec::new(),
        project_teams: Vec::new(),
        areas: Vec::new(),
    })
}

fn team(record: records::Team) -> Team {
    Team {
        id: record.id.to_string(),
        name: record.name,
        owner_team_id: record.parent_team_id.map(String::from),
        lead_member_id: record.lead_employee_id.map(String::from),
        version: record.meta.revision,
    }
}

fn member(record: &records::Employee, owner: &Id) -> Member {
    let kind = match record.kind {
        EmployeeKind::Person => MemberKind::Person,
        EmployeeKind::Agent => MemberKind::Agent,
    };
    // Reporting to the owner is stored as reporting to no one on the wire: one form per line.
    let reports_to = record.reports_to_employee_id.as_ref();
    let reports_to = reports_to.filter(|id| *id != owner).map(Id::to_string);
    Member {
        id: record.id.to_string(),
        kind,
        name: record.name.clone(),
        contact: record.contact.clone(),
        owner: record.id == *owner,
        office_id: None,
        agent: None,
        reports_to_member_id: reports_to,
        created_at: record.meta.created_at.clone(),
        version: record.meta.revision,
    }
}

/// An employee's membership of its home team. It has no record of its own, so it is named
/// by its employee: each employee has exactly one.
fn membership(record: &records::Employee) -> Membership {
    Membership {
        id: record.id.to_string(),
        team_id: record.home_team_id.to_string(),
        member_id: record.id.to_string(),
        title_in_team: record.title_in_team.clone(),
        version: record.meta.revision,
    }
}
