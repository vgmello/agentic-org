//! C2 hiring RPC routing (role catalog, HR hire lifecycle, permissions, team limits and
//! agent lifecycle writes) plus the shared read/write request envelopes and the
//! store-backed feature runners.

use org_proto::hiring::{
    AgentExecutionGet, AgentExecutionSet, AgentMove, AgentOffboard, AgentSetStatus, AgentUpdate,
    EntityOpResult, HireOpResult, HrHire, HrHireConfirm, HrHireDraft, HrHireInspect, HrHireList,
    HrHireListResult, HrHireWithdraw, MemberSetMayHire, ModelProfileList, ModelProfileListResult,
    ProviderConnectionCreate, RoleCreate, RoleInspect, RoleList, RoleListResult, RoleUpdate,
    TeamLimitRequestCreate, TeamLimitRequestDecide, TeamLimitRequestList,
    TeamLimitRequestListResult, TeamLimitRequestOpResult, TeamSetMaxMembers,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};
use org_store::{Actor, Id};

use super::super::Office;
use super::convert::{
    decode_reply, effort_from_wire, exec_to_wire, hire_to_wire, map_store_error, opt_id, parse_id,
    profile_to_wire, proposal_from_wire, role_to_wire, storage_err, team_limit_to_wire,
};
use crate::organization::{
    features::{
        Changed, ConfirmHire, CreateProviderConnection, CreateRole, DecideTeamLimitRequest,
        DraftHire, Feature, HireAgent, HireChanged, MoveAgent, OffboardAgent,
        RequestTeamLimitIncrease, SetAgentStatus, SetMayHire, SetTeamMaxMembers, TeamLimitChanged,
        UpdateAgent, UpdateRole, UpsertAgentExecution, WithdrawHire, NOT_AUTHORIZED,
    },
    records::{HireState, PermissionPreset, ProviderAuthMode},
};
use crate::store::open::{
    AgentExecutionById, HireInspectById, HiresList, ModelProfilesList, ReceiptById,
    RoleInspectByRef, RolesList, StructureOp, TeamLimitRequestsList,
};
use crate::structure::wire;

macro_rules! route_calls {
    ($self:ident, $req:ident, $member:ident, $handler:ident, { $($call:ty => |$a:ident, $p:ident| $body:expr),+ $(,)? }) => {{
        $(if $req.is::<$call>() {
            return Some($self.$handler::<$call>($req, $member, |$a, $p| $body));
        })+
        None
    }};
}

impl Office {
    /// Dispatches any C2 role, hiring, permission, team-limit, or agent lifecycle RPC.
    /// Returns `None` if `request.method` does not belong to this group.
    pub fn dispatch_hiring_rpc(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        self.dispatch_hiring_reads(member, request)
            .or_else(|| self.dispatch_hire_writes(member, request))
            .or_else(|| self.dispatch_role_and_policy_writes(member, request))
            .or_else(|| self.dispatch_agent_writes(member, request))
    }

    fn dispatch_hiring_reads(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        route_calls!(self, request, member, handle_read, {
            RoleList => |_c, _p| {
                let (roles, _) = self.store.read(RolesList).map_err(storage_err)?;
                Ok(RoleListResult { roles: roles.iter().map(role_to_wire).collect() })
            },
            RoleInspect => |_c, p| {
                let (found, _) = self.store.read(RoleInspectByRef(p.role)).map_err(storage_err)?;
                found.as_ref().map(role_to_wire).ok_or_else(|| ApiError::new(ErrorCode::NotFound, "role not found"))
            },
            HrHireList => |_c, p| {
                let (hires, _) = self.store.read(HiresList { team_id: p.team_id, state: p.state }).map_err(storage_err)?;
                Ok(HrHireListResult { hires: hires.iter().map(hire_to_wire).collect() })
            },
            HrHireInspect => |_c, p| {
                let id = parse_id(&p.request_id, "request_id")?;
                let (found, _) = self.store.read(HireInspectById(id)).map_err(storage_err)?;
                found.as_ref().map(hire_to_wire).ok_or_else(|| ApiError::new(ErrorCode::NotFound, "hire request not found"))
            },
            TeamLimitRequestList => |caller, p| {
                let Ok(actor) = Id::parse(caller) else {
                    return Ok(TeamLimitRequestListResult { requests: Vec::new() });
                };
                let (reqs, _) = self.store.read(TeamLimitRequestsList {
                    owner: self.effective_owner_for_ops(),
                    actor,
                    team_id: p.team_id,
                    state: p.state,
                    approver_employee_id: p.approver_employee_id,
                }).map_err(storage_err)?;
                Ok(TeamLimitRequestListResult { requests: reqs.iter().map(team_limit_to_wire).collect() })
            },
            AgentExecutionGet => |_c, p| {
                let id = parse_id(&p.employee_id, "employee_id")?;
                let (found, _) = self.store.read(AgentExecutionById(id)).map_err(storage_err)?;
                found.as_ref().map(exec_to_wire).ok_or_else(|| ApiError::new(ErrorCode::NotFound, "agent execution config not found"))
            },
            ModelProfileList => |_c, _p| {
                let (profiles, _) = self.store.read(ModelProfilesList).map_err(storage_err)?;
                Ok(ModelProfileListResult { profiles: profiles.iter().map(profile_to_wire).collect() })
            },
        })
    }

