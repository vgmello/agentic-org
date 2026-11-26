//! C2 hiring RPC routing (role catalog, HR hire lifecycle, permissions, team limits and
//! agent lifecycle writes) plus the shared read/write request envelopes and the
//! store-backed feature runners.

use org_proto::hiring::{
    AgentExecutionGet, AgentExecutionSet, AgentMove, AgentOffboard, AgentSetStatus, AgentUpdate,
    EntityOpResult, HireOpResult, HrHire, HrHireConfirm, HrHireDraft, HrHireInspect, HrHireList,
    HrHireListResult, HrHireWithdraw, MemberSetMayHire, ModelProfileList, ModelProfileListResult,
    ProviderConnectionCreate, RoleCreate, RoleInspect, RoleList, RoleListResult, RoleUpdate,
    TeamSetMaxMembers,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};
use org_store::{Actor, Id};

use super::super::Office;
use super::convert::{
    decode_reply, exec_to_wire, hire_to_wire, map_store_error, opt_id, parse_id, profile_to_wire,
    proposal_from_wire, role_to_wire, storage_err,
};
use crate::organization::{
    features::{
        Changed, ConfirmHire, CreateProviderConnection, CreateRole, DraftHire, Feature, HireAgent,
        HireChanged, MoveAgent, OffboardAgent, SetAgentStatus, SetMayHire, SetTeamMaxMembers,
        UpdateAgent, UpdateRole, UpsertAgentExecution, WithdrawHire, NOT_AUTHORIZED,
    },
    records::{HireState, PermissionPreset, ProviderAuthMode},
    wire,
};
use crate::store::open::{
    AgentExecutionById, HireInspectById, HiresList, ModelProfilesList, ReceiptById,
    RoleInspectByRef, RolesList, StructureOp,
};

impl Office {
    /// Dispatches any C2 role, hiring, permission, team-limit, or agent lifecycle RPC.
    /// Returns `None` if `request.method` does not belong to this group.
    pub fn dispatch_hiring_rpc(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if let Some(res) = self.dispatch_hiring_reads(member, request) {
            return Some(res);
        }
        if let Some(res) = self.dispatch_hire_writes(member, request) {
            return Some(res);
        }
        self.dispatch_lifecycle_writes(member, request)
    }

