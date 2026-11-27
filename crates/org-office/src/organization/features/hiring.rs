//! HR hiring flow: drafting, confirming, one-shot hiring, and withdrawing (`TASK-41` / C2,
//! `ADR-0029` HIRE-1, HIRE-3..6, HIRE-8, `doc-78` §Hire requests).
//!
//! Nothing creates an agent record, execution configuration, or private home before confirmation.
//! Every authorization failure returns unrecorded `not_authorized` with [`HIRE_DENIED`] and
//! writes no request record.

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, Timestamp, UnitOfWork};
use serde::{Deserialize, Serialize};

use super::{
    authority::{
        admit_active_employee, check_team_capacity, is_owner, may_hire_into, CapacityOp,
        HIRE_DENIED,
    },
    invalid, reply,
    shared::{check_revision, id_in_use, profile_file, reject, TOO_DEEP},
    stable, Feature, Op, RejectCode,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{
        self, AgentExecutionConfig, By, Employee, EmployeeKind, EmployeeStatus, HireProposal,
        HireRequest, HireState, Meta, PermissionPreset,
    },
};

/// Reply of a completed hire-request mutation.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HireChanged {
    pub request_id: Id,
    pub state: HireState,
    pub employee_id: Option<Id>,
    pub failed_step: Option<String>,
    pub revision: u64,
    pub generation: u64,
}

fn hire_reply(req: &HireRequest, generation: u64) -> Vec<u8> {
    reply(&HireChanged {
        request_id: req.id.clone(),
        state: req.state,
        employee_id: req.employee_id.clone(),
        failed_step: req.failed_step.clone(),
        revision: req.revision,
        generation,
    })
}

/// Validates a `HireProposal`'s textual fields, handle, reporting line, and optional execution
/// profile before any request is drafted or provisioned.
fn validate_proposal(
    view: &impl OrganizationRead,
    proposal: &HireProposal,
    reason: &str,
) -> Result<(), Refusal> {
    if !records::is_line(&proposal.name) {
        return reject(
            RejectCode::InvalidName,
            "the agent name is one line of text",
        );
    }
    if !records::is_handle(&proposal.handle) {
        return reject(
            RejectCode::InvalidName,
            "a handle is lowercase letters, digits, `-`, `_` and inner dots, and not a reserved word",
        );
    }
    if view.handle_in_use(&proposal.handle)? {
        return reject(RejectCode::AlreadyExists, "the handle is taken");
    }
    if reason.trim().is_empty() {
        return reject(RejectCode::InvalidRequest, "reason is required for a hire");
    }
    for (field, val) in [
        ("title", &proposal.title),
        ("focus", &proposal.focus),
        ("role_text", &proposal.role_text),
        ("title_in_team", &proposal.title_in_team),
    ] {
        if val.as_deref().is_some_and(|v| !records::is_line(v)) {
            return reject(
                RejectCode::InvalidRequest,
                format!("{field} is one line of text"),
            );
        }
    }
    if let Some(reports_to) = &proposal.reports_to_employee_id {
        let mgr_id = Id::parse(reports_to)
            .map_err(|_| Refusal::invalid("reports_to_employee_id is not a valid ID"))?;
        let Some(mgr) = view.employee(&mgr_id)? else {
            return reject(
                RejectCode::NotFound,
                "reports_to_employee_id names no employee",
            );
        };
        if mgr.status == EmployeeStatus::Inactive {
            return reject(RejectCode::InvalidRequest, "the manager is inactive");
        }
    }
    if let Some(office_id_str) = &proposal.office_id {
        let office_id = Id::parse(office_id_str)
            .map_err(|_| Refusal::invalid("office_id is not a valid ID"))?;
        if view.office(&office_id)?.is_none() {
            return reject(RejectCode::NotFound, "no such office");
        }
    }
    if let Some(profile_id_str) = &proposal.model_profile_id {
        let profile_id = Id::parse(profile_id_str)
            .map_err(|_| Refusal::invalid("model_profile_id is not a valid ID"))?;
        let Some(profile) = view.model_profile(&profile_id)? else {
            return reject(RejectCode::NotFound, "no such model profile");
        };
        if profile.archived {
            return reject(RejectCode::InvalidRequest, "the model profile is archived");
        }
        if proposal
            .model_id
            .as_deref()
            .is_none_or(|m| !records::is_line(m))
        {
            return reject(
                RejectCode::InvalidRequest,
                "model_id is required when model_profile_id is set",
            );
        }
    }
    Ok(())
}