    fn dispatch_hire_writes(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        route_calls!(self, request, member, handle_write, {
            HrHireDraft => |actor, p| self.run_hire_feature(&p.command_id, actor, DraftHire {
                id: parse_id(&p.request_id, "request_id")?,
                home_team: parse_id(&p.home_team_id, "home_team_id")?,
                role: opt_id(p.role_id.as_deref(), "role_id")?,
                proposal: proposal_from_wire(p.proposal)?,
                reason: p.reason,
            }),
            HrHireConfirm => |actor, p| self.run_hire_feature(&p.command_id, actor, ConfirmHire {
                request_id: parse_id(&p.request_id, "request_id")?,
                employee_id: parse_id(&p.employee_id, "employee_id")?,
                expected_revision: p.expected_version,
                fail_at_step: p.fail_at_step,
            }),
            HrHire => |actor, p| self.run_hire_feature(&p.command_id, actor, HireAgent {
                request_id: parse_id(&p.request_id, "request_id")?,
                employee_id: parse_id(&p.employee_id, "employee_id")?,
                home_team: parse_id(&p.home_team_id, "home_team_id")?,
                role: opt_id(p.role_id.as_deref(), "role_id")?,
                proposal: proposal_from_wire(p.proposal)?,
                reason: p.reason,
            }),
            HrHireWithdraw => |actor, p| self.run_hire_feature(&p.command_id, actor, WithdrawHire {
                request_id: parse_id(&p.request_id, "request_id")?,
                expected_revision: p.expected_version,
                closed_reason: p.closed_reason,
            }),
        })
    }

