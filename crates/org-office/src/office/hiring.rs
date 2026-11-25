//! Office-level dispatch and private-home provisioning for C2 HR hiring, role catalog,
//! team limits, `may_hire`, and agent lifecycle (`TASK-41` / C2, `ADR-0029`).

use std::path::PathBuf;

use org_proto::hiring::{
    AgentExecutionGet, AgentExecutionRecord, AgentExecutionSet, AgentMove, AgentOffboard,
    AgentSetStatus, AgentUpdate, EntityOpResult, HireOpResult, HireProposalWire, HireRequestRecord,
    HrHire, HrHireConfirm, HrHireDraft, HrHireInspect, HrHireList, HrHireListResult,
    HrHireWithdraw, MemberSetMayHire, ModelProfileList, ModelProfileListResult, ModelProfileRecord,
    ProviderConnectionCreate, RoleCreate, RoleInspect, RoleList, RoleListResult, RoleRecord,
    RoleUpdate, TeamSetMaxMembers,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};
use org_store::{Actor, Id, Outcome, Reply, StoreError};

use super::{storage, Office};
use crate::error::OfficeError;
use crate::organization::{
    features::{
        Changed, ConfirmHire, CreateProviderConnection, CreateRole, DraftHire, Feature, HireAgent,
        HireChanged, MoveAgent, OffboardAgent, OrgRejection, RejectCode, SetAgentStatus,
        SetMayHire, SetTeamMaxMembers, UpdateAgent, UpdateRole, UpsertAgentExecution, WithdrawHire,
        HIRE_DENIED, NOT_AUTHORIZED,
    },
    records::{
        self, AgentExecutionConfig, HireProposal, HireRequest, HireState, ModelProfile,
        PermissionPreset, ProviderAuthMode, Role,
    },
    wire,
};
use crate::store::open::{
    AgentExecutionById, CheckHrMessageAccess, EmployeeById, EmployeesList, HireInspectById,
    HiresList, ModelProfileById, ModelProfilesList, ProviderConnectionById, ReceiptById, RoleById,
    RoleInspectByRef, RolesList, StructureOp,
};

impl Office {
    /// Returns the path of an agent's private home directory on this office
    /// (`<durable>/local/offices/<office_id>/agents/<employee_id>/home`).
    /// Never created before hire confirmation (`TASK-41` AC #2 / `HIRE-8`).
    pub fn agent_private_home_path(&self, employee_id: &str) -> PathBuf {
        self.durable
            .join("local")
            .join("offices")
            .join(self.identity.office_id.as_str())
            .join("agents")
            .join(employee_id)
            .join("home")
    }

    /// Ensures the agent's private `0700` home directory exists and returns its path.
    pub fn ensure_private_home(&self, employee_id: &str) -> Result<PathBuf, OfficeError> {
        self.ensure_agent_private_home(employee_id)?;
        Ok(self.agent_private_home_path(employee_id))
    }

    /// Reads one employee record by canonical UUIDv7 ID.
    pub fn load_employee(
        &self,
        employee_id: &str,
    ) -> Result<Option<records::Employee>, OfficeError> {
        let Ok(id) = Id::parse(employee_id) else {
            return Ok(None);
        };
        Ok(self.store.read(EmployeeById(id)).map_err(storage)?.0)
    }

    /// Lists all employees registered in the organization.
    pub fn list_employees(&self) -> Result<Vec<records::Employee>, OfficeError> {
        Ok(self.store.read(EmployeesList).map_err(storage)?.0)
    }

    /// Reads one role record by canonical UUIDv7 ID.
    pub fn load_role(&self, role_id: &str) -> Result<Option<records::Role>, OfficeError> {
        let Ok(id) = Id::parse(role_id) else {
            return Ok(None);
        };
        Ok(self.store.read(RoleById(id)).map_err(storage)?.0)
    }