    fn dispatch_hiring_reads(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if request.is::<RoleList>() {
            return Some(self.handle_read::<RoleList>(request, member, |_| {
                let (roles, _) = self.store.read(RolesList).map_err(storage_err)?;
                Ok(RoleListResult {
                    roles: roles.iter().map(role_to_wire).collect(),
                })
            }));
        }
        if request.is::<RoleInspect>() {
            return Some(self.handle_read::<RoleInspect>(request, member, |p| {
                let (found, _) = self
                    .store
                    .read(RoleInspectByRef(p.role))
                    .map_err(storage_err)?;
                found
                    .as_ref()
                    .map(role_to_wire)
                    .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "role not found"))
            }));
        }
        if request.is::<HrHireList>() {
            return Some(self.handle_read::<HrHireList>(request, member, |p| {
                let (hires, _) = self
                    .store
                    .read(HiresList {
                        team_id: p.team_id,
                        state: p.state,
                    })
                    .map_err(storage_err)?;
                Ok(HrHireListResult {
                    hires: hires.iter().map(hire_to_wire).collect(),
                })
            }));
        }
        if request.is::<HrHireInspect>() {
            return Some(self.handle_read::<HrHireInspect>(request, member, |p| {
                let id = parse_id(&p.request_id, "request_id")?;
                let (found, _) = self.store.read(HireInspectById(id)).map_err(storage_err)?;
                found
                    .as_ref()
                    .map(hire_to_wire)
                    .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "hire request not found"))
            }));
        }
        if request.is::<AgentExecutionGet>() {
            return Some(self.handle_read::<AgentExecutionGet>(request, member, |p| {
                let id = parse_id(&p.employee_id, "employee_id")?;
                let (found, _) = self
                    .store
                    .read(AgentExecutionById(id))
                    .map_err(storage_err)?;
                found.as_ref().map(exec_to_wire).ok_or_else(|| {
                    ApiError::new(ErrorCode::NotFound, "agent execution config not found")
                })
            }));
        }
        if request.is::<ModelProfileList>() {
            return Some(self.handle_read::<ModelProfileList>(request, member, |_| {
                let (profiles, _) = self.store.read(ModelProfilesList).map_err(storage_err)?;
                Ok(ModelProfileListResult {
                    profiles: profiles.iter().map(profile_to_wire).collect(),
                })
            }));
        }
        None
    }

    fn dispatch_hire_writes(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if request.is::<HrHireDraft>() {
            return Some(
                self.handle_write::<HrHireDraft>(request, member, |actor, p| {
                    let cmd = DraftHire {
                        id: parse_id(&p.request_id, "request_id")?,
                        home_team: parse_id(&p.home_team_id, "home_team_id")?,
                        role: opt_id(p.role_id.as_deref(), "role_id")?,
                        proposal: proposal_from_wire(p.proposal)?,
                        reason: p.reason,
                    };
                    self.run_hire_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<HrHireConfirm>() {
            return Some(
                self.handle_write::<HrHireConfirm>(request, member, |actor, p| {
                    let cmd = ConfirmHire {
                        request_id: parse_id(&p.request_id, "request_id")?,
                        employee_id: parse_id(&p.employee_id, "employee_id")?,
                        expected_revision: p.expected_version,
                        fail_at_step: p.fail_at_step,
                    };
                    self.run_hire_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<HrHire>() {
            return Some(self.handle_write::<HrHire>(request, member, |actor, p| {
                let cmd = HireAgent {
                    request_id: parse_id(&p.request_id, "request_id")?,
                    employee_id: parse_id(&p.employee_id, "employee_id")?,
                    home_team: parse_id(&p.home_team_id, "home_team_id")?,
                    role: opt_id(p.role_id.as_deref(), "role_id")?,
                    proposal: proposal_from_wire(p.proposal)?,
                    reason: p.reason,
                };
                self.run_hire_feature(&p.command_id, actor, cmd)
            }));
        }
        if request.is::<HrHireWithdraw>() {
            return Some(
                self.handle_write::<HrHireWithdraw>(request, member, |actor, p| {
                    let cmd = WithdrawHire {
                        request_id: parse_id(&p.request_id, "request_id")?,
                        expected_revision: p.expected_version,
                        closed_reason: p.closed_reason,
                    };
                    self.run_hire_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        None
    }

    fn dispatch_lifecycle_writes(
        &self,
        member: Option<&str>,
        request: &Request,
    ) -> Option<Response> {
        self.dispatch_role_and_policy_writes(member, request)
            .or_else(|| self.dispatch_agent_writes(member, request))
    }

    fn dispatch_role_and_policy_writes(
        &self,
        member: Option<&str>,
        request: &Request,
    ) -> Option<Response> {
        if request.is::<RoleCreate>() {
            return Some(
                self.handle_write::<RoleCreate>(request, member, |actor, p| {
                    let cmd = CreateRole {
                        id: parse_id(&p.role_id, "role_id")?,
                        key: p.key,
                        name: p.name,
                        body: p.body,
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<RoleUpdate>() {
            return Some(
                self.handle_write::<RoleUpdate>(request, member, |actor, p| {
                    let cmd = UpdateRole {
                        id: parse_id(&p.role_id, "role_id")?,
                        expected_revision: p.expected_version,
                        key: p.key,
                        clear_key: p.clear_key,
                        name: p.name,
                        body: p.body,
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<MemberSetMayHire>() {
            return Some(
                self.handle_write::<MemberSetMayHire>(request, member, |actor, p| {
                    let cmd = SetMayHire {
                        id: parse_id(&p.member_id, "member_id")?,
                        expected_revision: p.expected_version,
                        may_hire: p.may_hire,
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<TeamSetMaxMembers>() {
            return Some(
                self.handle_write::<TeamSetMaxMembers>(request, member, |actor, p| {
                    let cmd = SetTeamMaxMembers {
                        id: parse_id(&p.team_id, "team_id")?,
                        expected_revision: p.expected_version,
                        max_members: p.max_members,
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<ProviderConnectionCreate>() {
            return Some(self.handle_write::<ProviderConnectionCreate>(
                request,
                member,
                |actor, p| {
                    let auth_mode = ProviderAuthMode::parse(&p.auth_mode).ok_or_else(|| {
                        ApiError::new(ErrorCode::InvalidRequest, "invalid auth_mode")
                    })?;
                    let cmd = CreateProviderConnection {
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
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                },
            ));
        }
        None
    }

    fn dispatch_agent_writes(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if request.is::<AgentUpdate>() {
            return Some(
                self.handle_write::<AgentUpdate>(request, member, |actor, p| {
                    let cmd = UpdateAgent {
                        id: parse_id(&p.member_id, "member_id")?,
                        expected_revision: p.expected_version,
                        name: p.name,
                        role_id: opt_id(p.role_id.as_deref(), "role_id")?,
                        clear_role_id: p.clear_role_id,
                        reports_to: opt_id(
                            p.reports_to_member_id.as_deref(),
                            "reports_to_member_id",
                        )?,
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
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<AgentMove>() {
            return Some(self.handle_write::<AgentMove>(request, member, |actor, p| {
                let cmd = MoveAgent {
                    id: parse_id(&p.member_id, "member_id")?,
                    expected_revision: p.expected_version,
                    home_team: parse_id(&p.to_team_id, "to_team_id")?,
                };
                self.run_entity_feature(&p.command_id, actor, cmd)
            }));
        }
        if request.is::<AgentSetStatus>() {
            return Some(
                self.handle_write::<AgentSetStatus>(request, member, |actor, p| {
                    let cmd = SetAgentStatus {
                        id: parse_id(&p.member_id, "member_id")?,
                        expected_revision: p.expected_version,
                        paused: p.paused,
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<AgentOffboard>() {
            return Some(
                self.handle_write::<AgentOffboard>(request, member, |actor, p| {
                    let cmd = OffboardAgent {
                        id: parse_id(&p.member_id, "member_id")?,
                        expected_revision: p.expected_version,
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        if request.is::<AgentExecutionSet>() {
            return Some(
                self.handle_write::<AgentExecutionSet>(request, member, |actor, p| {
                    let preset = match p.permission_preset.as_deref() {
                        Some(raw) => PermissionPreset::parse(raw).ok_or_else(|| {
                            ApiError::new(ErrorCode::InvalidRequest, "invalid permission_preset")
                        })?,
                        None => PermissionPreset::SafeAsk,
                    };
                    let cmd = UpsertAgentExecution {
                        employee_id: parse_id(&p.employee_id, "employee_id")?,
                        expected_revision: p.expected_version,
                        model_profile_id: parse_id(&p.model_profile_id, "model_profile_id")?,
                        model_id: p.model_id,
                        tools_config: p.tools_config,
                        permission_preset: preset,
                        enabled: p.enabled,
                        system_prompt_markdown: p.system_prompt_markdown,
                    };
                    self.run_entity_feature(&p.command_id, actor, cmd)
                }),
            );
        }
        None
    }

    fn handle_read<C: Call>(
        &self,
        request: &Request,
        member: Option<&str>,
        f: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
    ) -> Response {
        if member.is_none() {
            return Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            );
        }
        let Ok(params) = request.params::<C>() else {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid params for `{}`", C::METHOD),
                ),
            );
        };
        match f(params) {
            Ok(res) => Response::ok::<C>(request.id, &res).unwrap_or_else(|_| {
                Response::error(
                    request.id,
                    ApiError::new(ErrorCode::StorageError, "could not encode response"),
                )
            }),
            Err(err) => Response::error(request.id, err),
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
        let Ok(params) = request.params::<C>() else {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid params for `{}`", C::METHOD),
                ),
            );
        };
        match f(Actor::Employee(id), params) {
            Ok(res) => Response::ok::<C>(request.id, &res).unwrap_or_else(|_| {
                Response::error(
                    request.id,
                    ApiError::new(ErrorCode::StorageError, "could not encode response"),
                )
            }),
            Err(err) => Response::error(request.id, err),
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

    fn run_entity_feature<C: Feature>(
        &self,
        command_id: &str,
        actor: Actor,
        command: C,
    ) -> Result<EntityOpResult, ApiError> {
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
        let changed: Changed = decode_reply(reply)?;
        Ok(EntityOpResult {
            id: changed.id.as_str().to_owned(),
            revision: changed.revision,
            generation: changed.generation,
        })
    }

    fn run_hire_feature<C: Feature>(
        &self,
        command_id: &str,
        actor: Actor,
        command: C,
    ) -> Result<HireOpResult, ApiError> {
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
        let changed: HireChanged = decode_reply(reply)?;
        // Provision private agent home ONLY after confirmation commits `state == hired` (AC #2).
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