/// Step 1 of the two-step hire flow: drafts a proposal in `drafting` state.
/// Creates NO employee record, NO execution config, and NO private home (`HIRE-8`, AC #2).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct DraftHire {
    pub id: Id,
    pub home_team: Id,
    pub role: Option<Id>,
    pub proposal: HireProposal,
    pub reason: String,
}

impl Feature for DraftHire {
    const OP: Op = Op::DraftHire;

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
        stable(&self.id, "the hire request's ID")?;
        stable(&self.home_team, "the home team's ID")?;
        if let Some(role) = &self.role {
            stable(role, "the role's ID")?;
        }
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        if !may_hire_into(view, owner, &admitted.actor, &self.home_team)? {
            return invalid(HIRE_DENIED);
        }
        let Some(team) = view.team(&self.home_team)? else {
            return reject(RejectCode::NotFound, "no such team");
        };
        if !team.is_active() {
            return reject(RejectCode::InvalidRequest, "the team is archived");
        }
        check_team_capacity(view, owner, &admitted.actor, &team, CapacityOp::Hire)?;
        if let Some(role) = &self.role {
            if view.role(role)?.is_none() {
                return reject(RejectCode::NotFound, "no such role");
            }
        }
        validate_proposal(view, &self.proposal, &self.reason)?;
        if id_in_use(view, &admitted.organization, &self.id)? {
            return reject(RejectCode::AlreadyExists, "the hire request ID is in use");
        }
        let Some(chain) = view.team_chain(&team.id)? else {
            return invalid("the team has no chain of parents to a top-level team");
        };
        let now_text = records::time_text(admitted.now);
        let request = HireRequest {
            id: self.id.clone(),
            revision: 1,
            state: HireState::Drafting,
            requested_by_employee_id: admitted.actor.id,
            home_team_id: team.id,
            role_id: self.role.clone(),
            proposal: self.proposal.clone(),
            reason: self.reason.clone(),
            confirmed_at: None,
            closed_reason: None,
            employee_id: None,
            failed_step: None,
            created_at: now_text.clone(),
            updated_at: now_text,
        };
        let path = records::hire_request_path(&chain, &request.id)
            .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
        let reply = hire_reply(&request, admitted.generation);
        Ok(Change::new(Op::DraftHire.ack())
            .put(path, records::encode_hire_request(&request))
            .reply(reply))
    }
}

/// Step 2 of the two-step hire flow (and Try-again retry for stalled `hiring` requests per `HIRE-8`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct ConfirmHire {
    pub request_id: Id,
    pub employee_id: Id,
    pub expected_revision: u64,
    /// Optional fault injection step name (`HIRE-8` partial-provisioning failure test).
    pub fail_at_step: Option<String>,
}

impl Feature for ConfirmHire {
    const OP: Op = Op::ConfirmHire;

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
        stable(&self.request_id, "the hire request's ID")?;
        stable(&self.employee_id, "the new agent's ID")?;
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        let Some(req) = view.hire_request(&self.request_id)? else {
            return reject(RejectCode::NotFound, "no such hire request");
        };
        // Only the original hirer (still permitted) or the owner confirms/retries (ADR-0029 d5).
        let caller_is_owner = is_owner(owner, &admitted.actor);
        let caller_is_hirer = admitted.actor.id == req.requested_by_employee_id
            && may_hire_into(view, owner, &admitted.actor, &req.home_team_id)?;
        if !caller_is_owner && !caller_is_hirer {
            return invalid(HIRE_DENIED);
        }
        if !matches!(req.state, HireState::Drafting | HireState::Hiring) {
            return reject(
                RejectCode::InvalidRequest,
                "this hire request can no longer be confirmed",
            );
        }
        check_revision(req.revision, self.expected_revision)?;
        let Some(team) = view.team(&req.home_team_id)? else {
            return reject(RejectCode::NotFound, "no such team");
        };
        if !team.is_active() {
            return reject(RejectCode::InvalidRequest, "the team is archived");
        }
        check_team_capacity(view, owner, &admitted.actor, &team, CapacityOp::Hire)?;
        if let Some(role) = &req.role_id {
            if view.role(role)?.is_none() {
                return reject(RejectCode::NotFound, "no such role");
            }
        }
        validate_proposal(view, &req.proposal, &req.reason)?;
        if id_in_use(view, &admitted.organization, &self.employee_id)? {
            return reject(RejectCode::AlreadyExists, "the agent's ID is in use");
        }
        let Some(chain) = view.team_chain(&team.id)? else {
            return invalid("the team has no chain of parents to a top-level team");
        };
        let now_text = records::time_text(admitted.now);
        let confirmed_at = req.confirmed_at.clone().unwrap_or_else(|| now_text.clone());