    /// Reads one agent's execution configuration by canonical employee UUIDv7 ID.
    pub fn load_agent_execution(
        &self,
        employee_id: &str,
    ) -> Result<Option<records::AgentExecutionConfig>, OfficeError> {
        let Ok(id) = Id::parse(employee_id) else {
            return Ok(None);
        };
        Ok(self.store.read(AgentExecutionById(id)).map_err(storage)?.0)
    }

    /// Reads one provider connection by canonical UUIDv7 ID.
    pub fn load_provider_connection(
        &self,
        connection_id: &str,
    ) -> Result<Option<records::ProviderConnection>, OfficeError> {
        let Ok(id) = Id::parse(connection_id) else {
            return Ok(None);
        };
        Ok(self
            .store
            .read(ProviderConnectionById(id))
            .map_err(storage)?
            .0)
    }

    /// Reads one model profile by canonical UUIDv7 ID.
    pub fn load_model_profile(
        &self,
        profile_id: &str,
    ) -> Result<Option<records::ModelProfile>, OfficeError> {
        let Ok(id) = Id::parse(profile_id) else {
            return Ok(None);
        };
        Ok(self.store.read(ModelProfileById(id)).map_err(storage)?.0)
    }

    /// Reads the organization's channel watching model profile setting (`ADR-0024` d13, `WATCH-5`).
    pub fn watching_model_setting(
        &self,
    ) -> Result<Option<records::WatchingModelSetting>, OfficeError> {
        Ok(self
            .store
            .read(crate::store::open::WatchingModelRead)
            .map_err(storage)?
            .0)
    }

    /// Sets or clears the organization's channel watching model profile setting (owner-only,
    /// idempotent on `command_id`; `ADR-0024` d8/d13, `WATCH-5`).
    pub fn set_watching_model_setting(
        &self,
        actor: &str,
        command_id: &str,
        model_profile_id: Option<&str>,
    ) -> Result<Result<org_proto::chat::ChatWatchingModelResult, ApiError>, OfficeError> {
        let Ok(actor_id) = Id::parse(actor) else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "invalid actor id",
            )));
        };
        let Ok(cmd_id) = Id::parse(command_id) else {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id must be a canonical UUIDv7",
            )));
        };
        let profile_id = match model_profile_id {
            Some(raw) => match Id::parse(raw) {
                Ok(id) => Some(id),
                Err(_) => {
                    return Ok(Err(ApiError::new(
                        ErrorCode::InvalidRequest,
                        "model_profile_id must be a canonical UUIDv7",
                    )));
                }
            },
            None => None,
        };
        let op = crate::store::open::StructureOp {
            command: crate::organization::features::SetWatchingModel {
                model_profile_id: profile_id,
            },
            operation_id: cmd_id,
            by: org_store::Actor::Employee(actor_id),
            owner: self.owner.clone(),
            here: self.identity.clone(),
        };
        match self.store.run(op) {
            Ok(reply) => match decode_reply::<crate::organization::features::Changed>(reply) {
                Ok(_) => self.watching_model_view().map(Ok),
                Err(err) => Ok(Err(err)),
            },
            Err(e) => Ok(Err(map_store_error(e))),
        }
    }

    /// Builds the wire view for `chat.watching_model_get` / `chat.watching_model_set`.
    pub fn watching_model_view(
        &self,
    ) -> Result<org_proto::chat::ChatWatchingModelResult, OfficeError> {
        let setting = self.watching_model_setting()?;
        let revision = setting.as_ref().map_or(0, |s| s.meta.revision);
        let model_profile_id = setting
            .and_then(|s| s.model_profile_id)
            .map(|id| id.as_str().to_owned());
        let model_profile_name = match model_profile_id.as_deref() {
            Some(pid) => self.load_model_profile(pid)?.map(|p| p.name),
            None => None,
        };
        Ok(org_proto::chat::ChatWatchingModelResult {
            enabled: model_profile_id.is_some(),
            model_profile_id,
            model_profile_name,
            revision,
        })
    }

    fn ensure_agent_private_home(&self, employee_id: &str) -> Result<(), OfficeError> {
        let base = self
            .durable
            .join("local")
            .join("offices")
            .join(self.identity.office_id.as_str())
            .join("agents")
            .join(employee_id);
        let home = base.join("home");
        for dir in [
            self.durable.join("local"),
            self.durable.join("local").join("offices"),
            self.durable
                .join("local")
                .join("offices")
                .join(self.identity.office_id.as_str()),
            self.durable
                .join("local")
                .join("offices")
                .join(self.identity.office_id.as_str())
                .join("agents"),
            base,
            home,
        ] {
            match org_platform::fs::create_owner_only_dir(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(OfficeError::storage(e)),
            }
        }
        Ok(())
    }

    /// Checks whether `sender` may address `target` in messaging (ADR-0029 decision 3 / HIRE-2):
    /// anyone who may hire nowhere is refused when targeting `@hr` / the HR lead.
    pub fn check_hr_message_allowed(
        &self,
        sender: &str,
        target: &str,
    ) -> Result<Result<(), ApiError>, OfficeError> {
        let Ok(sender_id) = Id::parse(sender) else {
            return Ok(Err(ApiError::new(ErrorCode::NotAuthorized, HIRE_DENIED)));
        };
        let (allowed, _) = self
            .store
            .read(CheckHrMessageAccess {
                owner: self.owner.clone(),
                sender: sender_id,
                target: target.to_owned(),
            })
            .map_err(storage)?;
        if allowed {
            Ok(Ok(()))
        } else {
            Ok(Err(ApiError::new(ErrorCode::NotAuthorized, HIRE_DENIED)))
        }
    }

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

