//! Agent lifecycle and execution configuration features (`TASK-41` / C2, `FC-034`, `FC-097`):
//! - `UpdateAgent` (`member.update_agent`)
//! - `MoveAgent` (`member.move_agent`)
//! - `SetAgentStatus` (`member.set_agent_status` — pause / resume)
//! - `OffboardAgent` (`member.offboard`)
//! - `UpsertAgentExecution` (`agent.execution.set`)
//! - `CreateProviderConnection` & `CreateModelProfile`

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    authority::{
        admit_active_employee, check_team_capacity, is_owner, is_person_above_employee,
        is_person_ancestor_lead,
    },
    changed, invalid,
    shared::{
        admit, check_revision, id_in_use, led_by, named, profile_file, reject, team_file, touched,
        TOO_DEEP,
    },
    stable, Feature, Op, RejectCode, NOT_AUTHORIZED,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{
        self, AgentExecutionConfig, Employee, EmployeeKind, EmployeeStatus, Meta, ModelProfile,
        PermissionPreset, ProviderAuthMode, ProviderConnection, ReasoningEffort, Team,
    },
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct UpdateAgent {
    pub id: Id,
    pub expected_revision: u64,
    pub name: Option<String>,
    pub role_id: Option<Id>,
    pub clear_role_id: bool,
    pub reports_to: Option<Id>,
    pub clear_reports_to: bool,
    pub title: Option<String>,
    pub clear_title: bool,
    pub focus: Option<String>,
    pub clear_focus: bool,
    pub role_text: Option<String>,
    pub clear_role_text: bool,
    pub title_in_team: Option<String>,
    pub clear_title_in_team: bool,
    pub body: Option<String>,
}

impl Feature for UpdateAgent {
    const OP: Op = Op::UpdateAgent;

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
        stable(&self.id, "the agent's ID")?;
        if let Some(role_id) = &self.role_id {
            stable(role_id, "the role's ID")?;
        }
        if let Some(reports_to) = &self.reports_to {
            stable(reports_to, "the manager's ID")?;
        }
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        let Some(agent) = view.employee(&self.id)? else {
            return reject(RejectCode::NotFound, "no such agent");
        };
        if !is_person_above_employee(view, owner, &admitted.actor, &agent)? {
            return invalid(NOT_AUTHORIZED);
        }
        if agent.kind != EmployeeKind::Agent {
            return reject(RejectCode::InvalidRequest, "not an agent");
        }
        if agent.status == EmployeeStatus::Inactive {
            return reject(RejectCode::InvalidRequest, "the agent is offboarded");
        }
        validate_agent_edits(view, &agent, self)?;
        check_revision(agent.meta.revision, self.expected_revision)?;
        let Some(chain) = view.team_chain(&agent.home_team_id)? else {
            return invalid("the home team has no chain of parents to a top-level team");
        };
        let updated = apply_agent_edits(agent, self, &admitted.by, admitted.now);
        let path = profile_file(&chain, EmployeeKind::Agent, &updated.id)?;
        let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
        Ok(Change::new(Op::UpdateAgent.ack())
            .put(path, records::encode_employee(&updated))
            .reply(reply))
    }
}

fn check_no_conflicting_clears(cmd: &UpdateAgent) -> Result<(), Refusal> {
    let conflict = [
        cmd.role_id.is_some() && cmd.clear_role_id,
        cmd.reports_to.is_some() && cmd.clear_reports_to,
        cmd.title.is_some() && cmd.clear_title,
        cmd.focus.is_some() && cmd.clear_focus,
        cmd.role_text.is_some() && cmd.clear_role_text,
        cmd.title_in_team.is_some() && cmd.clear_title_in_team,
    ]
    .into_iter()
    .any(|bad| bad);
    if conflict {
        return reject(
            RejectCode::InvalidRequest,
            "cannot set and clear the same agent field in one update",
        );
    }
    Ok(())
}

