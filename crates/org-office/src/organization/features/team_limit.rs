//! Setting a team's `max_members` limit (ADR-0029 decision 8 / HIRE-6).
//!
//! Allowed for the organization owner or an active **person** who leads that team or an ancestor
//! team above it. An agent NEVER changes a team's member limit (`not_authorized`).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    authority::{admit_active_employee, is_owner, is_person_ancestor_lead},
    changed, invalid,
    shared::{check_revision, reject, team_file, touched},
    stable, Feature, Op, RejectCode, NOT_AUTHORIZED,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{self, EmployeeKind, Team},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SetTeamMaxMembers {
    pub id: Id,
    pub expected_revision: u64,
    pub max_members: u64,
}

impl Feature for SetTeamMaxMembers {
    const OP: Op = Op::SetTeamMaxMembers;

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
    command: &SetTeamMaxMembers,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    stable(&command.id, "the team's ID")?;
    let admitted = admit_active_employee(uow, here, by)?;
    if admitted.actor.kind != EmployeeKind::Person {
        return invalid(NOT_AUTHORIZED);
    }
    let view = uow.view();
    let Some(team) = view.team(&command.id)? else {
        return reject(RejectCode::NotFound, "no such team");
    };
    let allowed = is_owner(owner, &admitted.actor)
        || is_person_ancestor_lead(view, &admitted.actor, &team.id, true)?;
    if !allowed {
        return invalid(NOT_AUTHORIZED);
    }
    if !team.is_active() {
        return reject(RejectCode::InvalidRequest, "the team is archived");
    }
    if !(1..=100).contains(&command.max_members) {
        return reject(
            RejectCode::InvalidRequest,
            "max_members must be between 1 and 100",
        );
    }
    check_revision(team.meta.revision, command.expected_revision)?;
    let Some(chain) = view.team_chain(&team.id)? else {
        return invalid("the team has no chain of parents to a top-level team");
    };
    let updated = Team {
        max_members: command.max_members,
        meta: touched(&team.meta, &admitted.by, admitted.now),
        ..team
    };
    let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
    Ok(Change::new(Op::SetTeamMaxMembers.ack())
        .put(team_file(&chain)?, records::encode_team(&updated))
        .reply(reply))
}