fn parse_id(raw: &str, field: &str) -> Result<Id, ApiError> {
    if !records::is_stable_id(raw) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("{field} must be a lowercase UUIDv7"),
        ));
    }
    Id::parse(raw).map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            format!("{field} must be a lowercase UUIDv7"),
        )
    })
}

fn opt_id(raw: Option<&str>, field: &str) -> Result<Option<Id>, ApiError> {
    raw.map(|v| parse_id(v, field)).transpose()
}

fn storage_err(_err: StoreError) -> ApiError {
    ApiError::new(ErrorCode::StorageError, "office storage error")
}

fn map_store_error(err: StoreError) -> ApiError {
    match err {
        StoreError::Invalid(msg) => {
            if msg == HIRE_DENIED || msg == NOT_AUTHORIZED {
                ApiError::new(ErrorCode::NotAuthorized, msg)
            } else {
                ApiError::new(ErrorCode::InvalidRequest, msg)
            }
        }
        StoreError::Conflict => ApiError::new(
            ErrorCode::OperationReused,
            "command_id reused with different params",
        ),
        _ => ApiError::new(ErrorCode::StorageError, "office storage error"),
    }
}

fn decode_reply<T: serde::de::DeserializeOwned>(reply: Reply) -> Result<T, ApiError> {
    match reply.outcome {
        Outcome::Completed => serde_json::from_slice(&reply.bytes)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "unreadable store reply")),
        Outcome::Rejected => {
            let rej: OrgRejection = serde_json::from_slice(&reply.bytes)
                .map_err(|_| ApiError::new(ErrorCode::StorageError, "unreadable rejection"))?;
            let code = match rej.code {
                RejectCode::NotFound => ErrorCode::NotFound,
                RejectCode::InvalidRequest => ErrorCode::InvalidRequest,
                RejectCode::InvalidName => ErrorCode::InvalidName,
                RejectCode::OwnerProtected => ErrorCode::OwnerProtected,
                RejectCode::Cycle => ErrorCode::Cycle,
                RejectCode::NotEmpty => ErrorCode::NotEmpty,
                RejectCode::Conflict => ErrorCode::Conflict,
                RejectCode::AlreadyExists => ErrorCode::AlreadyExists,
            };
            Err(ApiError::new(code, rej.message))
        }
    }
}