fn validate_agent_manager(
    view: &impl OrganizationRead,
    agent_id: &Id,
    mgr_id: &Id,
) -> Result<(), Refusal> {
    if mgr_id == agent_id {
        return reject(RejectCode::Cycle, "an agent cannot report to itself");
    }
    let Some(mgr) = view.employee(mgr_id)? else {
        return reject(RejectCode::NotFound, "no such manager");
    };
    if mgr.status == EmployeeStatus::Inactive {
        return reject(RejectCode::InvalidRequest, "the manager is inactive");
    }
    check_reporting_cycle(view, agent_id, mgr_id)
}

fn validate_agent_edits(
    view: &impl OrganizationRead,
    agent: &Employee,
    cmd: &UpdateAgent,
) -> Result<(), Refusal> {
    check_no_conflicting_clears(cmd)?;
    if cmd.name.as_deref().is_some_and(|n| !records::is_line(n)) {
        return reject(RejectCode::InvalidName, "the name is one line of text");
    }
    for (field, val) in [
        ("title", &cmd.title),
        ("focus", &cmd.focus),
        ("role_text", &cmd.role_text),
        ("title_in_team", &cmd.title_in_team),
    ] {
        if val.as_deref().is_some_and(|v| !records::is_line(v)) {
            return reject(
                RejectCode::InvalidRequest,
                format!("{field} is one line of text"),
            );
        }
    }
    if let Some(role_id) = &cmd.role_id {
        if view.role(role_id)?.is_none() {
            return reject(RejectCode::NotFound, "no such role");
        }
    }
    if let Some(mgr_id) = &cmd.reports_to {
        validate_agent_manager(view, &agent.id, mgr_id)?;
    }
    Ok(())
}

fn check_reporting_cycle(
    view: &impl OrganizationRead,
    employee_id: &Id,
    start_manager: &Id,
) -> Result<(), Refusal> {
    let mut cursor = Some(start_manager.clone());
    let mut steps = 0usize;
    while let Some(id) = cursor {
        if id == *employee_id {
            return reject(RejectCode::Cycle, "reporting line would form a cycle");
        }
        steps += 1;
        if steps > 1024 {
            return reject(RejectCode::Cycle, "reporting line is too deep");
        }
        cursor = view.employee(&id)?.and_then(|e| e.reports_to_employee_id);
    }
    Ok(())
}

fn apply_opt<T: Clone>(cur: Option<T>, set: &Option<T>, clear: bool) -> Option<T> {
    match (set, clear) {
        (Some(v), _) => Some(v.clone()),
        (None, true) => None,
        (None, false) => cur,
    }
}

fn apply_agent_edits(
    agent: Employee,
    cmd: &UpdateAgent,
    by: &records::By,
    now: org_store::Timestamp,
) -> Employee {
    Employee {
        name: cmd.name.clone().unwrap_or(agent.name),
        role_id: apply_opt(agent.role_id, &cmd.role_id, cmd.clear_role_id),
        reports_to_employee_id: apply_opt(
            agent.reports_to_employee_id,
            &cmd.reports_to,
            cmd.clear_reports_to,
        ),
        title: apply_opt(agent.title, &cmd.title, cmd.clear_title),
        focus: apply_opt(agent.focus, &cmd.focus, cmd.clear_focus),
        role_text: apply_opt(agent.role_text, &cmd.role_text, cmd.clear_role_text),
        title_in_team: apply_opt(
            agent.title_in_team,
            &cmd.title_in_team,
            cmd.clear_title_in_team,
        ),
        body: cmd.body.clone().unwrap_or(agent.body),
        meta: touched(&agent.meta, by, now),
        ..agent
    }
}

/// Moves an agent to another home team, moving both `profile.md` and `execution.md` (if any)
/// and enforcing `max_members` on the destination team (ADR-0029 decision 8 / HIRE-6).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct MoveAgent {
    pub id: Id,
    pub expected_revision: u64,
    pub home_team: Id,
}

