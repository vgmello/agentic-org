//! What every structure change shares: who may make it, how a rule refuses it, and how a
//! team's subtree moves.

use org_store::{Actor, DurablePath, Engine, Id, OfficeIdentity, Refusal, Stream, Timestamp};
use org_store::{StoreError, UnitOfWork};

use super::super::ops::RejectCode;
pub(crate) use super::super::ops::{check_revision, checked_dod_items, reject, touched};
use super::{invalid, NOT_AUTHORIZED};
use crate::organization::{
    names::name_key,
    ports::{OrganizationRead, TeamDependents, MAX_TEAM_DEPTH},
    records::{self, By, EmployeeKind, EmployeeStatus, OfficeRole, Organization, Team},
};

/// What an admitted change knows before it reads its records.
pub(crate) struct Admitted {
    /// The directory generation the change becomes.
    pub generation: u64,
    pub now: Timestamp,
    pub organization: Organization,
    /// The owner, as records name who changed them.
    pub by: By,
}

/// Admits a structure change at HQ from the organization's owner (document 04, "Who may
/// change what"): `by` must be the employee `owner`, an active person on record. The
/// caller's grant authority has already checked the connection; this is the
/// organization's own check. Anything else is refused unrecorded.
pub(crate) fn admit<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
) -> Result<Admitted, Refusal>
where
    E::Tx: OrganizationRead,
{
    let generation = uow.next(Stream::Organization)?;
    let now = uow.now();
    let view = uow.view();
    let Some(organization) = view.organization()? else {
        return invalid("there is no organization yet");
    };
    // HQ alone changes the structure: a satellite's copy is not an authority.
    let office = view.office(&here.office_id)?;
    if !office.is_some_and(|office| office.role == OfficeRole::Hq) {
        return invalid("only HQ changes the organization's structure");
    }
    let Actor::Employee(actor) = by else {
        return invalid(NOT_AUTHORIZED);
    };
    let person = view.employee(owner)?.filter(|person| {
        person.kind == EmployeeKind::Person && person.status == EmployeeStatus::Active
    });
    if actor != owner || person.is_none() {
        return invalid(NOT_AUTHORIZED);
    }
    Ok(Admitted {
        generation,
        now,
        organization,
        by: By::Employee(owner.clone()),
    })
}

/// Whether `id` names any record of the organization, whatever its kind.
pub(crate) fn id_in_use(
    view: &impl TeamDependents,
    organization: &Organization,
    id: &Id,
) -> Result<bool, StoreError> {
    Ok(*id == organization.id
        || view.team(id)?.is_some()
        || view.office(id)?.is_some()
        || view.employee(id)?.is_some()
        || view.role(id)?.is_some()
        || view.hire_request(id)?.is_some()
        || view.provider_connection(id)?.is_some()
        || view.model_profile(id)?.is_some()
        || view.project_exists(id)?
        || view.repositories()?.iter().any(|r| r.id == *id))
}

/// Whether another active team under `parent` (none: the top level) has `name`, ignoring
/// case. Archived teams keep their names without blocking them.
pub(crate) fn sibling_named(
    view: &impl OrganizationRead,
    parent: Option<&Id>,
    name: &str,
    except: &Id,
) -> Result<bool, StoreError> {
    let key = name_key(name);
    let teams = view.teams()?;
    Ok(teams.iter().any(|team| {
        team.id != *except
            && team.is_active()
            && team.parent_team_id.as_ref() == parent
            && name_key(&team.name) == key
    }))
}

pub(super) const TOO_DEEP: &str = "the team tree would be too deep";

/// The file of the team at the end of `chain`, or the rule's refusal when the tree is too
/// deep for the limit or for a path. A team is only where an employee's profile also fits
/// (its path is the longer), so every team can take a person or an agent.
pub(crate) fn team_file(chain: &[Id]) -> Result<DurablePath, Refusal> {
    let Some(last) = chain.last() else {
        return invalid("a team path names a team");
    };
    // Every stable ID has the same length: the team's own stands in for any employee's.
    let profiles_fit = [EmployeeKind::Person, EmployeeKind::Agent]
        .into_iter()
        .all(|kind| records::employee_path(chain, kind, last).is_ok());
    if chain.len() > MAX_TEAM_DEPTH || !profiles_fit {
        return reject(RejectCode::InvalidRequest, TOO_DEEP);
    }
    records::team_path(chain).or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))
}