fn proposal_from_wire(w: HireProposalWire) -> Result<HireProposal, ApiError> {
    let permission_preset = match w.permission_preset.as_deref() {
        Some(raw) => Some(PermissionPreset::parse(raw).ok_or_else(|| {
            ApiError::new(ErrorCode::InvalidRequest, "invalid permission_preset")
        })?),
        None => None,
    };
    Ok(HireProposal {
        name: w.name,
        handle: w.handle,
        title: w.title,
        focus: w.focus,
        role_text: w.role_text,
        title_in_team: w.title_in_team,
        reports_to_employee_id: w.reports_to_employee_id,
        office_id: w.office_id,
        model_profile_id: w.model_profile_id,
        model_id: w.model_id,
        permission_preset,
        tools_config: w.tools_config,
        system_prompt_markdown: w.system_prompt_markdown,
        body: w.body,
    })
}

fn proposal_to_wire(p: &HireProposal) -> HireProposalWire {
    HireProposalWire {
        name: p.name.clone(),
        handle: p.handle.clone(),
        title: p.title.clone(),
        focus: p.focus.clone(),
        role_text: p.role_text.clone(),
        title_in_team: p.title_in_team.clone(),
        reports_to_employee_id: p.reports_to_employee_id.clone(),
        office_id: p.office_id.clone(),
        model_profile_id: p.model_profile_id.clone(),
        model_id: p.model_id.clone(),
        permission_preset: p.permission_preset.map(|x| x.as_str().to_owned()),
        tools_config: p.tools_config.clone(),
        system_prompt_markdown: p.system_prompt_markdown.clone(),
        body: p.body.clone(),
    }
}

fn by_to_wire(by: &crate::organization::records::By) -> (Option<String>, Option<String>) {
    match by {
        crate::organization::records::By::Employee(id) => {
            (Some("employee".to_owned()), Some(id.as_str().to_owned()))
        }
        crate::organization::records::By::System => (Some("system".to_owned()), None),
        crate::organization::records::By::Unknown => (Some("unknown".to_owned()), None),
    }
}

/// `9999-12-31T23:59:59.999Z`: largest UTC millisecond with a four-digit RFC 3339 year.
const MAX_RFC3339_4DIGIT_YEAR_MS: u64 = 253_402_300_799_999;

fn normalize_rfc3339_case(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            't' => 'T',
            'z' => 'Z',
            other => other,
        })
        .collect()
}

fn canonical_wire_time(raw: Option<&str>) -> Option<String> {
    raw.map(|value| match crate::time::parse_rfc3339_ms(value) {
        Some(ms) if ms <= MAX_RFC3339_4DIGIT_YEAR_MS => crate::time::format_rfc3339_ms(ms),
        Some(_) if value.as_bytes().get(17..19) == Some(b"60") => {
            crate::time::format_rfc3339_ms(MAX_RFC3339_4DIGIT_YEAR_MS)
        }
        Some(_) => normalize_rfc3339_case(value),
        None => value.to_owned(),
    })
}

fn role_to_wire(r: &Role) -> RoleRecord {
    let (created_by_kind, created_by_id) = by_to_wire(&r.meta.created_by);
    let (updated_by_kind, updated_by_id) = by_to_wire(&r.meta.updated_by);
    RoleRecord {
        id: r.id.as_str().to_owned(),
        key: r.key.clone(),
        name: r.name.clone(),
        body: r.body.clone(),
        revision: r.meta.revision,
        created_at: canonical_wire_time(r.meta.created_at.as_deref()),
        created_by_kind,
        created_by_id,
        updated_at: canonical_wire_time(r.meta.updated_at.as_deref()),
        updated_by_kind,
        updated_by_id,
    }
}

