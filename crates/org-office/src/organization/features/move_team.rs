//! Moving a team under another. The team keeps its ID and its number; its file, every
//! team below it and every profile homed in its subtree move to the folders the new chain
//! names, in one entry, and nothing is left at the old paths (document 01). Employees keep
//! their home teams, so no lead appointment is touched.

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    changed, invalid,
    shared::{admit, check_revision, reject, relocate, sibling_named, team_file, touched},
    Feature, Op, RejectCode,
};
use crate::organization::{
    ports::OrganizationRead,
    records::{self, Team},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct MoveTeam {
    pub id: Id,
    pub expected_revision: u64,
    /// The team it goes under.
    pub parent: Id,
}

impl Feature for MoveTeam {
    const OP: Op = Op::MoveTeam;

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

pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &MoveTeam,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let Some(team) = view.team(&command.id)? else {
        return reject(RejectCode::NotFound, "no such team");
    };
    let Some(parent) = view.team(&command.parent)? else {
        return reject(RejectCode::NotFound, "no such parent team");
    };
    // The team made with the organization is its top: never moved (owner decision Q1).
    if team.number == 1 {
        let rule = "the organization's first team stays at the top level";
        return reject(RejectCode::InvalidRequest, rule);
    }
    if !team.is_active() {
        return reject(RejectCode::InvalidRequest, "the team is archived");
    }
    if !parent.is_active() {
        return reject(RejectCode::InvalidRequest, "the parent team is archived");
    }
    let (Some(old), Some(above)) = (view.team_chain(&team.id)?, view.team_chain(&parent.id)?)
    else {
        return invalid("a team has no chain of parents to a top-level team");
    };
    if above.contains(&team.id) {
        let rule = "a team cannot move under itself or a team below it";
        return reject(RejectCode::Cycle, rule);
    }
    let mut new = above;
    new.push(team.id.clone());
    let path = team_file(&new)?;
    // Every file below moves with it; one that would be too deep refuses the move.
    let moved = relocate(view, &old, &new)?;
    check_revision(team.meta.revision, command.expected_revision)?;
    if team.parent_team_id.as_ref() == Some(&parent.id) {
        return reject(RejectCode::AlreadyExists, "the team is already there");
    }
    if sibling_named(view, Some(&parent.id), &team.name, &team.id)? {
        let clash = "a team with that name already exists there";
        return reject(RejectCode::AlreadyExists, clash);
    }

    let from = records::team_path(&old)?;
    let updated = Team {
        parent_team_id: Some(parent.id),
        meta: touched(&team.meta, &admitted.by, admitted.now),
        ..team
    };
    // Removals first: a put of the same record at its new path then stands.
    let mut change = Change::new(Op::MoveTeam.ack()).remove(from);
    for file in &moved {
        change = change.remove(file.from.clone());
    }
    change = change.put(path, records::encode_team(&updated));
    for file in moved {
        change = change.put(file.to, file.bytes);
    }
    let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
    Ok(change.reply(reply))
}