        if let Some(step) = &self.fail_at_step {
            if !records::is_line(step) {
                return reject(
                    RejectCode::InvalidRequest,
                    "fail_at_step is one line of text",
                );
            }
            let stalled_req = HireRequest {
                revision: req.revision + 1,
                state: HireState::Hiring,
                confirmed_at: Some(confirmed_at),
                closed_reason: None,
                employee_id: None,
                failed_step: Some(step.clone()),
                updated_at: now_text,
                ..req
            };
            let path = records::hire_request_path(&chain, &stalled_req.id)
                .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
            let reply = hire_reply(&stalled_req, admitted.generation);
            return Ok(Change::new(Op::ConfirmHire.ack())
                .put(path, records::encode_hire_request(&stalled_req))
                .reply(reply));
        }

        let hired_req = HireRequest {
            revision: req.revision + 1,
            state: HireState::Hired,
            confirmed_at: Some(confirmed_at),
            closed_reason: None,
            employee_id: Some(self.employee_id.clone()),
            failed_step: None,
            updated_at: now_text,
            ..req
        };
        build_provisioned_change(
            Op::ConfirmHire.ack(),
            &chain,
            hired_req,
            &self.employee_id,
            &admitted.by,
            admitted.now,
            admitted.generation,
        )
    }
}

/// One-shot confirmed hire (`ADR-0029` decision 5: a call supplying every required field is its
/// own confirmation, e.g. `agent-os hr hire --team ... --name ... --handle ... --reason ...`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct HireAgent {
    pub request_id: Id,
    pub employee_id: Id,
    pub home_team: Id,
    pub role: Option<Id>,
    pub proposal: HireProposal,
    pub reason: String,
}

impl Feature for HireAgent {
    const OP: Op = Op::HireAgent;

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
        stable(&self.request_id, "the hire request's ID")?;
        stable(&self.employee_id, "the new agent's ID")?;
        stable(&self.home_team, "the home team's ID")?;
        if self.request_id == self.employee_id {
            return invalid("request_id and employee_id must be distinct");
        }
        if let Some(role) = &self.role {
            stable(role, "the role's ID")?;
        }
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        if !may_hire_into(view, owner, &admitted.actor, &self.home_team)? {
            return invalid(HIRE_DENIED);
        }
        let Some(team) = view.team(&self.home_team)? else {
            return reject(RejectCode::NotFound, "no such team");
        };
        if !team.is_active() {
            return reject(RejectCode::InvalidRequest, "the team is archived");
        }
        check_team_capacity(view, owner, &admitted.actor, &team, CapacityOp::Hire)?;
        if let Some(role) = &self.role {
            if view.role(role)?.is_none() {
                return reject(RejectCode::NotFound, "no such role");
            }
        }
        validate_proposal(view, &self.proposal, &self.reason)?;
        if id_in_use(view, &admitted.organization, &self.request_id)? {
            return reject(RejectCode::AlreadyExists, "the hire request ID is in use");
        }
        if id_in_use(view, &admitted.organization, &self.employee_id)? {
            return reject(RejectCode::AlreadyExists, "the agent's ID is in use");
        }
        let Some(chain) = view.team_chain(&team.id)? else {
            return invalid("the team has no chain of parents to a top-level team");
        };
        let now_text = records::time_text(admitted.now);
        let hired_req = HireRequest {
            id: self.request_id.clone(),
            revision: 1,
            state: HireState::Hired,
            requested_by_employee_id: admitted.actor.id,
            home_team_id: team.id,
            role_id: self.role.clone(),
            proposal: self.proposal.clone(),
            reason: self.reason.clone(),
            confirmed_at: Some(now_text.clone()),
            closed_reason: None,
            employee_id: Some(self.employee_id.clone()),
            failed_step: None,
            created_at: now_text.clone(),
            updated_at: now_text,
        };
        build_provisioned_change(
            Op::HireAgent.ack(),
            &chain,
            hired_req,
            &self.employee_id,
            &admitted.by,
            admitted.now,
            admitted.generation,
        )
    }
}

/// Closes a `drafting` (`-> withdrawn`) or stalled `hiring` (`-> failed`) hire request without
/// creating an agent.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct WithdrawHire {
    pub request_id: Id,
    pub expected_revision: u64,
    pub closed_reason: String,
}

