//! Setting a team's `max_members` limit (ADR-0029 decision 8 / HIRE-6) and manager-approved
//! `team.max_members` increase requests (`TASK-210`).

use std::collections::BTreeSet;

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, StoreError, UnitOfWork};
use serde::{Deserialize, Serialize};

use super::{
    authority::{admit_active_employee, is_owner, is_person_ancestor_lead},
    changed, invalid, reply,
    shared::{check_revision, id_in_use, reject, reject_with_hint, team_file, touched, TOO_DEEP},
    stable, Feature, Op, RejectCode, NOT_AUTHORIZED,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents, MAX_TEAM_DEPTH},
    records::{
        self, Employee, EmployeeKind, EmployeeStatus, Team, TeamLimitRequest, TeamLimitRequestState,
    },
};

pub const TEAM_LIMIT_REQUEST_DENIED: &str =
    "only the team's active lead may request a max_members increase";
pub const TEAM_LIMIT_DECIDE_DENIED: &str =
    "only the team lead's manager or the organization owner may decide this team limit request";

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SetTeamMaxMembers {
    pub id: Id,
    pub expected_revision: u64,
    pub max_members: u64,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct RequestTeamLimitIncrease {
    pub id: Id,
    pub team_id: Id,
    pub requested_max_members: u64,
    pub reason: String,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct DecideTeamLimitRequest {
    pub request_id: Id,
    pub expected_revision: u64,
    pub approve: bool,
    pub reason: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamLimitChanged {
    pub request_id: Id,
    pub team_id: Id,
    pub state: TeamLimitRequestState,
    pub approver_employee_id: Id,
    pub requested_max_members: u64,
    pub revision: u64,
    pub team_revision: Option<u64>,
    pub generation: u64,
}

macro_rules! impl_limit_feature {
    ($ty:ty, $op:expr, $handler:ident) => {
        impl Feature for $ty {
            const OP: Op = $op;
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
                $handler(uow, here, owner, by, self)
            }
        }
    };
}

impl_limit_feature!(SetTeamMaxMembers, Op::SetTeamMaxMembers, decide);
impl_limit_feature!(
    RequestTeamLimitIncrease,
    Op::RequestTeamLimitIncrease,
    decide_request
);
impl_limit_feature!(
    DecideTeamLimitRequest,
    Op::DecideTeamLimitRequest,
    decide_approval
);

fn is_active_person(emp: &Employee) -> bool {
    emp.kind == EmployeeKind::Person && emp.status == EmployeeStatus::Active
}

fn validate_team_and_limit(team: &Team, max_members: u64) -> Result<(), Refusal> {
    if !team.is_active() {
        return reject(RejectCode::InvalidRequest, "the team is archived");
    }
    if !(1..=100).contains(&max_members) {
        return reject(
            RejectCode::InvalidRequest,
            "max_members must be between 1 and 100",
        );
    }
    Ok(())
}

fn load_team_and_chain(view: &impl OrganizationRead, id: &Id) -> Result<(Team, Vec<Id>), Refusal> {
    let Some(team) = view.team(id)? else {
        return reject(RejectCode::NotFound, "no such team");
    };
    let Some(chain) = view.team_chain(&team.id)? else {
        return invalid("the team has no chain of parents to a top-level team");
    };
    Ok((team, chain))
}

fn parse_reason(raw: &str, label: &str) -> Result<String, Refusal> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || !records::is_line(trimmed) {
        return reject(
            RejectCode::InvalidRequest,
            format!("{label} must be a non-empty single line of text"),
        );
    }
    Ok(trimmed.to_owned())
}

/// Walks `employee_id`'s `reports_to` chain and returns the first active `Person` matching `pick`.
fn find_person_in_reports_to(
    view: &impl OrganizationRead,
    start: &Id,
    first_mgr: Option<Id>,
    mut pick: impl FnMut(&Employee) -> bool,
) -> Result<Option<Id>, StoreError> {
    let mut visited = BTreeSet::from([start.clone()]);
    let mut cursor = first_mgr;
    let mut hops = 0;
    while let Some(mgr_id) = cursor {
        if hops >= MAX_TEAM_DEPTH || !visited.insert(mgr_id.clone()) {
            break;
        }
        hops += 1;
        let Some(mgr) = view.employee(&mgr_id)? else {
            break;
        };
        if is_active_person(&mgr) && pick(&mgr) {
            return Ok(Some(mgr.id));
        }
        cursor = mgr.reports_to_employee_id;
    }
    Ok(None)
}

/// Whether `candidate_id` is an active person strictly above `employee_id` in `reports_to`.
pub(crate) fn is_person_in_reports_to_chain(
    view: &impl OrganizationRead,
    employee_id: &Id,
    candidate_id: &Id,
) -> Result<bool, StoreError> {
    if employee_id == candidate_id {
        return Ok(false);
    }
    let first = view
        .employee(employee_id)?
        .and_then(|e| e.reports_to_employee_id);
    Ok(find_person_in_reports_to(view, employee_id, first, |m| m.id == *candidate_id)?.is_some())
}

/// Resolves the team lead's manager who approves a `max_members` increase:
/// 1. Nearest active `Person` in `requester.reports_to_employee_id`,
/// 2. Nearest active `Person` lead walking up `team.parent_team_id`,
/// 3. Active `Person` organization owner (`!= requester`).
fn resolve_limit_approver(
    view: &impl OrganizationRead,
    owner: &Id,
    requester: &Employee,
    chain: &[Id],
) -> Result<Option<Id>, StoreError> {
    if let Some(mgr) = find_person_in_reports_to(
        view,
        &requester.id,
        requester.reports_to_employee_id.clone(),
        |_| true,
    )? {
        return Ok(Some(mgr));
    }
    for ancestor_id in chain[..chain.len().saturating_sub(1)].iter().rev() {
        if let Some(lead_id) = view.team(ancestor_id)?.and_then(|t| t.lead_employee_id) {
            if lead_id != requester.id
                && view
                    .employee(&lead_id)?
                    .is_some_and(|e| is_active_person(&e))
            {
                return Ok(Some(lead_id));
            }
        }
    }
    if requester.id != *owner && view.employee(owner)?.is_some_and(|e| is_active_person(&e)) {
        return Ok(Some(owner.clone()));
    }
    Ok(None)
}

/// Whether `actor` may approve or deny `req` right now (live hierarchy check; fail closed).
pub(crate) fn may_decide_limit_request(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
    req: &TeamLimitRequest,
) -> Result<bool, StoreError> {
    if !is_active_person(actor) {
        return Ok(false);
    }
    if is_owner(owner, actor) {
        return Ok(true);
    }
    if actor.id == req.requested_by_employee_id {
        return Ok(false);
    }
    if is_person_ancestor_lead(view, actor, &req.team_id, false)? {
        return Ok(true);
    }
    is_person_in_reports_to_chain(view, &req.requested_by_employee_id, &actor.id)
}

/// Whether `actor` may inspect `req` in `team.limit_request.list`.
pub(crate) fn can_view_limit_request(
    view: &impl OrganizationRead,
    owner: &Id,
    actor: &Employee,
    req: &TeamLimitRequest,
) -> Result<bool, StoreError> {
    if actor.status != EmployeeStatus::Active {
        return Ok(false);
    }
    if is_owner(owner, actor)
        || actor.id == req.requested_by_employee_id
        || actor.id == req.approver_employee_id
    {
        return Ok(true);
    }
    may_decide_limit_request(view, owner, actor, req)
}

fn limit_reply(req: &TeamLimitRequest, team_revision: Option<u64>, generation: u64) -> Vec<u8> {
    reply(&TeamLimitChanged {
        request_id: req.id.clone(),
        team_id: req.team_id.clone(),
        state: req.state,
        approver_employee_id: req.approver_employee_id.clone(),
        requested_max_members: req.requested_max_members,
        revision: req.revision,
        team_revision,
        generation,
    })
}

pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &SetTeamMaxMembers,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    stable(&command.id, "the team's ID")?;
    let admitted = admit_active_employee(uow, here, by)?;
    if admitted.actor.kind != EmployeeKind::Person {
        return invalid(NOT_AUTHORIZED);
    }
    let view = uow.view();
    let (team, chain) = load_team_and_chain(view, &command.id)?;
    let allowed = is_owner(owner, &admitted.actor)
        || is_person_ancestor_lead(view, &admitted.actor, &team.id, true)?;
    if !allowed {
        return invalid(NOT_AUTHORIZED);
    }
    validate_team_and_limit(&team, command.max_members)?;
    check_revision(team.meta.revision, command.expected_revision)?;
    let updated = Team {
        max_members: command.max_members,
        meta: touched(&team.meta, &admitted.by, admitted.now),
        ..team
    };
    let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
    Ok(Change::new(Op::SetTeamMaxMembers.ack())
        .put(team_file(&chain)?, records::encode_team(&updated))
        .reply(reply))
}

