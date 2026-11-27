//! Feature-slice admission and team-capacity checks for C2 hiring, `may_hire`, `max_members`,
//! and agent lifecycle features. Read-only authority predicates live in
//! [`crate::organization::authority`].

use org_proto::hiring::{format_team_full_hint, TeamFullHintInput, MAX_SUGGESTED_OPEN_TEAMS};
use org_store::{
    Actor, Engine, Id, OfficeIdentity, Refusal, StoreError, Stream, Timestamp, UnitOfWork,
};

pub(crate) use crate::organization::authority::is_owner;
pub use crate::organization::authority::{
    is_hiring_chief_of_staff, is_person_above_employee, is_person_ancestor_lead, may_address_hr,
    may_hire_anywhere, may_hire_into, may_manage_employee, team_active_member_count, HIRE_DENIED,
};
use crate::organization::{
    ports::OrganizationRead,
    records::{By, Employee, EmployeeKind, EmployeeStatus, OfficeRole, Organization, Team},
};

use super::{
    invalid,
    shared::{named, reject_with_hint},
    RejectCode, NOT_AUTHORIZED,
};

/// Which operation triggered a `max_members` capacity check (governs authorized alternative teams).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CapacityOp {
    Hire,
    Move,
}

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

fn caller_may_target_team(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
    team_id: &Id,
    op: CapacityOp,
) -> Result<bool, StoreError> {
    match op {
        CapacityOp::Hire => may_hire_into(view, owner, actor, team_id),
        CapacityOp::Move => {
            Ok(is_owner(owner, actor) || is_person_ancestor_lead(view, actor, team_id, true)?)
        }
    }
}

type OpenTeamSlot = (String, String, u64, u64);

fn authorized_open_teams(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
    full_team_id: &Id,
    op: CapacityOp,
) -> Result<(Vec<OpenTeamSlot>, usize), StoreError> {
    let mut open = Vec::new();
    for candidate in view.teams()? {
        if candidate.id == *full_team_id || !candidate.is_active() {
            continue;
        }
        if !caller_may_target_team(view, owner, actor, &candidate.id, op)? {
            continue;
        }
        let used = team_active_member_count(view, &candidate.id)?;
        if used < candidate.max_members {
            open.push((
                candidate.name,
                candidate.id.as_str().to_owned(),
                used,
                candidate.max_members,
            ));
        }
    }
    open.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let extra = open.len().saturating_sub(MAX_SUGGESTED_OPEN_TEAMS);
    open.truncate(MAX_SUGGESTED_OPEN_TEAMS);
    Ok((open, extra))
}

/// Enforces `team.max_members` (ADR-0029 decision 8 / HIRE-6), returning an actionable hint listing
/// caller-authorized open teams, sub-team creation, and direct/requested limit increases.
pub(crate) fn check_team_capacity(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
    team: &Team,
    op: CapacityOp,
) -> Result<(), Refusal> {
    let count = team_active_member_count(view, &team.id)?;
    if count < team.max_members {
        return Ok(());
    }
    let msg = format!(
        "{} is full ({count} of {} members; max_members limit reached)",
        named(team),
        team.max_members
    );
    let (open_teams, extra_open_count) = authorized_open_teams(view, owner, actor, &team.id, op)?;
    let can_set_directly = actor.kind == EmployeeKind::Person
        && (is_owner(owner, actor) || is_person_ancestor_lead(view, actor, &team.id, true)?);
    let can_request_increase =
        team.lead_employee_id.as_ref() == Some(&actor.id) && !is_owner(owner, actor);
    let hint = format_team_full_hint(&TeamFullHintInput {
        team_id: team.id.as_str(),
        current_max: team.max_members,
        team_revision: team.meta.revision,
        open_teams: &open_teams,
        extra_open_count,
        can_set_directly,
        can_request_increase,
    });
    reject_with_hint(RejectCode::InvalidRequest, msg, hint)
}
