//! The organization's structure as the native protocol's `org.snapshot` gives it
//! (`org_proto::OrgSnapshot`), from one consistent read of the organization and projects
//! families. Pure:
//! it reads through the read port and names no store and no engine.
//!
//! Agents (including the built-in HR lead) carry their `office_id` (HQ office) and
//! `AgentProfile` (`status`, `messaging_name`, `model`, `title`, `role`); `run_mode` stays
//! unset until B6 adds live presence tracking. Every employee belongs to one home team
//! (`Member::home_team_id`, data contract document 01).

use std::collections::{BTreeMap, BTreeSet};

use org_proto::org::{
    AgentProfile, AgentRuntime, AgentStatus, Member, MemberKind, OrgSnapshot, Position, Project,
    ProjectTeam, Team,
};
use org_store::{EngineError, Id};

use crate::organization::records::{self, EmployeeKind, EmployeeStatus, HireState, OfficeRole};
use crate::projects::{ports::ProjectsRead, records::Project as ProjectRecord};

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
pub fn project(read: &impl ProjectsRead, owner: &Id) -> Result<OrgSnapshot, EngineError> {
    let generation = read.generation()?;
    let teams = read
        .teams()?
        .into_iter()
        .filter(|t| t.is_active())
        .map(team)
        .collect();
    let offices = read.offices()?;
    let enrolled_offices: BTreeSet<String> =
        offices.iter().map(|office| office.id.to_string()).collect();
    let hq_office_id = offices
        .into_iter()
        .find(|office| office.role == OfficeRole::Hq)
        .map(|office| office.id.to_string());
    let mut employee_offices: BTreeMap<String, String> = BTreeMap::new();
    for req in read.hire_requests()? {
        if req.state == HireState::Hired {
            if let (Some(emp_id), Some(oid)) = (req.employee_id, req.proposal.office_id) {
                if enrolled_offices.contains(&oid) {
                    employee_offices.insert(emp_id.to_string(), oid);
                }
            }
        }
    }
    let employees = read.employees()?;
    let mut members = Vec::with_capacity(employees.len());
    for employee in &employees {
        if employee.status == EmployeeStatus::Inactive {
            continue;
        }
        let exec = if employee.kind == EmployeeKind::Agent {
            read.agent_execution(&employee.id)?
        } else {
            None
        };
        let agent_office_id = employee_offices
            .get(employee.id.as_str())
            .map(String::as_str)
            .or(hq_office_id.as_deref());
        members.push(member(employee, owner, agent_office_id, exec.as_ref()));
    }
    let project_records = read.projects()?;
    let mut projects = Vec::with_capacity(project_records.len());
    let mut project_teams = Vec::new();
    for record in &project_records {
        projects.push(project_summary(record));
        project_teams.push(ProjectTeam {
            id: format!("{}:{}", record.id, record.owner_team_id),
            project_id: record.id.to_string(),
            team_id: record.owner_team_id.to_string(),
            version: record.meta.revision,
        });
        for team_id in &record.contributing_team_ids {
            project_teams.push(ProjectTeam {
                id: format!("{}:{}", record.id, team_id),
                project_id: record.id.to_string(),
                team_id: team_id.to_string(),
                version: record.meta.revision,
            });
        }
    }
    Ok(OrgSnapshot {
        position: position(generation),
        teams,
        members,
        projects,
        project_teams,
    })
}

fn project_summary(record: &ProjectRecord) -> Project {
    let mut summary = Project::new(record.id.to_string(), record.name.clone());
    summary.key = record.key();
    summary.archived = record.archived;
    summary.created_at = record.meta.created_at.clone();
    summary.version = record.meta.revision;
    summary
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

fn member(
    record: &records::Employee,
    owner: &Id,
    hq_office_id: Option<&str>,
    exec: Option<&records::AgentExecutionConfig>,
) -> Member {
    let kind = match record.kind {
        EmployeeKind::Person => MemberKind::Person,
        EmployeeKind::Agent => MemberKind::Agent,
    };
    let (office_id, agent) = if record.kind == EmployeeKind::Agent {
        let status = match record.status {
            EmployeeStatus::Active => AgentStatus::Active,
            EmployeeStatus::Paused => AgentStatus::Paused,
            EmployeeStatus::Inactive => AgentStatus::Offboarded,
        };
        let profile = AgentProfile {
            role: record
                .role_text
                .clone()
                .unwrap_or_else(|| "Agent".to_owned()),
            title: record
                .title
                .clone()
                .or_else(|| record.title_in_team.clone())
                .unwrap_or_else(|| record.name.clone()),
            focus: record.focus.clone(),
            runtime: AgentRuntime::ClaudeCode,
            model: exec.map(|cfg| cfg.model_id.clone()),
            // Runtime presence and run-mode status lines stay unset until B6.
            run_mode: None,
            schedule: None,
            home: None,
            messaging_name: record.handle.clone(),
            status,
            offboarded_at: record.offboarded_at.clone(),
        };
        (hq_office_id.map(str::to_owned), Some(profile))
    } else {
        (None, None)
    };
    // Reporting to the owner is stored as reporting to no one on the wire: one form per line.
    let reports_to = record.reports_to_employee_id.as_ref();
    let reports_to = reports_to.filter(|id| *id != owner).map(Id::to_string);
    Member {
        id: record.id.to_string(),
        kind,
        name: record.name.clone(),
        handle: Some(record.handle.clone()),
        home_team_id: Some(record.home_team_id.to_string()),
        role_id: record.role_id.as_ref().map(Id::to_string),
        title_in_team: record.title_in_team.clone(),
        contact: record.contact.clone(),
        owner: record.id == *owner,
        office_id,
        agent,
        reports_to_member_id: reports_to,
        created_at: record.meta.created_at.clone(),
        version: record.meta.revision,
    }
}
