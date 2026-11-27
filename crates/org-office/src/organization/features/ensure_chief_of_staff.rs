//! Provisioning the built-in Chief of Staff (Tess, `TASK-80.1`) in an organization created
//! before she existed: the idempotent bootstrap the HQ runs when it opens.
//!
//! An organization created by this build already has her ([`super::create_organization`]),
//! so this only ever adds her once. Her ID and the operation's ID are both derived from the
//! organization ID ([`crate::organization::chief_of_staff`]), so a retried bootstrap is the same
//! operation, answered from its receipt, and a rebuilt projection finds her profile on disk and
//! leaves her be. She is placed exactly as a creation places her: in the HR lead's (top-level)
//! team, reporting to the owner, with hiring power and no hire request. Like the HR lead at
//! creation she is part of the product, not a hire, so the team's `max_members` does not
//! refuse her.

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, Stream, UnitOfWork};
use serde::{Deserialize, Serialize};

use super::{
    authority::is_owner,
    invalid, reply,
    shared::{id_in_use, profile_file, reject},
    Feature, Op, RejectCode, NOT_AUTHORIZED,
};
use crate::organization::{
    chief_of_staff::{chief_of_staff_id, chief_of_staff_record, free_handle},
    owner::effective_owner_id,
    ports::{OrganizationRead, TeamDependents},
    records::{self, BuiltinKey, By, Meta, OfficeRole, Team},
};

/// Adds the Chief of Staff when the organization has none. The actor is the system (the
/// HQ's own bootstrap) or the owner.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize)]
pub struct EnsureChiefOfStaff {}

/// The reply: who she is now.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefOfStaffEnsured {
    pub employee_id: Id,
    pub handle: String,
    pub generation: u64,
}

impl Feature for EnsureChiefOfStaff {
    const OP: Op = Op::EnsureChiefOfStaff;

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
        let generation = uow.next(Stream::Organization)?;
        let now = uow.now();
        let view = uow.view();
        let Some(organization) = view.organization()? else {
            return invalid("there is no organization yet");
        };
        let office = view.office(&here.office_id)?;
        if !office.is_some_and(|office| office.role == OfficeRole::Hq) {
            return invalid("only HQ changes the organization's structure");
        }
        let owner = effective_owner_id(view, owner)?;
        let attributed = match by {
            Actor::System => By::System,
            Actor::Employee(actor) => {
                let actor = view.employee(actor)?;
                if !actor.is_some_and(|actor| is_owner(&owner, &actor)) {
                    return invalid(NOT_AUTHORIZED);
                }
                By::Employee(owner.clone())
            }
        };
        let employees = view.employees()?;
        if let Some(tess) = employees
            .iter()
            .find(|e| e.builtin_key == Some(BuiltinKey::ChiefOfStaff))
        {
            let message = format!("the Chief of Staff is already {}", tess.id);
            return reject(RejectCode::AlreadyExists, message);
        }
        let Some(team) = home_team(view, &employees, &owner)? else {
            return reject(
                RejectCode::NotFound,
                "no active top-level team for the Chief of Staff",
            );
        };
        let mut free_id = None;
        for attempt in 0..=u8::MAX {
            let id = chief_of_staff_id(&organization.id, attempt);
            if !id_in_use(view, &organization, &id)? {
                free_id = Some(id);
                break;
            }
        }
        let Some(id) = free_id else {
            return reject(
                RejectCode::AlreadyExists,
                "no ID is free for the Chief of Staff",
            );
        };
        let Some(handle) = free_handle(|handle| view.handle_in_use(handle))? else {
            return reject(
                RejectCode::AlreadyExists,
                "every handle of the Chief of Staff is taken",
            );
        };
        let Some(chain) = view.team_chain(&team.id)? else {
            return invalid("the team has no chain of parents to a top-level team");
        };
        let meta = Meta::created(&attributed, now);
        let tess = chief_of_staff_record(id, &handle, &team.id, meta);
        let path = profile_file(&chain, tess.kind, &tess.id)?;
        let ensured = ChiefOfStaffEnsured {
            employee_id: tess.id.clone(),
            handle: tess.handle.clone(),
            generation,
        };
        Ok(Change::new(Op::EnsureChiefOfStaff.ack())
            .put(path, records::encode_employee(&tess))
            .reply(reply(&ensured)))
    }
}

/// Where she lives: the HR lead's team when it is an active top-level team, else the owner's
/// home team's top-level team, else the active top-level team with the lowest number.
fn home_team(
    view: &impl OrganizationRead,
    employees: &[records::Employee],
    owner: &Id,
) -> Result<Option<Team>, org_store::EngineError> {
    let hr_team = employees
        .iter()
        .find(|e| e.builtin_key == Some(BuiltinKey::Hr))
        .map(|hr| hr.home_team_id.clone());
    let owner_team = employees
        .iter()
        .find(|e| e.id == *owner)
        .map(|person| person.home_team_id.clone());
    for team_id in hr_team.into_iter().chain(owner_team) {
        let top = view
            .team_chain(&team_id)?
            .and_then(|chain| chain.first().cloned());
        if let Some(team) = top.map(|id| view.team(&id)).transpose()?.flatten() {
            if team.is_active() && team.parent_team_id.is_none() {
                return Ok(Some(team));
            }
        }
    }
    Ok(view
        .teams()?
        .into_iter()
        .filter(|team| team.is_active() && team.parent_team_id.is_none())
        .min_by_key(|team| team.number))
}
