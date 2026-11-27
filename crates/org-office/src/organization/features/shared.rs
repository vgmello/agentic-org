//! What every organization structure change shares: re-exports of [`super::super::ops`]
//! admission/refusal primitives, plus subtree relocation for `MoveTeam`.

use org_store::{DurablePath, Id, Refusal, StoreError};

pub(crate) use super::super::ops::{
    admit, check_revision, checked_dod_items, id_in_use, reject, reject_with_hint, sibling_named,
    team_file, touched, RejectCode, TOO_DEEP,
};
use super::invalid;
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{self, EmployeeKind, Team},
};

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
    for req in view.team_limit_requests()? {
        let Some((from, to)) = chains.get(&req.team_id) else {
            continue;
        };
        moved.push(Moved {
            from: records::team_limit_request_path(from, &req.id)?,
            to: records::team_limit_request_path(to, &req.id)
                .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?,
            bytes: records::encode_team_limit_request(&req),
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