fn hire_to_wire(h: &HireRequest) -> HireRequestRecord {
    HireRequestRecord {
        id: h.id.as_str().to_owned(),
        revision: h.revision,
        state: h.state.as_str().to_owned(),
        requested_by_employee_id: h.requested_by_employee_id.as_str().to_owned(),
        home_team_id: h.home_team_id.as_str().to_owned(),
        role_id: h.role_id.as_ref().map(|i| i.as_str().to_owned()),
        proposal: proposal_to_wire(&h.proposal),
        reason: h.reason.clone(),
        confirmed_at: h.confirmed_at.clone(),
        closed_reason: h.closed_reason.clone(),
        employee_id: h.employee_id.as_ref().map(|i| i.as_str().to_owned()),
        failed_step: h.failed_step.clone(),
        created_at: h.created_at.clone(),
        updated_at: h.updated_at.clone(),
    }
}

fn exec_to_wire(e: &AgentExecutionConfig) -> AgentExecutionRecord {
    AgentExecutionRecord {
        employee_id: e.employee_id.as_str().to_owned(),
        model_profile_id: e.model_profile_id.as_str().to_owned(),
        model_id: e.model_id.clone(),
        tools_config: e.tools_config.clone(),
        permission_preset: e.permission_preset.as_str().to_owned(),
        enabled: e.enabled,
        system_prompt_markdown: e.system_prompt_markdown.clone(),
        revision: e.meta.revision,
    }
}

fn profile_to_wire(p: &ModelProfile) -> ModelProfileRecord {
    ModelProfileRecord {
        id: p.id.as_str().to_owned(),
        key: p.key.clone(),
        name: p.name.clone(),
        description: p.description.clone(),
        is_auto_generated: p.is_auto_generated,
        generated_for_connection_id: p
            .generated_for_connection_id
            .as_ref()
            .map(|i| i.as_str().to_owned()),
        archived: p.archived,
        connection_ids: p
            .connection_ids
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect(),
        revision: p.meta.revision,
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use crate::organization::records::{By, Meta};

    #[test]
    fn role_to_wire_canonicalizes_rfc3339_variants_and_leap_seconds() {
        let role = Role {
            id: Id::parse("01900000-0000-7000-8000-0000000000a1").unwrap(),
            key: Some("eng".into()),
            name: "Engineer".into(),
            body: "## Mission\nBuild.\n".into(),
            meta: Meta {
                revision: 2,
                created_at: Some("2028-02-29t08:00:00+02:00".into()),
                created_by: By::System,
                updated_at: Some("2026-12-31t23:59:60z".into()),
                updated_by: By::Employee(
                    Id::parse("01900000-0000-7000-8000-000000000001").unwrap(),
                ),
            },
        };
        let wire = role_to_wire(&role);
        assert_eq!(wire.created_at.as_deref(), Some("2028-02-29T06:00:00.000Z"));
        assert_eq!(wire.updated_at.as_deref(), Some("2027-01-01T00:00:00.000Z"));
        assert_eq!(wire.created_by_kind.as_deref(), Some("system"));
        assert_eq!(wire.updated_by_kind.as_deref(), Some("employee"));
        assert_eq!(
            wire.updated_by_id.as_deref(),
            Some("01900000-0000-7000-8000-000000000001")
        );

        // Offset timestamps whose UTC year exceeds 9999 preserve their validated 4-digit offset form;
        // leap second 9999-12-31T23:59:60Z clamps to 9999-12-31T23:59:59.999Z for browser parity.
        let boundary = Role {
            meta: Meta {
                created_at: Some("9999-12-31t23:59:59-00:01".into()),
                updated_at: Some("9999-12-31T23:59:60Z".into()),
                ..role.meta
            },
            ..role
        };
        let wire_boundary = role_to_wire(&boundary);
        assert_eq!(
            wire_boundary.created_at.as_deref(),
            Some("9999-12-31T23:59:59-00:01")
        );
        assert_eq!(
            wire_boundary.updated_at.as_deref(),
            Some("9999-12-31T23:59:59.999Z")
        );
    }
}