/// The profile of `employee` homed at the end of `chain`. A team is only made where its
/// profiles fit ([`team_file`]); a tree on record deeper than that is refused by the rule.
pub(super) fn profile_file(
    chain: &[Id],
    kind: EmployeeKind,
    employee: &Id,
) -> Result<DurablePath, Refusal> {
    records::employee_path(chain, kind, employee)
        .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))
}

/// How one file moves: from where, to where, with what bytes.
pub(super) struct Moved {
    pub from: DurablePath,
    pub to: DurablePath,
    pub bytes: Vec<u8>,
}

/// Every file below the team at the end of `old` once that team's chain is `new`: each
/// descendant team's file, every profile homed in the team or below it, and every team
/// process/standard document in the moved subtree, unchanged but for the path.
/// The team's own file is the caller's, since its record changes.
pub(super) fn relocate(
    view: &impl TeamDependents,
    old: &[Id],
    new: &[Id],
) -> Result<Vec<Moved>, Refusal> {
    let mut moved = Vec::new();
    let mut chains = std::collections::BTreeMap::new();
    for team in view.teams()? {
        let Some(chain) = view.team_chain(&team.id)? else {
            return invalid("a team has no chain of parents to a top-level team");
        };
        let Some(below) = chain.strip_prefix(old) else {
            continue;
        };
        let to: Vec<Id> = new.iter().chain(below).cloned().collect();
        if !below.is_empty() {
            moved.push(Moved {
                from: records::team_path(&chain)?,
                to: team_file(&to)?,
                bytes: records::encode_team(&team),
            });
        }
        chains.insert(team.id, (chain, to));
    }
    for employee in view.employees()? {
        let Some((from, to)) = chains.get(&employee.home_team_id) else {
            continue;
        };
        let profile = |chain: &[Id]| records::employee_path(chain, employee.kind, &employee.id);
        moved.push(Moved {
            from: profile(from)?,
            to: profile_file(to, employee.kind, &employee.id)?,
            bytes: records::encode_employee(&employee),
        });
        if employee.kind == EmployeeKind::Agent {
            if let Some(exec) = view.agent_execution(&employee.id)? {
                moved.push(Moved {
                    from: records::agent_execution_path(from, &employee.id)?,
                    to: records::agent_execution_path(to, &employee.id)
                        .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?,
                    bytes: records::encode_agent_execution(&exec),
                });
            }
        }
    }
    for req in view.hire_requests()? {
        let Some((from, to)) = chains.get(&req.home_team_id) else {
            continue;
        };
        moved.push(Moved {
            from: records::hire_request_path(from, &req.id)?,
            to: records::hire_request_path(to, &req.id)
                .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?,
            bytes: records::encode_hire_request(&req),
        });
    }
    for homed in view.team_homed_moves(&chains)? {
        moved.push(Moved {
            from: homed.from?,
            to: match homed.to {
                Some(to) => to,
                None => return reject(RejectCode::InvalidRequest, TOO_DEEP),
            },
            bytes: homed.bytes,
        });
    }
    Ok(moved)
}

/// The readable name of a team in a message: `TEAM-<number> (<name>)`.
pub(super) fn named(team: &Team) -> String {
    format!("{} ({})", records::team_key(team.number), team.name)
}

/// The team an employee leads, if any. A lead's home is the team, so there is one at most.
pub(super) fn led_by(
    view: &impl OrganizationRead,
    employee: &Id,
) -> Result<Option<Team>, StoreError> {
    let teams = view.teams()?;
    Ok(teams
        .into_iter()
        .find(|team| team.lead_employee_id.as_ref() == Some(employee)))
}
