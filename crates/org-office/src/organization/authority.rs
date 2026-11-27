//! Domain-public read-only authorization and membership predicates for the `organization`
//! family (`ADR-0029`, document-04, `TASK-80.1`).
//!
//! Lives outside `organization::features` so sibling domains (`messaging`, `tasks`, `processes`)
//! and office facades query organization authority without reaching into feature slices.

use org_proto::ErrorCode;
use org_store::{Id, Refusal, StoreError};

use crate::error::{api_refusal, storage_api_refusal};

use super::{
    ports::OrganizationRead,
    records::{BuiltinKey, Employee, EmployeeKind, EmployeeStatus, Team},
};

/// Exact ADR-0029 decision 3 refusal text for callers who may not hire.
pub const HIRE_DENIED: &str = "You don't have access to hire. Ask your team lead.";

/// Looks up `member_id` as an active organization employee, refusing unknown, malformed,
/// paused, or inactive callers with `NotAuthorized`.
pub(crate) fn require_active_employee<R: OrganizationRead>(
    view: &R,
    member_id: &str,
    inactive_message: &str,
) -> Result<Employee, Refusal> {
    let Ok(id) = Id::parse(member_id) else {
        return Err(api_refusal(
            ErrorCode::NotAuthorized,
            "caller is not a valid organization employee",
        ));
    };
    let emp = view
        .employee(&id)
        .map_err(|err| storage_api_refusal("storage error", err))?
        .ok_or_else(|| api_refusal(ErrorCode::NotAuthorized, "caller is not an employee"))?;
    if emp.status != EmployeeStatus::Active {
        return Err(api_refusal(ErrorCode::NotAuthorized, inactive_message));
    }
    Ok(emp)
}

/// Whether `actor` is the organization's active person owner.
pub(crate) fn is_owner(owner: &Id, actor: &Employee) -> bool {
    actor.id == *owner
        && actor.kind == EmployeeKind::Person
        && actor.status == EmployeeStatus::Active
}

/// Whether `actor` is the active built-in Chief of Staff holding hiring power: the one agent
/// whose `may_hire` reaches every team of the organization (`TASK-80.1`, `ADR-0029`). With
/// `may_hire` revoked by the owner she hires nowhere.
pub fn is_hiring_chief_of_staff(actor: &Employee) -> bool {
    actor.builtin_key == Some(BuiltinKey::ChiefOfStaff)
        && actor.kind == EmployeeKind::Agent
        && actor.status == EmployeeStatus::Active
        && actor.may_hire
}

