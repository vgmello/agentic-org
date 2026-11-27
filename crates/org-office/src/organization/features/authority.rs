//! Feature-slice admission and team-capacity checks for C2 hiring, `may_hire`, `max_members`,
//! and agent lifecycle features. Read-only authority predicates live in
//! [`crate::organization::authority`].

use org_store::{Actor, Engine, OfficeIdentity, Refusal, Stream, Timestamp, UnitOfWork};

pub(crate) use crate::organization::authority::is_owner;
pub use crate::organization::authority::{
    is_hiring_chief_of_staff, is_person_above_employee, is_person_ancestor_lead, may_address_hr,
    may_hire_anywhere, may_hire_into, may_manage_employee, team_active_member_count, HIRE_DENIED,
};
use crate::organization::{
    ports::OrganizationRead,
    records::{By, Employee, EmployeeStatus, OfficeRole, Organization, Team},
};

use super::{
    invalid,
    shared::{named, reject},
    RejectCode, NOT_AUTHORIZED,
};

/// Admitted context for any active employee (person or agent) at HQ.
pub(crate) struct AdmittedActor {
    pub generation: u64,
    pub now: Timestamp,
    pub organization: Organization,
    pub actor: Employee,
    pub by: By,
}

/// Admits an operation at HQ from any active employee (person or agent) on record.
/// Individual features then enforce their exact ADR-0029 / document-04 authority rules.
pub(crate) fn admit_active_employee<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    by: &Actor,
) -> Result<AdmittedActor, Refusal>
where
    E::Tx: OrganizationRead,
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
    let Actor::Employee(actor_id) = by else {
        return invalid(NOT_AUTHORIZED);
    };
    let Some(actor) = view.employee(actor_id)? else {
        return invalid(NOT_AUTHORIZED);
    };
    if actor.status != EmployeeStatus::Active {
        return invalid(NOT_AUTHORIZED);
    }
    Ok(AdmittedActor {
        generation,
        now,
        organization,
        by: By::Employee(actor.id.clone()),
        actor,
    })
}

/// Enforces `team.max_members` (ADR-0029 decision 8 / HIRE-6), naming the limit in the refusal.
pub(crate) fn check_team_capacity(
    view: &impl OrganizationRead,
    team: &Team,
) -> Result<(), Refusal> {
    let count = team_active_member_count(view, &team.id)?;
    if count >= team.max_members {
        let msg = format!(
            "{} is full ({count} of {} members; max_members limit reached)",
            named(team),
            team.max_members
        );
        return reject(RejectCode::InvalidRequest, msg);
    }
    Ok(())
}