impl Feature for MoveAgent {
    const OP: Op = Op::MoveAgent;

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
        stable(&self.id, "the agent's ID")?;
        stable(&self.home_team, "the destination team's ID")?;
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        let Some(agent) = view.employee(&self.id)? else {
            return reject(RejectCode::NotFound, "no such agent");
        };
        if !is_person_above_employee(view, owner, &admitted.actor, &agent)? {
            return invalid(NOT_AUTHORIZED);
        }
        let Some(team) = view.team(&self.home_team)? else {
            return reject(RejectCode::NotFound, "no such team");
        };
        if !is_owner(owner, &admitted.actor)
            && !is_person_ancestor_lead(view, &admitted.actor, &team.id, true)?
        {
            return invalid(NOT_AUTHORIZED);
        }
        if agent.kind != EmployeeKind::Agent {
            return reject(RejectCode::InvalidRequest, "not an agent");
        }
        if agent.status == EmployeeStatus::Inactive {
            return reject(RejectCode::InvalidRequest, "the agent is offboarded");
        }
        if agent.builtin_key.is_some() {
            return reject(
                RejectCode::InvalidRequest,
                "the built-in HR lead stays in the top-level team",
            );
        }
        if !team.is_active() {
            return reject(RejectCode::InvalidRequest, "the team is archived");
        }
        if team.id != agent.home_team_id {
            if let Some(led) = led_by(view, &agent.id)? {
                return reject(
                    RejectCode::InvalidRequest,
                    format!(
                        "{} leads {}: clear the appointment or appoint another lead first",
                        agent.name,
                        named(&led)
                    ),
                );
            }
            check_team_capacity(view, &team)?;
        }
        check_revision(agent.meta.revision, self.expected_revision)?;
        if team.id == agent.home_team_id {
            return reject(
                RejectCode::AlreadyExists,
                "the agent is already in that team",
            );
        }
        let (Some(old), Some(new)) = (
            view.team_chain(&agent.home_team_id)?,
            view.team_chain(&team.id)?,
        ) else {
            return invalid("a team has no chain of parents to a top-level team");
        };
        let from = records::employee_path(&old, EmployeeKind::Agent, &agent.id)?;
        let to = profile_file(&new, EmployeeKind::Agent, &agent.id)?;
        let exec = view.agent_execution(&agent.id)?;
        let moved = Employee {
            home_team_id: team.id,
            meta: touched(&agent.meta, &admitted.by, admitted.now),
            ..agent
        };
        let reply = changed(&moved.id, moved.meta.revision, admitted.generation);
        let mut change = Change::new(Op::MoveAgent.ack())
            .remove(from)
            .put(to, records::encode_employee(&moved));
        if let Some(exec) = exec {
            let from_exec = records::agent_execution_path(&old, &moved.id)?;
            let to_exec = records::agent_execution_path(&new, &moved.id)
                .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
            change = change
                .remove(from_exec)
                .put(to_exec, records::encode_agent_execution(&exec));
        }
        Ok(change.reply(reply))
    }
}

/// Pauses or resumes an agent (`FC-034`, `doc-78` table: owner or ancestor person lead).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SetAgentStatus {
    pub id: Id,
    pub expected_revision: u64,
    pub paused: bool,
}

impl Feature for SetAgentStatus {
    const OP: Op = Op::SetAgentStatus;

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
        stable(&self.id, "the agent's ID")?;
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        let Some(agent) = view.employee(&self.id)? else {
            return reject(RejectCode::NotFound, "no such agent");
        };
        if !is_person_above_employee(view, owner, &admitted.actor, &agent)? {
            return invalid(NOT_AUTHORIZED);
        }
        if agent.kind != EmployeeKind::Agent {
            return reject(
                RejectCode::InvalidRequest,
                "only agents can be paused or resumed",
            );
        }
        if agent.status == EmployeeStatus::Inactive {
            return reject(RejectCode::InvalidRequest, "the agent is offboarded");
        }
        if agent.builtin_key.is_some() {
            return reject(
                RejectCode::InvalidRequest,
                "the built-in HR lead cannot be paused",
            );
        }
        check_revision(agent.meta.revision, self.expected_revision)?;
        let Some(chain) = view.team_chain(&agent.home_team_id)? else {
            return invalid("the home team has no chain of parents to a top-level team");
        };
        let next_status = if self.paused {
            EmployeeStatus::Paused
        } else {
            EmployeeStatus::Active
        };
        let updated = Employee {
            status: next_status,
            meta: touched(&agent.meta, &admitted.by, admitted.now),
            ..agent
        };
        let path = profile_file(&chain, EmployeeKind::Agent, &updated.id)?;
        let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
        Ok(Change::new(Op::SetAgentStatus.ack())
            .put(path, records::encode_employee(&updated))
            .reply(reply))
    }
}

