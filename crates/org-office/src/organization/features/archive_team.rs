//! Archiving a team. Only an empty team is archived: no active child team and no
//! employee who is not inactive (owner decision Q5). It keeps its ID, number and records
//! (document 01); there is no unarchiving in B2.

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    changed, invalid,
    shared::{admit, check_revision, reject, team_file, touched},
    stable, Feature, Op, RejectCode,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{self, EmployeeStatus, Team, TeamStatus},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct ArchiveTeam {
    pub id: Id,
    pub expected_revision: u64,
}

impl Feature for ArchiveTeam {
    const OP: Op = Op::ArchiveTeam;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: TeamDependents,
    {
        decide(uow, here, owner, by, self)
    }
}

pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &ArchiveTeam,
) -> Result<Change, Refusal>
where
    E::Tx: TeamDependents,
{
    stable(&command.id, "the team's ID")?;
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let Some(team) = view.team(&command.id)? else {
        return reject(RejectCode::NotFound, "no such team");
    };
    if team.number == 1 {
        let rule = "the organization's first team is never archived";
        return reject(RejectCode::InvalidRequest, rule);
    }
    if !team.is_active() {
        return reject(RejectCode::InvalidRequest, "the team is archived");
    }
    let teams = view.teams()?;
    let child_teams = teams
        .iter()
        .filter(|child| child.parent_team_id.as_ref() == Some(&team.id) && child.is_active())
        .count();
    let employees = view.employees()?;
    let members = employees
        .iter()
        .filter(|e| e.home_team_id == team.id && e.status != EmployeeStatus::Inactive)
        .count();
    if child_teams > 0 || members > 0 {
        let count = |n: usize, one: &str, many: &str| match n {
            1 => format!("1 {one}"),
            _ => format!("{n} {many}"),
        };
        let message = format!(
            "the team still has {} and {}: move or archive them first",
            count(child_teams, "active team", "active teams"),
            count(members, "employee", "employees"),
        );
        return reject(RejectCode::NotEmpty, message);
    }
    if view.team_in_active_project(&team.id)? {
        return reject(
            RejectCode::NotEmpty,
            "the team is still accountable for or contributing to an active project",
        );
    }
    check_revision(team.meta.revision, command.expected_revision)?;
    let Some(chain) = view.team_chain(&team.id)? else {
        return invalid("the team has no chain of parents to a top-level team");
    };

    let archived = Team {
        status: TeamStatus::Archived,
        meta: touched(&team.meta, &admitted.by, admitted.now),
        ..team
    };
    let reply = changed(&archived.id, archived.meta.revision, admitted.generation);
    Ok(Change::new(Op::ArchiveTeam.ack())
        .put(team_file(&chain)?, records::encode_team(&archived))
        .reply(reply))
}
