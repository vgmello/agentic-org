//! Changing an employee's `may_hire` permission (ADR-0029 decision 7 / HIRE-5).
//!
//! Only the owner or an active **person** above the employee in the hierarchy may turn `may_hire`
//! on or off. Nobody sets their own, an agent never sets anyone's, and the built-in HR lead
//! never has `may_hire`. The built-in Chief of Staff starts with it; the owner may revoke and
//! restore it, and she can never set it herself.

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    authority::{admit_active_employee, may_manage_employee},
    changed, invalid,
    shared::{check_revision, profile_file, reject, touched},
    stable, Feature, Op, RejectCode, NOT_AUTHORIZED,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{self, Employee, EmployeeKind, EmployeeStatus},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SetMayHire {
    pub id: Id,
    pub expected_revision: u64,
    pub may_hire: bool,
}

impl Feature for SetMayHire {
    const OP: Op = Op::SetMayHire;

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
    command: &SetMayHire,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    stable(&command.id, "the employee's ID")?;
    let admitted = admit_active_employee(uow, here, by)?;
    // Agents never set anyone's may_hire; nobody sets their own (HIRE-5).
    if admitted.actor.kind != EmployeeKind::Person || admitted.actor.id == command.id {
        return invalid(NOT_AUTHORIZED);
    }
    let view = uow.view();
    let Some(target) = view.employee(&command.id)? else {
        return reject(RejectCode::NotFound, "no such employee");
    };
    // A built-in member's hiring power (the Chief of Staff's reaches every team) is the
    // owner's alone to change (`TASK-80.1`).
    if !may_manage_employee(view, owner, &admitted.actor, &target)? {
        return invalid(NOT_AUTHORIZED);
    }
    if target.status == EmployeeStatus::Inactive {
        return reject(RejectCode::InvalidRequest, "the employee is inactive");
    }
    // The owner turns the Chief of Staff's hiring power off and on like anyone's; the HR
    // lead never has it (`TASK-80.1`).
    if let Some(key) = target.builtin_key.filter(|key| !key.may_hold_hiring()) {
        let reason = format!("{}'s hiring permission cannot be changed", key.described());
        return reject(RejectCode::InvalidRequest, reason);
    }
    check_revision(target.meta.revision, command.expected_revision)?;
    let Some(chain) = view.team_chain(&target.home_team_id)? else {
        return invalid("the home team has no chain of parents to a top-level team");
    };
    let updated = Employee {
        may_hire: command.may_hire,
        meta: touched(&target.meta, &admitted.by, admitted.now),
        ..target
    };
    let path = profile_file(&chain, updated.kind, &updated.id)?;
    let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
    Ok(Change::new(Op::SetMayHire.ack())
        .put(path, records::encode_employee(&updated))
        .reply(reply))
}