/// Offboards an agent (`status: inactive`, `offboarded_at: now`, clears team lead if any).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct OffboardAgent {
    pub id: Id,
    pub expected_revision: u64,
}

impl Feature for OffboardAgent {
    const OP: Op = Op::OffboardAgent;

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
        stable(&self.id, "the agent's ID")?;
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        let Some(agent) = view.employee(&self.id)? else {
            return reject(RejectCode::NotFound, "no such agent");
        };
        if !is_person_above_employee(view, owner, &admitted.actor, &agent)? {
            return invalid(NOT_AUTHORIZED);
        }
        if agent.kind != EmployeeKind::Agent {
            return reject(RejectCode::InvalidRequest, "only agents can be offboarded");
        }
        if agent.builtin_key.is_some() {
            return reject(
                RejectCode::InvalidRequest,
                "the built-in HR lead cannot be offboarded",
            );
        }
        if agent.status == EmployeeStatus::Inactive {
            return reject(
                RejectCode::InvalidRequest,
                "the agent is already offboarded",
            );
        }
        check_revision(agent.meta.revision, self.expected_revision)?;
        let Some(chain) = view.team_chain(&agent.home_team_id)? else {
            return invalid("the home team has no chain of parents to a top-level team");
        };
        let led_team = led_by(view, &agent.id)?;
        let updated = Employee {
            status: EmployeeStatus::Inactive,
            offboarded_at: Some(records::time_text(admitted.now)),
            meta: touched(&agent.meta, &admitted.by, admitted.now),
            ..agent
        };
        let path = profile_file(&chain, EmployeeKind::Agent, &updated.id)?;
        let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
        let mut change =
            Change::new(Op::OffboardAgent.ack()).put(path, records::encode_employee(&updated));
        if let Some(team) = led_team {
            let Some(t_chain) = view.team_chain(&team.id)? else {
                return invalid("the led team has no chain of parents");
            };
            let cleared = Team {
                lead_employee_id: None,
                meta: touched(&team.meta, &admitted.by, admitted.now),
                ..team
            };
            change = change.put(team_file(&t_chain)?, records::encode_team(&cleared));
        }
        Ok(change.reply(reply))
    }
}

/// Writes or updates an agent's `execution.md` (`FC-097`, `doc-88` §3–§4).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct UpsertAgentExecution {
    pub employee_id: Id,
    pub expected_revision: Option<u64>,
    pub model_profile_id: Id,
    pub model_id: String,
    pub tools_config: Vec<String>,
    pub permission_preset: PermissionPreset,
    /// Left out of the operation fingerprint when `None`, so a retry from before `TASK-181`
    /// still matches its receipt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    pub enabled: bool,
    pub system_prompt_markdown: String,
}