fn decide_request<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &RequestTeamLimitIncrease,
) -> Result<Change, Refusal>
where
    E::Tx: TeamDependents,
{
    stable(&command.id, "the limit request's ID")?;
    stable(&command.team_id, "the team's ID")?;
    let admitted = admit_active_employee(uow, here, by)?;
    let view = uow.view();
    let (team, chain) = load_team_and_chain(view, &command.team_id)?;
    if team.lead_employee_id.as_ref() != Some(&admitted.actor.id) {
        return invalid(TEAM_LIMIT_REQUEST_DENIED);
    }
    validate_team_and_limit(&team, command.requested_max_members)?;
    if command.requested_max_members <= team.max_members {
        return reject(
            RejectCode::InvalidRequest,
            format!(
                "requested_max_members ({}) must be greater than current max_members ({})",
                command.requested_max_members, team.max_members
            ),
        );
    }
    let reason = parse_reason(&command.reason, "reason")?;
    let Some(approver_id) = resolve_limit_approver(view, owner, &admitted.actor, &chain)? else {
        return reject_with_hint(
            RejectCode::InvalidRequest,
            "no higher manager exists to approve this limit request",
            format!(
                "Raise the limit directly with `team set-limit --team {} --max-members {} --if-match {}`.",
                team.id, command.requested_max_members, team.meta.revision
            ),
        );
    };
    if id_in_use(view, &admitted.organization, &command.id)? {
        return reject(RejectCode::AlreadyExists, "the limit request ID is in use");
    }
    let now_text = records::time_text(admitted.now);
    let record = TeamLimitRequest {
        id: command.id.clone(),
        revision: 1,
        state: TeamLimitRequestState::Pending,
        team_id: team.id,
        requested_by_employee_id: admitted.actor.id,
        approver_employee_id: approver_id,
        current_max_members: team.max_members,
        requested_max_members: command.requested_max_members,
        reason,
        decision_reason: None,
        decided_by_employee_id: None,
        decided_at: None,
        created_at: now_text.clone(),
        updated_at: now_text,
    };
    let path = records::team_limit_request_path(&chain, &record.id)
        .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
    let reply = limit_reply(&record, None, admitted.generation);
    Ok(Change::new(Op::RequestTeamLimitIncrease.ack())
        .put(path, records::encode_team_limit_request(&record))
        .reply(reply))
}

