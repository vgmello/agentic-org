//! Creating a team, under an existing one or at the top level (the organization is a
//! team forest, owner decision Q1). HQ numbers it: the team, the advanced counter and the
//! number's immutable allocation are the files of one entry (document 06).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::{Deserialize, Serialize};

use super::{
    invalid, reply,
    shared::{admit, id_in_use, reject, sibling_named, team_file},
    stable, Feature, Op, RejectCode,
};
use crate::organization::{
    ports::OrganizationRead,
    records::{self, Meta, Team, TeamAllocation, TeamStatus},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct CreateTeam {
    /// Minted by the caller, so a retry names the same team.
    pub id: Id,
    pub name: String,
    /// `None`: a top-level team.
    pub parent: Option<Id>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamCreated {
    pub id: Id,
    /// The organization-wide `TEAM-<number>`.
    pub number: u64,
    /// The directory generation the creation is.
    pub generation: u64,
}

impl Feature for CreateTeam {
    const OP: Op = Op::CreateTeam;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: OrganizationRead,
    {
        decide(uow, here, owner, by, self)
    }
}

/// Only the owner creates a team (document 04, "Who may change what").
pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &CreateTeam,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    stable(&command.id, "the team's ID")?;
    if let Some(parent) = &command.parent {
        stable(parent, "the parent team's ID")?;
    }
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let mut chain = Vec::new();
    if let Some(parent) = &command.parent {
        let Some(parent) = view.team(parent)? else {
            return reject(RejectCode::NotFound, "no such parent team");
        };
        let Some(above) = view.team_chain(&parent.id)? else {
            return invalid("the parent team has no chain of parents to a top-level team");
        };
        if !parent.is_active() {
            return reject(RejectCode::InvalidRequest, "the parent team is archived");
        }
        chain = above;
    }
    chain.push(command.id.clone());
    let path = team_file(&chain)?;
    if !records::is_line(&command.name) {
        return reject(RejectCode::InvalidName, "the name is one line of text");
    }
    let parent = command.parent.as_ref();
    if sibling_named(view, parent, &command.name, &command.id)? {
        let clash = "a team with that name already exists here";
        return reject(RejectCode::AlreadyExists, clash);
    }
    if id_in_use(view, &admitted.organization, &command.id)? {
        return reject(RejectCode::AlreadyExists, "the team's ID is in use");
    }
    let organization = &admitted.organization;
    let Some(counter) = view.team_counter()? else {
        return invalid("the organization has no team counter");
    };
    let counter = counter.advance()?;
    let number = counter.last_number;
    if view.team_by_number(number)?.is_some() {
        return invalid("the next team number is already a team's");
    }

    let team = Team {
        id: command.id.clone(),
        name: command.name.clone(),
        number,
        parent_team_id: command.parent.clone(),
        lead_employee_id: None,
        status: TeamStatus::Active,
        max_members: records::DEFAULT_MAX_MEMBERS,
        meta: Meta::created(&admitted.by, admitted.now),
    };
    let allocation = TeamAllocation {
        organization_id: organization.id.clone(),
        number,
        team_id: team.id.clone(),
        allocated_at: records::time_text(admitted.now),
    };
    let created = TeamCreated {
        id: team.id.clone(),
        number,
        generation: admitted.generation,
    };
    Ok(Change::new(Op::CreateTeam.ack())
        .put(path, records::encode_team(&team))
        .put(
            records::team_counter_path(&organization.id),
            records::encode_team_counter(&counter),
        )
        .create(
            records::team_allocation_path(&organization.id, number),
            records::encode_team_allocation(&allocation),
        )
        .reply(reply(&created)))
}