impl Feature for UpsertAgentExecution {
    const OP: Op = Op::UpsertAgentExecution;

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
        stable(&self.employee_id, "the agent's ID")?;
        stable(&self.model_profile_id, "the model profile's ID")?;
        let admitted = admit_active_employee(uow, here, by)?;
        let view = uow.view();
        let Some(agent) = view.employee(&self.employee_id)? else {
            return reject(RejectCode::NotFound, "no such agent");
        };
        if !is_person_above_employee(view, owner, &admitted.actor, &agent)? {
            return invalid(NOT_AUTHORIZED);
        }
        if agent.kind != EmployeeKind::Agent {
            return reject(
                RejectCode::InvalidRequest,
                "only agents have execution configs",
            );
        }
        if agent.status == EmployeeStatus::Inactive {
            return reject(RejectCode::InvalidRequest, "the agent is offboarded");
        }
        let Some(profile) = view.model_profile(&self.model_profile_id)? else {
            return reject(RejectCode::NotFound, "no such model profile");
        };
        if profile.archived {
            return reject(RejectCode::InvalidRequest, "the model profile is archived");
        }
        if !records::is_line(&self.model_id) {
            return reject(RejectCode::InvalidRequest, "model_id is one line of text");
        }
        if self.system_prompt_markdown.trim().is_empty() {
            return reject(
                RejectCode::InvalidRequest,
                "system_prompt_markdown must not be empty",
            );
        }
        let existing = view.agent_execution(&self.employee_id)?;
        let meta = match (&existing, self.expected_revision) {
            (Some(prior), Some(expected)) => {
                check_revision(prior.meta.revision, expected)?;
                touched(&prior.meta, &admitted.by, admitted.now)
            }
            (Some(prior), None) => touched(&prior.meta, &admitted.by, admitted.now),
            (None, Some(_)) => {
                return reject(RejectCode::Conflict, "execution config does not exist yet");
            }
            (None, None) => Meta::created(&admitted.by, admitted.now),
        };
        let Some(chain) = view.team_chain(&agent.home_team_id)? else {
            return invalid("the home team has no chain of parents");
        };
        let config = AgentExecutionConfig {
            employee_id: self.employee_id.clone(),
            model_profile_id: self.model_profile_id.clone(),
            model_id: self.model_id.clone(),
            tools_config: self.tools_config.clone(),
            permission_preset: self.permission_preset,
            effort: self.effort,
            enabled: self.enabled,
            meta,
            system_prompt_markdown: self.system_prompt_markdown.clone(),
        };
        let path = records::agent_execution_path(&chain, &config.employee_id)
            .or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))?;
        let reply = changed(
            &config.employee_id,
            config.meta.revision,
            admitted.generation,
        );
        Ok(Change::new(Op::UpsertAgentExecution.ack())
            .put(path, records::encode_agent_execution(&config))
            .reply(reply))
    }
}

/// Owner creates a provider connection + its auto-generated default model profile atomically
/// (`doc-88` §4: "Connection creation is one idempotent organization transaction").
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct CreateProviderConnection {
    pub id: Id,
    pub default_profile_id: Id,
    pub key: String,
    pub name: String,
    pub provider_type: String,
    pub auth_mode: ProviderAuthMode,
    pub subscription_identity: Option<String>,
    pub runtime_driver: String,
    pub endpoint_url: Option<String>,
    pub max_concurrent_turns: u64,
}

impl Feature for CreateProviderConnection {
    const OP: Op = Op::CreateProviderConnection;

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
        stable(&self.id, "the connection's ID")?;
        stable(&self.default_profile_id, "the default profile's ID")?;
        if self.id == self.default_profile_id {
            return invalid("connection id and default_profile_id must be distinct");
        }
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        validate_provider_connection(view, &admitted.organization, self)?;
        let meta = Meta::created(&admitted.by, admitted.now);
        let conn = ProviderConnection {
            id: self.id.clone(),
            key: self.key.clone(),
            name: self.name.clone(),
            provider_type: self.provider_type.clone(),
            auth_mode: self.auth_mode,
            subscription_identity: self.subscription_identity.clone(),
            runtime_driver: self.runtime_driver.clone(),
            endpoint_url: self.endpoint_url.clone(),
            enabled: true,
            archived: false,
            max_concurrent_turns: self.max_concurrent_turns,
            meta: meta.clone(),
        };
        let profile = ModelProfile {
            id: self.default_profile_id.clone(),
            key: self.key.clone(),
            name: self.name.clone(),
            description: format!("Default profile for {}", self.name),
            is_auto_generated: true,
            generated_for_connection_id: Some(self.id.clone()),
            archived: false,
            connection_ids: vec![self.id.clone()],
            meta,
        };
        let reply = changed(&conn.id, conn.meta.revision, admitted.generation);
        Ok(Change::new(Op::CreateProviderConnection.ack())
            .put(
                records::provider_connection_path(&conn.id),
                records::encode_provider_connection(&conn),
            )
            .put(
                records::model_profile_path(&profile.id),
                records::encode_model_profile(&profile),
            )
            .reply(reply))
    }
}

