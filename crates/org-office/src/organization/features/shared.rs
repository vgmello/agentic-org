//! What every structure change shares: who may make it, how a rule refuses it, and how a
//! team's subtree moves.

use org_store::{Actor, DurablePath, Engine, Id, OfficeIdentity, Refusal, Stream, Timestamp};
use org_store::{StoreError, UnitOfWork};

use super::{invalid, reply, OrgRejection, RejectCode, NOT_AUTHORIZED};
use crate::organization::{
    names::name_key,
    ports::{OrganizationRead, MAX_TEAM_DEPTH},
    records::{self, By, EmployeeKind, EmployeeStatus, Meta, OfficeRole, Organization, Team},
};

/// A rule refused the change: recorded with its operation, so a retry gets this answer.
pub(super) fn reject<T>(code: RejectCode, message: impl Into<String>) -> Result<T, Refusal> {
    let rejection = OrgRejection {
        code,
        message: message.into(),
    };
    Err(Refusal::rejected(reply(&rejection)))
}

/// What an admitted change knows before it reads its records.
pub(super) struct Admitted {
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
pub(super) fn admit<E: Engine>(
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

/// `meta` after one more change by `by` at `now`.
pub(super) fn touched(meta: &Meta, by: &By, now: Timestamp) -> Meta {
    Meta {
        revision: meta.revision + 1,
        updated_at: Some(records::time_text(now)),
        updated_by: by.clone(),
        ..meta.clone()
    }
}

/// A stale `expected_revision`: told after every other rule but `already_exists`.
pub(super) fn check_revision(current: u64, expected: u64) -> Result<(), Refusal> {
    if current == expected {
        return Ok(());
    }
    reject(
        RejectCode::Conflict,
        "it changed since you last looked; refresh and try again",
    )
}

/// Whether `id` names any record of the organization, whatever its kind.
pub(super) fn id_in_use(
    view: &impl OrganizationRead,
    organization: &Organization,
    id: &Id,
) -> Result<bool, StoreError> {
    Ok(*id == organization.id
        || view.team(id)?.is_some()
        || view.office(id)?.is_some()
        || view.employee(id)?.is_some()
        || view.role(id)?.is_some()
        || view.repositories()?.iter().any(|r| r.id == *id))
}

/// Whether another active team under `parent` (none: the top level) has `name`, ignoring
/// case. Archived teams keep their names without blocking them.
pub(super) fn sibling_named(
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
/// deep for the limit or for a path.
pub(super) fn team_file(chain: &[Id]) -> Result<DurablePath, Refusal> {
    if chain.len() > MAX_TEAM_DEPTH {
        return reject(RejectCode::InvalidRequest, TOO_DEEP);
    }
    records::team_path(chain).or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))
}

/// How one file moves: from where, to where, with what bytes.
pub(super) struct Moved {
    pub from: DurablePath,
    pub to: DurablePath,
    pub bytes: Vec<u8>,
}

/// Every file below the team at the end of `old` once that team's chain is `new`: each
/// descendant team's file and every profile homed in the team or below it, unchanged but
/// for the path. The team's own file is the caller's, since its record changes.
pub(super) fn relocate(
    view: &impl OrganizationRead,
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
            to: profile(to).or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?,
            bytes: records::encode_employee(&employee),
        });
    }
    Ok(moved)
}

/// The readable name of a team in a message: `TEAM-<number> (<name>)`.
pub(super) fn named(team: &Team) -> String {
    format!("{} ({})", records::team_key(team.number), team.name)
}

/// The team an employee leads, if any. A lead's home is the team, so there is one at most.
pub(super) fn led_by(view: &impl OrganizationRead, employee: &Id) -> Result<Option<Team>, StoreError> {
    let teams = view.teams()?;
    Ok(teams
        .into_iter()
        .find(|team| team.lead_employee_id.as_ref() == Some(employee)))
}