impl Feature for WithdrawHire {
    const OP: Op = Op::WithdrawHire;

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
        stable(&self.request_id, "the hire request's ID")?;
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        let Some(req) = view.hire_request(&self.request_id)? else {
            return reject(RejectCode::NotFound, "no such hire request");
        };
        if !is_owner(owner, &admitted.actor) && admitted.actor.id != req.requested_by_employee_id {
            return invalid(HIRE_DENIED);
        }
        if !matches!(req.state, HireState::Drafting | HireState::Hiring) {
            return reject(
                RejectCode::InvalidRequest,
                "only a drafting or in-progress hire request can be closed",
            );
        }
        if !records::is_line(&self.closed_reason) {
            return reject(
                RejectCode::InvalidRequest,
                "closed_reason is one line of text",
            );
        }
        check_revision(req.revision, self.expected_revision)?;
        let Some(chain) = view.team_chain(&req.home_team_id)? else {
            return invalid("the team has no chain of parents to a top-level team");
        };
        let next_state = match req.state {
            HireState::Drafting => HireState::Withdrawn,
            HireState::Hiring => HireState::Failed,
            _ => unreachable!(),
        };
        let closed = HireRequest {
            revision: req.revision + 1,
            state: next_state,
            confirmed_at: if next_state == HireState::Failed {
                req.confirmed_at.clone()
            } else {
                None
            },
            closed_reason: Some(self.closed_reason.clone()),
            employee_id: None,
            failed_step: if next_state == HireState::Failed {
                req.failed_step.clone()
            } else {
                None
            },
            updated_at: records::time_text(admitted.now),
            ..req
        };
        let path = records::hire_request_path(&chain, &closed.id)
            .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
        let reply = hire_reply(&closed, admitted.generation);
        Ok(Change::new(Op::WithdrawHire.ack())
            .put(path, records::encode_hire_request(&closed))
            .reply(reply))
    }
}

fn build_provisioned_change(
    ack: org_store::Ack,
    chain: &[Id],
    req: HireRequest,
    employee_id: &Id,
    by: &By,
    now: Timestamp,
    generation: u64,
) -> Result<Change, Refusal> {
    let reports_to = match &req.proposal.reports_to_employee_id {
        Some(raw) => Some(Id::parse(raw).map_err(|_| Refusal::invalid("invalid reports_to"))?),
        None => None,
    };
    let agent = Employee {
        id: employee_id.clone(),
        handle: req.proposal.handle.clone(),
        kind: EmployeeKind::Agent,
        name: req.proposal.name.clone(),
        home_team_id: req.home_team_id.clone(),
        role_id: req.role_id.clone(),
        reports_to_employee_id: reports_to,
        status: EmployeeStatus::Active,
        contact: None,
        title: req.proposal.title.clone(),
        focus: req.proposal.focus.clone(),
        role_text: req.proposal.role_text.clone(),
        title_in_team: req.proposal.title_in_team.clone(),
        offboarded_at: None,
        // ADR-0029 decision 7 / HIRE-5: a new agent always starts with may_hire = false.
        may_hire: false,
        builtin_key: None,
        meta: Meta::created(by, now),
        body: req.proposal.body.clone().unwrap_or_default(),
    };
    let profile_path = profile_file(chain, EmployeeKind::Agent, &agent.id)?;
    let req_path = records::hire_request_path(chain, &req.id)
        .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
    let mut change = Change::new(ack)
        .put(profile_path, records::encode_employee(&agent))
        .put(req_path, records::encode_hire_request(&req));

    if let (Some(profile_id_str), Some(model_id)) =
        (&req.proposal.model_profile_id, &req.proposal.model_id)
    {
        let model_profile_id =
            Id::parse(profile_id_str).map_err(|_| Refusal::invalid("invalid model_profile_id"))?;
        let prompt = req
            .proposal
            .system_prompt_markdown
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("You are {} (@{}).", agent.name, agent.handle));
        let exec = AgentExecutionConfig {
            employee_id: agent.id.clone(),
            model_profile_id,
            model_id: model_id.clone(),
            tools_config: req.proposal.tools_config.clone(),
            permission_preset: req
                .proposal
                .permission_preset
                .unwrap_or(PermissionPreset::SafeAsk),
            effort: req.proposal.effort,
            enabled: true,
            meta: Meta::created(by, now),
            system_prompt_markdown: prompt,
        };
        let exec_path = records::agent_execution_path(chain, &agent.id)
            .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
        change = change.put(exec_path, records::encode_agent_execution(&exec));
    }

    Ok(change.reply(hire_reply(&req, generation)))
}