fn check_provider_fields(cmd: &CreateProviderConnection) -> Result<(), Refusal> {
    if !records::is_line(&cmd.key) || !records::is_line(&cmd.name) {
        return reject(
            RejectCode::InvalidName,
            "key and name are single lines of text",
        );
    }
    if !records::is_line(&cmd.provider_type) || !records::is_line(&cmd.runtime_driver) {
        return reject(
            RejectCode::InvalidRequest,
            "provider_type and runtime_driver are required",
        );
    }
    if !(1..=1000).contains(&cmd.max_concurrent_turns) {
        return reject(
            RejectCode::InvalidRequest,
            "max_concurrent_turns must be between 1 and 1000",
        );
    }
    let bad_sub = cmd
        .subscription_identity
        .as_deref()
        .is_some_and(|s| !records::is_line(s))
        || (cmd.auth_mode == ProviderAuthMode::SubscriptionCli
            && cmd.subscription_identity.is_none());
    if bad_sub {
        return reject(
            RejectCode::InvalidRequest,
            "subscription_identity must be a valid line and is required for subscription_cli",
        );
    }
    if cmd
        .endpoint_url
        .as_deref()
        .is_some_and(|u| !super::provider_pools::validate_endpoint_url(u))
    {
        return reject(
            RejectCode::InvalidRequest,
            "endpoint_url must be a valid https:// or loopback http:// URL without credentials",
        );
    }
    Ok(())
}

fn validate_provider_connection(
    view: &impl TeamDependents,
    organization: &records::Organization,
    cmd: &CreateProviderConnection,
) -> Result<(), Refusal> {
    check_provider_fields(cmd)?;
    let conns = view.provider_connections()?;
    if conns.iter().any(|c| c.key == cmd.key)
        || view.model_profiles()?.iter().any(|p| p.key == cmd.key)
    {
        return reject(
            RejectCode::AlreadyExists,
            "a provider connection or model profile with that key already exists",
        );
    }
    let sub_taken = cmd.subscription_identity.as_deref().is_some_and(|sub| {
        conns.iter().any(|c| {
            c.provider_type == cmd.provider_type && c.subscription_identity.as_deref() == Some(sub)
        })
    });
    if sub_taken {
        return reject(RejectCode::AlreadyExists, "SUBSCRIPTION_ALREADY_REGISTERED");
    }
    if id_in_use(view, organization, &cmd.id)?
        || id_in_use(view, organization, &cmd.default_profile_id)?
    {
        return reject(
            RejectCode::AlreadyExists,
            "connection or profile ID is in use",
        );
    }
    Ok(())
}

/// Owner-only organization setting choosing the model profile for channel question watching
/// (`ADR-0024` d8/d13, `WATCH-5`). Passing `model_profile_id: None` disables watching.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SetWatchingModel {
    pub model_profile_id: Option<Id>,
}

impl Feature for SetWatchingModel {
    const OP: Op = Op::SetWatchingModel;

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
        if let Some(pid) = &self.model_profile_id {
            stable(pid, "the watching model profile's ID")?;
        }
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        if let Some(pid) = &self.model_profile_id {
            let Some(profile) = view.model_profile(pid)? else {
                return reject(RejectCode::NotFound, "model profile not found");
            };
            if profile.archived {
                return reject(
                    RejectCode::InvalidRequest,
                    "cannot select an archived model profile for channel watching",
                );
            }
        }
        let existing = view.watching_model()?;
        let meta = match existing {
            Some(prev) => touched(&prev.meta, &admitted.by, admitted.now),
            None => Meta::created(&admitted.by, admitted.now),
        };
        let record = records::WatchingModelSetting {
            model_profile_id: self.model_profile_id.clone(),
            meta,
        };
        let reply = changed(
            &admitted.organization.id,
            record.meta.revision,
            admitted.generation,
        );
        Ok(Change::new(Op::SetWatchingModel.ack())
            .put(
                records::watching_model_path(),
                records::encode_watching_model(&record),
            )
            .reply(reply))
    }
}