/// Whether `actor` may hire into `team_id` (ADR-0029 decision 2):
/// - the organization owner into any team, OR
/// - the built-in Chief of Staff with `may_hire == true` into any active team of the
///   organization ([`is_hiring_chief_of_staff`]), OR
/// - an active employee (person or agent) with `may_hire == true` who leads `team_id` or an
///   ancestor team above `team_id`.
///
/// Every hire still runs the normal flow's provisioning, `max_members` and active-team checks.
pub fn may_hire_into(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
    team_id: &Id,
) -> Result<bool, StoreError> {
    if is_owner(owner, actor) {
        return Ok(true);
    }
    if actor.status != EmployeeStatus::Active || !actor.may_hire {
        return Ok(false);
    }
    if is_hiring_chief_of_staff(actor) {
        return Ok(view.team(team_id)?.is_some_and(|team| team.is_active()));
    }
    let Some(chain) = view.team_chain(team_id)? else {
        return Ok(false);
    };
    for ancestor_id in chain {
        if let Some(team) = view.team(&ancestor_id)? {
            if team.is_active() && team.lead_employee_id.as_ref() == Some(&actor.id) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Whether `actor` may hire into at least one active team in the organization (used to gate
/// direct messages and mentions to the built-in HR lead per ADR-0029 decision 3).
pub fn may_hire_anywhere(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
) -> Result<bool, StoreError> {
    if is_owner(owner, actor) {
        return Ok(true);
    }
    if actor.status != EmployeeStatus::Active || !actor.may_hire {
        return Ok(false);
    }
    let teams = view.teams()?;
    if is_hiring_chief_of_staff(actor) {
        return Ok(teams.iter().any(Team::is_active));
    }
    Ok(teams
        .iter()
        .any(|t| t.is_active() && t.lead_employee_id.as_ref() == Some(&actor.id)))
}

/// Whether `actor` is an active **person** who leads an ancestor team of `team_id` (including
/// `team_id` itself when `include_same_team` is true). Agents ALWAYS return `false`.
pub fn is_person_ancestor_lead(
    view: &impl OrganizationRead,
    actor: &Employee,
    team_id: &Id,
    include_same_team: bool,
) -> Result<bool, StoreError> {
    if actor.kind != EmployeeKind::Person || actor.status != EmployeeStatus::Active {
        return Ok(false);
    }
    let Some(mut chain) = view.team_chain(team_id)? else {
        return Ok(false);
    };
    if !include_same_team {
        chain.pop();
    }
    for ancestor_id in chain {
        if let Some(team) = view.team(&ancestor_id)? {
            if team.is_active() && team.lead_employee_id.as_ref() == Some(&actor.id) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Whether `actor` is the owner or an active person above `target` in the team hierarchy
/// (either leading a strict ancestor team of `target.home_team_id`, or leading
/// `target.home_team_id` when `target` is not that team's lead).
pub fn is_person_above_employee(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
    target: &Employee,
) -> Result<bool, StoreError> {
    if is_owner(owner, actor) {
        return Ok(true);
    }
    if actor.kind != EmployeeKind::Person || actor.status != EmployeeStatus::Active {
        return Ok(false);
    }
    let target_is_own_lead = view
        .team(&target.home_team_id)?
        .is_some_and(|t| t.lead_employee_id.as_ref() == Some(&target.id));
    is_person_ancestor_lead(view, actor, &target.home_team_id, !target_is_own_lead)
}

/// Whether `sender` may address or `@mention` the built-in HR lead (`HIRE-2`, ADR-0029
/// decision 3): an active employee who may hire somewhere. The built-in Chief of Staff never
/// may, whatever her own `may_hire`: she acts for members who may not hire and for wakes that
/// act for nobody, so her reach would open HR to them; she hires through the hire flow,
/// which checks the member she acts for (`TASK-80.1`).
pub fn may_address_hr(
    view: &impl OrganizationRead,
    owner: &Id,
    sender: &Employee,
) -> Result<bool, StoreError> {
    if sender.builtin_key == Some(BuiltinKey::ChiefOfStaff) {
        return Ok(false);
    }
    may_hire_anywhere(view, owner, sender)
}

/// Whether `actor` may change `target`'s profile, execution configuration or hiring power:
/// a person above the employee ([`is_person_above_employee`]); for a built-in member (the HR
/// lead, the Chief of Staff) the organization's owner alone, so no team lead edits her, re-runs
/// her on another configuration or toggles her organization-wide `may_hire` (`TASK-80.1`).
pub fn may_manage_employee(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
    target: &Employee,
) -> Result<bool, StoreError> {
    if target.builtin_key.is_some() {
        return Ok(is_owner(owner, actor));
    }
    is_person_above_employee(view, owner, actor, target)
}

/// Active member count of `team_id` (every non-inactive employee, people and agents), less
/// the built-in Chief of Staff: she was added to every organization after its teams were sized
/// (`TASK-80.1`), so she never takes a slot a team's `max_members` was set for.
pub fn team_active_member_count(
    view: &impl OrganizationRead,
    team_id: &Id,
) -> Result<u64, StoreError> {
    let count = view
        .employees()?
        .iter()
        .filter(|e| e.home_team_id == *team_id && e.status != EmployeeStatus::Inactive)
        .filter(|e| e.builtin_key != Some(BuiltinKey::ChiefOfStaff))
        .count();
    Ok(count as u64)
}