    fn dispatch_role_and_policy_writes(
        &self,
        member: Option<&str>,
        request: &Request,
    ) -> Option<Response> {
        route_calls!(self, request, member, handle_write, {
            RoleCreate => |actor, p| self.run_entity_feature(&p.command_id, actor, CreateRole {
                id: parse_id(&p.role_id, "role_id")?,
                key: p.key,
                name: p.name,
                body: p.body,
            }),
            RoleUpdate => |actor, p| self.run_entity_feature(&p.command_id, actor, UpdateRole {
                id: parse_id(&p.role_id, "role_id")?,
                expected_revision: p.expected_version,
                key: p.key,
                clear_key: p.clear_key,
                name: p.name,
                body: p.body,
            }),
            MemberSetMayHire => |actor, p| self.run_entity_feature(&p.command_id, actor, SetMayHire {
                id: parse_id(&p.member_id, "member_id")?,
                expected_revision: p.expected_version,
                may_hire: p.may_hire,
            }),
            TeamSetMaxMembers => |actor, p| self.run_entity_feature(&p.command_id, actor, SetTeamMaxMembers {
                id: parse_id(&p.team_id, "team_id")?,
                expected_revision: p.expected_version,
                max_members: p.max_members,
            }),
            TeamLimitRequestCreate => |actor, p| self.run_team_limit_feature(&p.command_id, actor, RequestTeamLimitIncrease {
                id: parse_id(&p.request_id, "request_id")?,
                team_id: parse_id(&p.team_id, "team_id")?,
                requested_max_members: p.requested_max_members,
                reason: p.reason,
            }),
            TeamLimitRequestDecide => |actor, p| self.run_team_limit_feature(&p.command_id, actor, DecideTeamLimitRequest {
                request_id: parse_id(&p.request_id, "request_id")?,
                expected_revision: p.expected_version,
                approve: p.approve,
                reason: p.reason,
            }),
            ProviderConnectionCreate => |actor, p| {
                let auth_mode = ProviderAuthMode::parse(&p.auth_mode)
                    .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "invalid auth_mode"))?;
                self.run_entity_feature(&p.command_id, actor, CreateProviderConnection {
                    id: parse_id(&p.connection_id, "connection_id")?,
                    default_profile_id: parse_id(&p.default_profile_id, "default_profile_id")?,
                    key: p.key,
                    name: p.name,
                    provider_type: p.provider_type,
                    auth_mode,
                    subscription_identity: p.subscription_identity,
                    runtime_driver: p.runtime_driver,
                    endpoint_url: p.endpoint_url,
                    max_concurrent_turns: p.max_concurrent_turns,
                })
            },
        })
    }

    fn dispatch_agent_writes(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        route_calls!(self, request, member, handle_write, {
            AgentUpdate => |actor, p| self.run_entity_feature(&p.command_id, actor, UpdateAgent {
                id: parse_id(&p.member_id, "member_id")?,
                expected_revision: p.expected_version,
                name: p.name,
                role_id: opt_id(p.role_id.as_deref(), "role_id")?,
                clear_role_id: p.clear_role_id,
                reports_to: opt_id(p.reports_to_member_id.as_deref(), "reports_to_member_id")?,
                clear_reports_to: p.clear_reports_to,
                title: p.title,
                clear_title: p.clear_title,
                focus: p.focus,
                clear_focus: p.clear_focus,
                role_text: p.role_text,
                clear_role_text: p.clear_role_text,
                title_in_team: p.title_in_team,
                clear_title_in_team: p.clear_title_in_team,
                body: p.body,
            }),
            AgentMove => |actor, p| self.run_entity_feature(&p.command_id, actor, MoveAgent {
                id: parse_id(&p.member_id, "member_id")?,
                expected_revision: p.expected_version,
                home_team: parse_id(&p.to_team_id, "to_team_id")?,
            }),
            AgentSetStatus => |actor, p| self.run_entity_feature(&p.command_id, actor, SetAgentStatus {
                id: parse_id(&p.member_id, "member_id")?,
                expected_revision: p.expected_version,
                paused: p.paused,
            }),
            AgentOffboard => |actor, p| self.run_entity_feature(&p.command_id, actor, OffboardAgent {
                id: parse_id(&p.member_id, "member_id")?,
                expected_revision: p.expected_version,
            }),
            AgentExecutionSet => |actor, p| {
                let preset = match p.permission_preset.as_deref() {
                    Some(raw) => PermissionPreset::parse(raw).ok_or_else(|| {
                        ApiError::new(ErrorCode::InvalidRequest, "invalid permission_preset")
                    })?,
                    None => PermissionPreset::SafeAsk,
                };
                self.run_entity_feature(&p.command_id, actor, UpsertAgentExecution {
                    employee_id: parse_id(&p.employee_id, "employee_id")?,
                    expected_revision: p.expected_version,
                    model_profile_id: parse_id(&p.model_profile_id, "model_profile_id")?,
                    model_id: p.model_id,
                    tools_config: p.tools_config,
                    permission_preset: preset,
                    effort: effort_from_wire(p.effort.as_deref())?,
                    enabled: p.enabled,
                    system_prompt_markdown: p.system_prompt_markdown,
                })
            },
        })
    }

    fn finish_rpc<C: Call>(&self, id: u64, outcome: Result<C::Result, ApiError>) -> Response {
        match outcome {
            Ok(res) => Response::ok::<C>(id, &res).unwrap_or_else(|_| {
                Response::error(
                    id,
                    ApiError::new(ErrorCode::StorageError, "could not encode response"),
                )
            }),
            Err(err) => Response::error(id, err),
        }
    }

    fn decode_params<C: Call>(&self, request: &Request) -> Result<C::Params, Response> {
        request.params::<C>().map_err(|_| {
            Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid params for `{}`", C::METHOD),
                ),
            )
        })
    }

    fn handle_read<C: Call>(
        &self,
        request: &Request,
        member: Option<&str>,
        f: impl FnOnce(&str, C::Params) -> Result<C::Result, ApiError>,
    ) -> Response {
        let Some(caller) = member else {
            return Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            );
        };
        match self.decode_params::<C>(request) {
            Ok(params) => self.finish_rpc::<C>(request.id, f(caller, params)),
            Err(resp) => resp,
        }
    }

    fn handle_write<C: Call>(
        &self,
        request: &Request,
        member: Option<&str>,
        f: impl FnOnce(Actor, C::Params) -> Result<C::Result, ApiError>,
    ) -> Response {
        let Some(member_id) = member else {
            return Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            );
        };
        let Ok(id) = Id::parse(member_id) else {
            return Response::error(
                request.id,
                ApiError::new(ErrorCode::NotAuthorized, NOT_AUTHORIZED),
            );
        };
        match self.decode_params::<C>(request) {
            Ok(params) => self.finish_rpc::<C>(request.id, f(Actor::Employee(id), params)),
            Err(resp) => resp,
        }
    }

    fn check_window_unless_known(&self, command_id: &str) -> Result<Id, ApiError> {
        let op_id = parse_id(command_id, "command_id")?;
        let (existing, _) = self
            .store
            .read(ReceiptById(op_id.clone()))
            .map_err(storage_err)?;
        if existing.is_none() {
            let now_ms = self.clock.now().0;
            let high_water = self
                .clock_high_ms
                .fetch_max(now_ms, std::sync::atomic::Ordering::AcqRel)
                .max(now_ms);
            wire::check_command_window(command_id, high_water)?;
        }
        Ok(op_id)
    }

    fn run_structure_feature<C: Feature, R: serde::de::DeserializeOwned>(
        &self,
        command_id: &str,
        actor: Actor,
        command: C,
    ) -> Result<R, ApiError> {
        let op_id = self.check_window_unless_known(command_id)?;
        let reply = self
            .store
            .run(StructureOp {
                operation_id: op_id,
                here: self.identity.clone(),
                owner: self.owner.clone(),
                by: actor,
                command,
            })
            .map_err(map_store_error)?;
        decode_reply(reply)
    }

    fn run_entity_feature<C: Feature>(
        &self,
        command_id: &str,
        actor: Actor,
        command: C,
    ) -> Result<EntityOpResult, ApiError> {
        let changed: Changed = self.run_structure_feature(command_id, actor, command)?;
        Ok(EntityOpResult {
            id: changed.id.as_str().to_owned(),
            revision: changed.revision,
            generation: changed.generation,
        })
    }

    fn run_team_limit_feature<C: Feature>(
        &self,
        command_id: &str,
        actor: Actor,
        command: C,
    ) -> Result<TeamLimitRequestOpResult, ApiError> {
        let changed: TeamLimitChanged = self.run_structure_feature(command_id, actor, command)?;
        Ok(TeamLimitRequestOpResult {
            request_id: changed.request_id.as_str().to_owned(),
            team_id: changed.team_id.as_str().to_owned(),
            state: changed.state.as_str().to_owned(),
            approver_employee_id: changed.approver_employee_id.as_str().to_owned(),
            requested_max_members: changed.requested_max_members,
            revision: changed.revision,
            team_revision: changed.team_revision,
            generation: changed.generation,
        })
    }

    fn run_hire_feature<C: Feature>(
        &self,
        command_id: &str,
        actor: Actor,
        command: C,
    ) -> Result<HireOpResult, ApiError> {
        let changed: HireChanged = self.run_structure_feature(command_id, actor, command)?;
        if changed.state == HireState::Hired {
            if let Some(emp_id) = &changed.employee_id {
                self.ensure_agent_private_home(emp_id.as_str())
                    .map_err(|_| {
                        ApiError::new(ErrorCode::StorageError, "could not create agent home")
                    })?;
            }
        }
        Ok(HireOpResult {
            request_id: changed.request_id.as_str().to_owned(),
            state: changed.state.as_str().to_owned(),
            employee_id: changed.employee_id.map(|i| i.as_str().to_owned()),
            failed_step: changed.failed_step,
            revision: changed.revision,
            generation: changed.generation,
        })
    }
}