fn apply_approved_limit(
    change: Change,
    team: Team,
    chain: &[Id],
    requested: u64,
    by: &records::By,
    now: org_store::Timestamp,
) -> Result<(Change, u64), Refusal> {
    validate_team_and_limit(&team, requested)?;
    if team.max_members >= requested {
        return reject(
            RejectCode::Conflict,
            "team max_members is already at or above the requested limit",
        );
    }
    let updated_team = Team {
        max_members: requested,
        meta: touched(&team.meta, by, now),
        ..team
    };
    let rev = updated_team.meta.revision;
    Ok((
        change.put(team_file(chain)?, records::encode_team(&updated_team)),
        rev,
    ))
}

fn decide_approval<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &DecideTeamLimitRequest,
) -> Result<Change, Refusal>
where
    E::Tx: TeamDependents,
{
    stable(&command.request_id, "the limit request's ID")?;
    let admitted = admit_active_employee(uow, here, by)?;
    let view = uow.view();
    let Some(req) = view.team_limit_request(&command.request_id)? else {
        return reject(RejectCode::NotFound, "no such team limit request");
    };
    if !may_decide_limit_request(view, owner, &admitted.actor, &req)? {
        return invalid(TEAM_LIMIT_DECIDE_DENIED);
    }
    if req.state.is_terminal() {
        return reject(
            RejectCode::Conflict,
            "this team limit request is already decided",
        );
    }
    check_revision(req.revision, command.expected_revision)?;
    let decision_reason = command
        .reason
        .as_deref()
        .map(|raw| parse_reason(raw, "decision reason"))
        .transpose()?;
    let (team, chain) = load_team_and_chain(view, &req.team_id)?;
    let now_text = records::time_text(admitted.now);
    let state = if command.approve {
        TeamLimitRequestState::Approved
    } else {
        TeamLimitRequestState::Denied
    };
    let updated_req = TeamLimitRequest {
        revision: req.revision + 1,
        state,
        decision_reason,
        decided_by_employee_id: Some(admitted.actor.id),
        decided_at: Some(now_text.clone()),
        updated_at: now_text,
        ..req
    };
    let req_path = records::team_limit_request_path(&chain, &updated_req.id)
        .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
    let change = Change::new(Op::DecideTeamLimitRequest.ack())
        .put(req_path, records::encode_team_limit_request(&updated_req));
    let (change, team_revision) = if command.approve {
        let (c, rev) = apply_approved_limit(
            change,
            team,
            &chain,
            updated_req.requested_max_members,
            &admitted.by,
            admitted.now,
        )?;
        (c, Some(rev))
    } else {
        (change, None)
    };
    Ok(change.reply(limit_reply(
        &updated_req,
        team_revision,
        admitted.generation,
    )))
}
