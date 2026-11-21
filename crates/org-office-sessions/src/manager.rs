//! Production office `SessionManager`: spawns supervised agent sessions, coordinates
//! concurrency, enforces owner/agent authorization, persists stall/recovery state, and
//! dispatches all C3 RPCs with idempotent `command_id` replay (`TASK-42`).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    AgentIdentity, AgentName, AgentRuntime, Decision, EnvName, LaunchContext, PathPattern,
    RuntimeId, SessionId, SessionKind, SessionSpec, SpecPath, Token,
};
use org_office::organization_family::records::{
    BuiltinKey, Employee, EmployeeKind, EmployeeStatus, PermissionPreset, ProviderAuthMode,
};
use org_office::Office;
use org_proto::sessions::{
    MemberSessionApprovalDecide, MemberSessionClearStall, MemberSessionGrants,
    MemberSessionInspect, MemberSessionStart, MemberSessionStartParams, MemberSessionStop,
    ProviderCredentialRevoke, ProviderCredentialStatus, ProviderEnrollmentCancel,
    ProviderEnrollmentStart, ProviderEnrollmentStatus, ProviderEnrollmentSubmit,
    SessionGrantsResult, SessionInspectResult, SessionState, SessionView,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};
use org_supervise::{Environment, SearchPath, SessionMark};

use crate::binding::RunBindingRegistry;
use crate::credentials::{
    random_hex, rfc3339_now, CliEnrollmentDriver, CredentialVault, EnrollmentDriver,
    EnrollmentManager, EnrollmentVaultError,
};
use crate::instructions::{
    assemble_standing_instructions, compose_turn_objective, InstructionInput, TurnTrigger,
    SAME_USER_ISOLATION_NOTICE,
};
use crate::preset::{build_grants_summary, build_session_policy, PresetConfig};
use crate::pump::{run_pump, PumpCommand, PumpContext, SessionRuntimeState};
use crate::recovery::{
    ActiveTurnRecord, NativeProcessExitOracle, PendingStartRecord, ProcessExitOracle,
    RecoveryAction, RecoveryReport, SessionStore, StallWatchdog, DEFAULT_STALL_TIMEOUT,
};
use crate::runtime_registry::RuntimeRegistry;
use crate::subagents::{
    build_general_subagent_definition, write_subagent_definition, BackgroundRegistry,
};

/// Configuration for [`SessionManager`].
#[derive(Clone)]
pub struct SessionManagerConfig {
    pub office_socket: PathBuf,
    pub helper_dir: Option<PathBuf>,
    pub stall_timeout: Duration,
    pub enrollment_timeout: Duration,
    pub runtimes: RuntimeRegistry,
    pub enrollment_driver: Arc<dyn EnrollmentDriver>,
}

impl SessionManagerConfig {
    pub fn production(office_socket: PathBuf) -> Self {
        Self {
            office_socket,
            helper_dir: None,
            stall_timeout: DEFAULT_STALL_TIMEOUT,
            enrollment_timeout: crate::credentials::DEFAULT_ENROLLMENT_TIMEOUT,
            runtimes: RuntimeRegistry::production(),
            enrollment_driver: Arc::new(CliEnrollmentDriver),
        }
    }
}

struct LiveSessionHandle {
    state: Arc<Mutex<SessionRuntimeState>>,
    tx: Sender<PumpCommand>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct SessionRegistry {
    live: HashMap<String, LiveSessionHandle>,
    starting: HashSet<String>,
}

struct StartSlotGuard<'a> {
    registry: &'a Mutex<SessionRegistry>,
    employee_id: String,
}

impl Drop for StartSlotGuard<'_> {
    fn drop(&mut self) {
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.starting.remove(&self.employee_id);
    }
}

struct ResolvedRuntimePolicy {
    adapter: Arc<dyn AgentRuntime>,
    model_token: Option<Token>,
    auth_env: Vec<(OsString, OsString)>,
    preset: PermissionPreset,
    sandbox_enabled: bool,
}

struct PlannedSessionLaunch {
    emp_id: String,
    run_id: String,
    session_id: String,
    mark_str: String,
    runtime_id: String,
    home: PathBuf,
    workspace_real: PathBuf,
    tmpdir: PathBuf,
    preset_cfg: PresetConfig,
    spec_hash: String,
    auth_env: Vec<(OsString, OsString)>,
    ctx: LaunchContext,
    plan: org_agent_runtime::LaunchPlan,
    session: Box<dyn org_agent_runtime::Session>,
}

struct PreparedSessionLaunch {
    emp_id: String,
    run_id: String,
    session_id: String,
    runtime_id: String,
    home: PathBuf,
    preset_cfg: PresetConfig,
    spec_hash: String,
    objective: String,
    persisted_stalls: u32,
    spawned: org_supervise::Spawned,
    session: Box<dyn org_agent_runtime::Session>,
    lifecycle: org_supervise::runtime::Lifecycle,
}

const MAX_COMMAND_CACHE_ENTRIES: usize = 512;

#[derive(Clone)]
struct CachedRpcResponse {
    fingerprint: String,
    response: Response,
}

struct BindingGuard<'a> {
    bindings: &'a RunBindingRegistry,
    employee_id: &'a str,
    run_id: &'a str,
    armed: bool,
}

impl<'a> BindingGuard<'a> {
    fn new(bindings: &'a RunBindingRegistry, employee_id: &'a str, run_id: &'a str) -> Self {
        Self {
            bindings,
            employee_id,
            run_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for BindingGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.bindings.revoke(self.employee_id, Some(self.run_id));
        }
    }
}

/// Coordinates all live agent sessions, run bindings, recovery, and provider CLI enrollments
/// for one [`Office`].
pub struct SessionManager {
    office: Arc<Office>,
    config: SessionManagerConfig,
    store: SessionStore,
    bindings: RunBindingRegistry,
    enrollment: EnrollmentManager,
    registry: Mutex<SessionRegistry>,
    last_states: Mutex<HashMap<String, SessionView>>,
    startup_recovery: Mutex<RecoveryReport>,
    command_cache: Mutex<HashMap<(String, String, String), CachedRpcResponse>>,
}

impl SessionManager {
    pub fn new(office: Arc<Office>, config: SessionManagerConfig) -> Self {
        let info = office.info();
        let store = SessionStore::new(office.durable_root(), &info.office_id);
        let key = office.secrets_key().map(|k| *k.expose_bytes());
        let vault = CredentialVault::new(
            office.durable_root().to_path_buf(),
            &info.org_id,
            &info.office_id,
            key,
        );
        let enroll_tmp = office
            .durable_root()
            .join("local")
            .join("offices")
            .join(&info.office_id)
            .join("enroll-tmp");
        let enrollment = EnrollmentManager::with_timeout(
            vault,
            enroll_tmp,
            Arc::clone(&config.enrollment_driver),
            config.enrollment_timeout,
        );
        Self {
            office,
            config,
            store,
            bindings: RunBindingRegistry::new(),
            enrollment,
            registry: Mutex::new(SessionRegistry::default()),
            last_states: Mutex::new(HashMap::new()),
            startup_recovery: Mutex::new(RecoveryReport::default()),
            command_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn bindings(&self) -> &RunBindingRegistry {
        &self.bindings
    }

    /// Records an accepted `agent-os` command in memory and durably in `active-turn.v1.json`
    /// so mid-turn office crashes preserve accepted commands on restart (`ADR-0030` d8).
    pub fn record_accepted_command(&self, employee_id: &str, method: &str) {
        self.bindings
            .record_accepted_command(employee_id, method, method);
        let method_owned = method.to_owned();
        let _ = self.store.update_active_turn(employee_id, |rec| {
            rec.accepted_commands.push(method_owned);
            rec.updated_at = crate::credentials::rfc3339_now();
        });
    }

    #[cfg(test)]
    pub(crate) fn command_cache_fingerprints(&self) -> Vec<String> {
        self.command_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .map(|v| v.fingerprint.clone())
            .collect()
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    pub fn enrollment(&self) -> &EnrollmentManager {
        &self.enrollment
    }

    /// Scans office-local session state on startup and stores actionable ADR-0030 recovery items.
    pub fn recover_on_startup(&self) -> io::Result<RecoveryReport> {
        self.recover_on_startup_with(&NativeProcessExitOracle)
    }

    pub fn recover_on_startup_with(
        &self,
        oracle: &dyn ProcessExitOracle,
    ) -> io::Result<RecoveryReport> {
        let report = self.store.recover_on_startup(oracle)?;
        *self
            .startup_recovery
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = report.clone();
        Ok(report)
    }

    /// Returns pending startup resume actions computed during [`Self::recover_on_startup`].
    pub fn pending_resumes(&self) -> Vec<RecoveryAction> {
        self.startup_recovery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .actions
            .clone()
    }

    /// Stops any active session for `employee_id`, reaps background tasks, and revokes its
    /// run binding immediately (called on pause, offboard, or explicit stop; `SUB-5`, `B2 Q9`).
    pub fn stop_session(&self, employee_id: &str, reason: &str) -> Result<SessionView, ApiError> {
        self.bindings.revoke(employee_id, None);
        let handle = {
            let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.remove(employee_id)
        };
        if let Some(mut handle) = handle {
            let _ = handle.tx.send(PumpCommand::Stop {
                reason: reason.to_owned(),
            });
            if let Some(thread) = handle.thread.take() {
                let _ = thread.join();
            }
            let view = handle
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .to_view();
            self.last_states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(employee_id.to_owned(), view.clone());
            return Ok(view);
        }
        self.idle_view_for(employee_id)
    }

    /// Stops every live session cleanly when the office shuts down.
    pub fn stop_all(&self, reason: &str) {
        let ids: Vec<String> = {
            let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.keys().cloned().collect()
        };
        for id in ids {
            let _ = self.stop_session(&id, reason);
        }
    }

    /// Starts (or sends a direct turn to) an agent's supervised session (`AC #1`, `LIVE-9..13`).
    pub fn start_session(
        &self,
        actor_member_id: &str,
        params: &MemberSessionStartParams,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_or_person(actor_member_id)?;
        let employee = self.require_active_agent(&params.employee_id)?;
        let emp_id = employee.id.as_str();
        self.check_not_blocked(emp_id)?;

        let (live_view, finished_handle) =
            self.reserve_start_or_send(&employee, actor_member_id, params)?;
        if let Some(mut old) = finished_handle {
            if let Some(thread) = old.thread.take() {
                let _ = thread.join();
            }
        }
        if let Some(view) = live_view {
            return Ok(view);
        }
        let _guard = StartSlotGuard {
            registry: &self.registry,
            employee_id: emp_id.to_owned(),
        };
        // Re-check after joining any finishing pump so a concurrent stall/recovery lock is honored.
        self.check_not_blocked(emp_id)?;
        let prepared = self.prepare_and_spawn(actor_member_id, &employee, params)?;
        self.install_live_session(prepared)
    }

    /// Inspects an agent's live or last session (`CLI-9`). Agents may only inspect themselves.
    pub fn inspect_session(
        &self,
        caller_id: &str,
        employee_id: Option<&str>,
    ) -> Result<SessionInspectResult, ApiError> {
        let target = employee_id.unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person(caller_id)?;
        }
        let employee = self.require_agent(target)?;
        let accepted = self.bindings.accepted_commands(target).len() as u64;
        if let Some(handle) = self
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .live
            .get(target)
        {
            let guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            return Ok(SessionInspectResult {
                session: guard.to_view(),
                accepted_commands_count: accepted,
                spec_hash: guard.spec_hash.clone(),
            });
        }
        let view = self.idle_view_for(employee.id.as_str())?;
        Ok(SessionInspectResult {
            session: view,
            accepted_commands_count: accepted,
            spec_hash: String::new(),
        })
    }

    /// Lists effective permission grants & sandbox constraints (`CLI-10`). Agents may only read self.
    pub fn list_grants(
        &self,
        caller_id: &str,
        employee_id: Option<&str>,
    ) -> Result<SessionGrantsResult, ApiError> {
        let target = employee_id.unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person(caller_id)?;
        }
        let employee = self.require_agent(target)?;
        let home = self.office.agent_private_home_path(employee.id.as_str());
        let workspace = canonical_or_spec(&home)?;
        let (preset, sandbox_enabled) = self.load_preset_and_sandbox(employee.id.as_str())?;
        let socket_str = self.config.office_socket.display().to_string();
        let socket_spec = SpecPath::try_from(socket_str.as_str()).ok();
        let denied_globs = self.denied_office_globs();
        let cfg = build_session_policy(
            preset,
            sandbox_enabled,
            &workspace,
            socket_spec.as_ref(),
            &denied_globs,
            1,
        )
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid permission preset"))?;
        Ok(build_grants_summary(employee.id.as_str(), &cfg, &[]))
    }

    /// Owner approves or denies a pending escalation (`allow_once`, `allow_always`, `deny`).
    pub fn decide_approval(
        &self,
        actor_member_id: &str,
        employee_id: &str,
        approval_id: &str,
        decision_str: &str,
        reason: Option<String>,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_only(actor_member_id)?;
        let decision = parse_decision(decision_str, reason)?;
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let Some(handle) = reg.live.get(employee_id) else {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                "no live session for this agent",
            ));
        };
        let guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        let pending = guard
            .pending_approvals
            .iter()
            .find(|p| p.approval_id == approval_id)
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "approval request not found"))?;
        let allow_always_tool = (decision_str == "allow_always"
            && pending.escalation != "sandbox_bypass")
            .then_some(pending.tool);
        let view = guard.to_view();
        drop(guard);
        let _ = handle.tx.send(PumpCommand::AnswerApproval {
            approval_id: approval_id.to_owned(),
            decision,
            allow_always_tool,
        });
        Ok(view)
    }

    /// Owner clears a consecutive-stall or manual-recovery lock (`LIVE-12`).
    pub fn clear_stall_lock(
        &self,
        actor_member_id: &str,
        employee_id: &str,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_only(actor_member_id)?;
        let _ = self.require_agent(employee_id)?;
        let _ = self.store.clear_pending_start(employee_id);
        let _ = self.store.update_active_turn(employee_id, |persisted| {
            persisted.consecutive_stalls = 0;
            persisted.stall_locked = false;
            persisted.recovery_required = false;
            persisted.updated_at = rfc3339_now();
        });
        if let Some(handle) = self
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .live
            .get(employee_id)
        {
            let mut guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            guard.watchdog.clear_stall_lock(Instant::now());
            guard.recovery_required = false;
            if matches!(
                guard.state,
                SessionState::StallLocked | SessionState::RecoveryRequired
            ) {
                guard.state = SessionState::Idle;
            }
            return Ok(guard.to_view());
        }
        self.last_states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(employee_id);
        self.idle_view_for(employee_id)
    }

    /// Dispatches all C3 session & provider-enrollment RPCs with caller-scoped `command_id` replay.
    pub fn dispatch_rpc(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if !is_c3_rpc_method(&request.method) {
            return None;
        }
        let Some(caller_id) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotAuthorized, "authenticated caller required"),
            ));
        };
        if let Err(err) = self.authorize_rpc_caller(caller_id, &request.method) {
            return Some(Response::error(request.id, err));
        }
        let cache_meta = command_cache_meta(request);
        if let Some((cid, fp)) = cache_meta.as_ref() {
            if let Some(replayed) =
                self.lookup_command_cache(caller_id, &request.method, cid, fp, request.id)
            {
                return Some(replayed);
            }
        }
        let resp = self
            .dispatch_session_rpcs(Some(caller_id), request)
            .or_else(|| self.dispatch_enrollment_rpcs(Some(caller_id), request))?;
        if let (Some((cid, fp)), org_proto::Outcome::Result(_)) = (cache_meta, &resp.outcome) {
            self.store_command_cache(caller_id, &request.method, cid, fp, &resp);
        }
        Some(resp)
    }

    fn lookup_command_cache(
        &self,
        caller_id: &str,
        method: &str,
        cid: &str,
        fp: &str,
        request_id: u64,
    ) -> Option<Response> {
        let key = (caller_id.to_owned(), method.to_owned(), cid.to_owned());
        let cached = self
            .command_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
            .cloned()?;
        if cached.fingerprint != fp {
            return Some(Response::error(
                request_id,
                ApiError::new(
                    ErrorCode::Conflict,
                    "command_id already used with different parameters",
                ),
            ));
        }
        let mut replay = cached.response;
        replay.id = request_id;
        Some(replay)
    }

    fn store_command_cache(
        &self,
        caller_id: &str,
        method: &str,
        cid: String,
        fp: String,
        resp: &Response,
    ) {
        let mut cache = self.command_cache.lock().unwrap_or_else(|p| p.into_inner());
        if cache.len() >= MAX_COMMAND_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(
            (caller_id.to_owned(), method.to_owned(), cid),
            CachedRpcResponse {
                fingerprint: fp,
                response: resp.clone(),
            },
        );
    }

    fn authorize_rpc_caller(&self, caller_id: &str, method: &str) -> Result<(), ApiError> {
        match method {
            "provider.enrollment_start"
            | "provider.enrollment_status"
            | "provider.enrollment_submit"
            | "provider.enrollment_cancel"
            | "provider.credential_status"
            | "provider.credential_revoke"
            | "member.session_approval"
            | "member.session_clear_stall" => self.authorize_owner_only(caller_id),
            "member.session_start" | "member.session_stop" => {
                self.authorize_owner_or_person(caller_id)
            }
            _ => Ok(()),
        }
    }

    fn dispatch_session_rpcs(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if request.is::<MemberSessionStart>() {
            return Some(handle_call::<MemberSessionStart>(
                request,
                member,
                |m, p| self.start_session(m, &p),
            ));
        }
        if request.is::<MemberSessionStop>() {
            return Some(handle_call::<MemberSessionStop>(request, member, |m, p| {
                self.authorize_owner_or_person(m)?;
                self.stop_session(&p.employee_id, p.reason.as_deref().unwrap_or("stopped"))
            }));
        }
        if request.is::<MemberSessionInspect>() {
            return Some(handle_call::<MemberSessionInspect>(
                request,
                member,
                |m, p| self.inspect_session(m, p.employee_id.as_deref()),
            ));
        }
        if request.is::<MemberSessionGrants>() {
            return Some(handle_call::<MemberSessionGrants>(
                request,
                member,
                |m, p| self.list_grants(m, p.employee_id.as_deref()),
            ));
        }
        if request.is::<MemberSessionApprovalDecide>() {
            return Some(handle_call::<MemberSessionApprovalDecide>(
                request,
                member,
                |m, p| {
                    self.decide_approval(m, &p.employee_id, &p.approval_id, &p.decision, p.reason)
                },
            ));
        }
        if request.is::<MemberSessionClearStall>() {
            return Some(handle_call::<MemberSessionClearStall>(
                request,
                member,
                |m, p| self.clear_stall_lock(m, &p.employee_id),
            ));
        }
        None
    }

    fn dispatch_enrollment_rpcs(
        &self,
        member: Option<&str>,
        request: &Request,
    ) -> Option<Response> {
        if request.is::<ProviderEnrollmentStart>() {
            return Some(handle_call::<ProviderEnrollmentStart>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    let provider =
                        self.resolve_connection_provider(&p.connection_id, p.provider)?;
                    self.enrollment
                        .start(m, &p.connection_id, &provider)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderEnrollmentStatus>() {
            return Some(handle_call::<ProviderEnrollmentStatus>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .status(m, &p.connection_id, &p.login_id)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderEnrollmentSubmit>() {
            return Some(handle_call::<ProviderEnrollmentSubmit>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .submit_code(m, &p.connection_id, &p.login_id, &p.code)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderEnrollmentCancel>() {
            return Some(handle_call::<ProviderEnrollmentCancel>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .cancel(m, &p.connection_id, &p.login_id)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderCredentialStatus>() {
            return Some(handle_call::<ProviderCredentialStatus>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .vault()
                        .status(&p.connection_id)
                        .map_err(map_vault_err)
                },
            ));
        }
        if request.is::<ProviderCredentialRevoke>() {
            return Some(handle_call::<ProviderCredentialRevoke>(
                request,
                member,
                |m, p| {
                    self.authorize_owner_only(m)?;
                    self.enrollment
                        .vault()
                        .revoke(&p.connection_id)
                        .map_err(map_vault_err)
                },
            ));
        }
        None
    }

    fn resolve_connection_provider(
        &self,
        connection_id: &str,
        explicit: Option<String>,
    ) -> Result<String, ApiError> {
        if let Some(conn) = self
            .office
            .load_provider_connection(connection_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read connection"))?
        {
            return Ok(conn.provider_type);
        }
        explicit.ok_or_else(|| ApiError::new(ErrorCode::NotFound, "provider connection not found"))
    }

    fn authorize_owner_only(&self, member_id: &str) -> Result<(), ApiError> {
        if member_id == self.office.owner_member_id() {
            return Ok(());
        }
        Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner may perform this operation",
        ))
    }

    fn authorize_owner_or_person(&self, member_id: &str) -> Result<(), ApiError> {
        if member_id == self.office.owner_member_id() {
            return Ok(());
        }
        let found = self
            .office
            .load_employee(member_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not verify caller"))?;
        if found
            .is_some_and(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active)
        {
            return Ok(());
        }
        Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only an active person or the owner may manage sessions",
        ))
    }

    fn require_agent(&self, employee_id: &str) -> Result<Employee, ApiError> {
        let emp = self
            .office
            .load_employee(employee_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read employee"))?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "agent not found"))?;
        if emp.kind != EmployeeKind::Agent {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "target employee is not an agent",
            ));
        }
        Ok(emp)
    }

    fn require_active_agent(&self, employee_id: &str) -> Result<Employee, ApiError> {
        let emp = self.require_agent(employee_id)?;
        if emp.status != EmployeeStatus::Active {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "only active agents can start a session",
            ));
        }
        Ok(emp)
    }

    fn check_not_blocked(&self, employee_id: &str) -> Result<(), ApiError> {
        if let Ok(Some(record)) = self.store.read_active_turn(employee_id) {
            if record.stall_locked {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session is stall-locked after 3 consecutive stalls; clear the stall lock first",
                ));
            }
            if record.recovery_required {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session requires manual recovery before starting",
                ));
            }
        }
        Ok(())
    }

    fn reserve_start_or_send(
        &self,
        employee: &Employee,
        actor_member_id: &str,
        params: &MemberSessionStartParams,
    ) -> Result<(Option<SessionView>, Option<LiveSessionHandle>), ApiError> {
        let emp_id = employee.id.as_str();
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.starting.contains(emp_id) {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "agent session start is already in progress",
            ));
        }
        let finished = if let Some(handle) = reg.live.get(emp_id) {
            let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            if !matches!(
                state.state,
                SessionState::Stopped | SessionState::StallLocked | SessionState::RecoveryRequired
            ) {
                if let Some(instruction) = &params.instruction {
                    let objective = build_objective_text(employee, actor_member_id, instruction);
                    if handle.tx.send(PumpCommand::SendTurn(objective)).is_ok() {
                        return Ok((Some(state.to_view()), None));
                    }
                    // Pump exited concurrently; fall through to reap & spawn fresh.
                    true
                } else {
                    return Ok((Some(state.to_view()), None));
                }
            } else {
                true
            }
        } else {
            false
        };
        let old = if finished {
            reg.live.remove(emp_id)
        } else {
            None
        };
        reg.starting.insert(emp_id.to_owned());
        Ok((None, old))
    }

    fn prepare_and_spawn(
        &self,
        actor_member_id: &str,
        employee: &Employee,
        params: &MemberSessionStartParams,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let planned = self.plan_session_launch(employee, params)?;
        self.issue_and_spawn_launch(actor_member_id, employee, params, planned)
    }

    fn plan_session_launch(
        &self,
        employee: &Employee,
        params: &MemberSessionStartParams,
    ) -> Result<PlannedSessionLaunch, ApiError> {
        let emp_id = employee.id.as_str().to_owned();
        let home = self
            .office
            .ensure_private_home(&emp_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not prepare agent home"))?;
        let workspace_real = resolve_workspace(&home, params.workspace.as_deref())?;
        let workspace_spec = canonical_or_spec(&workspace_real)?;

        let resolved = self.resolve_runtime_and_policy(&emp_id)?;
        let binary = self
            .config
            .runtimes
            .resolve_binary(resolved.adapter.as_ref(), None)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable"))?;

        let socket_str = self.config.office_socket.display().to_string();
        let office_socket_spec = SpecPath::try_from(socket_str.as_str()).ok();
        let denied_globs = self.denied_office_globs();
        let preset_cfg = build_session_policy(
            resolved.preset,
            resolved.sandbox_enabled,
            &workspace_spec,
            office_socket_spec.as_ref(),
            &denied_globs,
            1,
        )
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid permission preset"))?;

        let (runtime_dir, tmpdir) = self.prepare_run_dirs(&emp_id)?;
        let subagent_def =
            build_general_subagent_definition(preset_cfg.allowed_tools.as_deref(), None);
        let _ = write_subagent_definition(&runtime_dir, &subagent_def);

        let instructions = self.build_instructions(employee, &home, &workspace_real, &preset_cfg);
        let spec = build_session_spec(
            resolved.adapter.id(),
            employee,
            resolved.model_token,
            workspace_spec,
            instructions,
            &preset_cfg,
        )?;
        let spec_hash = spec.spec_hash();
        let run_id = format!("run-{}", next_entropy_hex(10)?);
        let session_id = format!("sess-{}", next_entropy_hex(10)?);
        let mark_str = format!("m-{}", next_entropy_hex(12)?);

        let ctx = LaunchContext {
            runtime_path: binary,
            runtime_version: None,
            runtime_dir: canonical_or_spec(&runtime_dir)?,
            tmpdir: canonical_or_spec(&tmpdir)?,
            office_env_names: office_env_names(),
            new_session_id: SessionId::try_from(session_id.as_str()).ok(),
            resume: None,
            config_dir: None,
        };
        let (plan, session) = resolved
            .adapter
            .plan_session(&spec, &ctx)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "runtime rejected session spec"))?;

        Ok(PlannedSessionLaunch {
            emp_id,
            run_id,
            session_id,
            mark_str,
            runtime_id: resolved.adapter.id().as_str().to_owned(),
            home,
            workspace_real,
            tmpdir,
            preset_cfg,
            spec_hash,
            auth_env: resolved.auth_env,
            ctx,
            plan,
            session,
        })
    }

    fn issue_and_spawn_launch(
        &self,
        actor_member_id: &str,
        employee: &Employee,
        params: &MemberSessionStartParams,
        planned: PlannedSessionLaunch,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let PlannedSessionLaunch {
            emp_id,
            run_id,
            session_id,
            mark_str,
            runtime_id,
            home,
            workspace_real,
            tmpdir,
            preset_cfg,
            spec_hash,
            auth_env,
            ctx,
            plan,
            session,
        } = planned;
        let binding = self
            .bindings
            .issue(&emp_id, &run_id, &session_id, 0)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not mint session token"))?;
        let mut binding_guard = BindingGuard::new(&self.bindings, &emp_id, &run_id);
        let env = self.build_environment(
            &home,
            &workspace_real,
            &tmpdir,
            &emp_id,
            &session_id,
            binding.expose_token(),
        )?;
        let mark = SessionMark::new(mark_str.clone())
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "invalid session mark"))?;
        let launch = org_supervise::runtime::prepare_with_auth(plan, &ctx, env, &auth_env, mark)
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "failed to prepare supervised session",
                )
            })?;

        let objective = params
            .instruction
            .as_deref()
            .map(|text| build_objective_text(employee, actor_member_id, text))
            .unwrap_or_default();
        let persisted_stalls = self
            .store
            .read_active_turn(&emp_id)
            .ok()
            .flatten()
            .map_or(0, |r| r.consecutive_stalls);

        let spawned = self.spawn_and_persist(
            launch.prepared,
            &emp_id,
            &run_id,
            &session_id,
            &mark_str,
            &objective,
            persisted_stalls,
            launch
                .lifecycle
                .runtime_session_id
                .as_ref()
                .map(|s| s.as_str()),
        )?;
        binding_guard.disarm();
        drop(binding_guard);

        Ok(PreparedSessionLaunch {
            emp_id,
            run_id,
            session_id,
            runtime_id,
            home,
            preset_cfg,
            spec_hash,
            objective,
            persisted_stalls,
            spawned,
            session,
            lifecycle: launch.lifecycle,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_and_persist(
        &self,
        prepared: org_supervise::PreparedSpawn,
        emp_id: &str,
        run_id: &str,
        session_id: &str,
        mark_str: &str,
        objective: &str,
        persisted_stalls: u32,
        runtime_session_id: Option<&str>,
    ) -> Result<org_supervise::Spawned, ApiError> {
        let pending = PendingStartRecord {
            version: 1,
            employee_id: emp_id.to_owned(),
            run_id: run_id.to_owned(),
            session_id: session_id.to_owned(),
            mark: mark_str.to_owned(),
            started_at: rfc3339_now(),
        };
        let recorded = prepared
            .record_mark(|_| self.store.write_pending_start(&pending))
            .map_err(|_| {
                self.bindings.revoke(emp_id, Some(run_id));
                ApiError::new(ErrorCode::StorageError, "could not record pending start")
            })?;
        let pending_child = recorded.spawn().map_err(|_| {
            self.bindings.revoke(emp_id, Some(run_id));
            let _ = self.store.clear_pending_start(emp_id);
            ApiError::new(ErrorCode::Unavailable, "failed to spawn agent process")
        })?;
        pending_child
            .record_identity(|ident| {
                self.bindings
                    .set_root_pid(emp_id, run_id, ident.pid as libc::pid_t);
                let active_turn = ActiveTurnRecord {
                    version: 1,
                    employee_id: emp_id.to_owned(),
                    run_id: run_id.to_owned(),
                    session_id: session_id.to_owned(),
                    runtime_session_id: runtime_session_id.map(str::to_owned),
                    mark: ident.mark.clone(),
                    pid: Some(ident.pid),
                    pgid: Some(ident.pgid),
                    started_identity: Some(ident.started.clone()),
                    boot_identity: Some(ident.boot.clone()),
                    objective: objective.to_owned(),
                    accepted_commands: Vec::new(),
                    resume_attempts: 0,
                    consecutive_stalls: persisted_stalls,
                    stall_locked: false,
                    recovery_required: false,
                    active: true,
                    updated_at: rfc3339_now(),
                };
                self.store.write_active_turn(&active_turn)?;
                self.store.clear_pending_start(emp_id)
            })
            .map_err(|_| {
                self.bindings.revoke(emp_id, Some(run_id));
                ApiError::new(ErrorCode::StorageError, "could not persist child identity")
            })
    }

    fn install_live_session(
        &self,
        prepared: PreparedSessionLaunch,
    ) -> Result<SessionView, ApiError> {
        let PreparedSessionLaunch {
            emp_id,
            run_id,
            session_id,
            runtime_id,
            home,
            preset_cfg,
            spec_hash,
            objective,
            persisted_stalls,
            spawned,
            session,
            lifecycle,
        } = prepared;
        let runtime_state = Arc::new(Mutex::new(SessionRuntimeState {
            employee_id: emp_id.clone(),
            office_id: self.office.info().office_id.clone(),
            state: SessionState::Starting,
            run_id: Some(run_id.clone()),
            session_id: Some(session_id),
            runtime_id: Some(runtime_id),
            runtime_session_id: lifecycle
                .runtime_session_id
                .as_ref()
                .map(|s| s.as_str().to_owned()),
            private_home: home.display().to_string(),
            permission_preset: preset_cfg.preset.as_str().to_owned(),
            sandbox_enabled: preset_cfg.sandbox.enabled,
            spec_hash,
            watchdog: StallWatchdog::from_persisted(
                self.config.stall_timeout,
                Instant::now(),
                persisted_stalls,
                false,
            ),
            recovery_required: false,
            last_reply: None,
            last_error: None,
            backgrounds: BackgroundRegistry::new(),
            pending_approvals: Vec::new(),
            policy: preset_cfg.policy,
            preset: preset_cfg.preset,
        }));
        let (tx, rx) = mpsc::channel();
        if !objective.is_empty() {
            let _ = tx.send(PumpCommand::SendTurn(objective));
        }
        let pump_ctx = PumpContext {
            employee_id: emp_id.clone(),
            run_id,
            store: self.store.clone(),
            bindings: self.bindings.clone(),
            state: Arc::clone(&runtime_state),
            stop_plan: lifecycle.stop,
            cleanup_paths: lifecycle.cleanup_after_exit,
        };
        let thread_name = format!("org-session-{}", &emp_id[..emp_id.len().min(8)]);
        let spawned_child = spawned;
        let thread = match thread::Builder::new().name(thread_name).spawn({
            let ctx = pump_ctx;
            move || run_pump(spawned_child, session, rx, ctx)
        }) {
            Ok(handle) => handle,
            Err(_) => {
                // Note: if spawn fails, the closure (holding spawned_child) drops, which reaps the
                // child via OwnedChild::drop, and we revoke the run binding explicitly.
                self.bindings.revoke(&emp_id, None);
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "could not start session pump thread",
                ));
            }
        };
        let view = runtime_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_view();
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.live.insert(
            emp_id,
            LiveSessionHandle {
                state: runtime_state,
                tx,
                thread: Some(thread),
            },
        );
        Ok(view)
    }

    fn load_preset_and_sandbox(
        &self,
        employee_id: &str,
    ) -> Result<(PermissionPreset, bool), ApiError> {
        let exec = self.office.load_agent_execution(employee_id).map_err(|_| {
            ApiError::new(ErrorCode::StorageError, "could not read execution config")
        })?;
        Ok(match exec {
            Some(cfg) => (cfg.permission_preset, true),
            None => (PermissionPreset::SafeAsk, true),
        })
    }

    fn resolve_runtime_and_policy(
        &self,
        employee_id: &str,
    ) -> Result<ResolvedRuntimePolicy, ApiError> {
        let exec = self.office.load_agent_execution(employee_id).map_err(|_| {
            ApiError::new(ErrorCode::StorageError, "could not load execution config")
        })?;
        let preset = exec
            .as_ref()
            .map(|e| e.permission_preset)
            .unwrap_or(PermissionPreset::SafeAsk);
        let mut runtime_key = "claude".to_owned();
        let mut model_token = None;
        let mut auth_env = Vec::new();
        if let Some(cfg) = exec {
            if let Some(profile) = self
                .office
                .load_model_profile(cfg.model_profile_id.as_str())
                .map_err(|_| {
                    ApiError::new(ErrorCode::StorageError, "could not load model profile")
                })?
            {
                let conn_id = profile
                    .generated_for_connection_id
                    .as_ref()
                    .or_else(|| profile.connection_ids.first());
                if let Some(conn_id) = conn_id {
                    self.apply_connection_auth(conn_id.as_str(), &mut runtime_key, &mut auth_env)?;
                }
                model_token = Token::try_from(cfg.model_id.as_str())
                    .ok()
                    .or_else(|| Token::try_from(profile.key.as_str()).ok());
            }
        }
        let adapter =
            self.config.runtimes.resolve(&runtime_key).map_err(|_| {
                ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable")
            })?;
        Ok(ResolvedRuntimePolicy {
            adapter,
            model_token,
            auth_env,
            preset,
            sandbox_enabled: true,
        })
    }

    fn apply_connection_auth(
        &self,
        conn_id: &str,
        runtime_key: &mut String,
        auth_env: &mut Vec<(OsString, OsString)>,
    ) -> Result<(), ApiError> {
        let Some(conn) = self.office.load_provider_connection(conn_id).map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not load provider connection",
            )
        })?
        else {
            return Ok(());
        };
        *runtime_key = conn.runtime_driver.clone();
        let secret = self
            .enrollment
            .vault()
            .load_unwrapped(conn.id.as_str())
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "provider credential could not be unwrapped",
                )
            })?;
        let Some(secret) = secret else {
            return Ok(());
        };
        let secret_str = std::str::from_utf8(&secret).map_err(|_| {
            ApiError::new(ErrorCode::Unavailable, "provider credential is not UTF-8")
        })?;
        let var_name = auth_env_var_for(&conn.runtime_driver, conn.auth_mode, secret_str);
        auth_env.push((OsString::from(var_name), OsString::from(secret_str)));
        Ok(())
    }

    fn prepare_run_dirs(&self, employee_id: &str) -> Result<(PathBuf, PathBuf), ApiError> {
        let base = self
            .office
            .durable_root()
            .join("local")
            .join("offices")
            .join(&self.office.info().office_id)
            .join("agents")
            .join(employee_id);
        let runtime_dir = base.join("runtime");
        let tmpdir = base.join("tmp");
        for dir in [&runtime_dir, &tmpdir] {
            ensure_owner_only_dir(dir)?;
        }
        Ok((runtime_dir, tmpdir))
    }

    fn build_instructions(
        &self,
        employee: &Employee,
        home: &Path,
        workspace: &Path,
        preset_cfg: &PresetConfig,
    ) -> String {
        let info = self.office.info();
        let role = employee
            .role_id
            .as_ref()
            .and_then(|id| self.office.load_role(id.as_str()).ok().flatten());
        let table = org_agent_cli::table::table();
        let input = InstructionInput {
            organization_name: &info.org_name,
            office_name: &info.office_name,
            employee_id: employee.id.as_str(),
            employee_name: &employee.name,
            handle: &employee.handle,
            team_name: employee.home_team_id.as_str(),
            role_name: role.as_ref().map(|r| r.name.as_str()),
            role_instructions: role.as_ref().map(|r| r.body.as_str()),
            agent_instructions: &employee.body,
            is_hr_lead: employee.builtin_key == Some(BuiltinKey::Hr),
            private_home: &home.display().to_string(),
            workspace: &workspace.display().to_string(),
            permission_preset: preset_cfg.preset.as_str(),
            sandbox_enabled: preset_cfg.sandbox.enabled,
        };
        assemble_standing_instructions(&input, &table)
    }

    fn build_environment(
        &self,
        home: &Path,
        workspace: &Path,
        tmpdir: &Path,
        employee_id: &str,
        session_id: &str,
        token: &str,
    ) -> Result<Environment, ApiError> {
        let helper_dir = RuntimeRegistry::resolve_helper_dir(self.config.helper_dir.as_deref());
        let captured_path =
            std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
        let forbidden = vec![home.to_path_buf(), workspace.to_path_buf()];
        let search_path = SearchPath::new(&helper_dir, &captured_path, &forbidden)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid helper search path"))?;
        let captured = vec![
            (OsString::from("HOME"), home.as_os_str().to_os_string()),
            (OsString::from("TMPDIR"), tmpdir.as_os_str().to_os_string()),
            (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
            (OsString::from("TERM"), OsString::from("dumb")),
        ];
        let info = self.office.info();
        let context = vec![
            (
                OsString::from("AGENT_OS_SOCKET"),
                self.config.office_socket.as_os_str().to_os_string(),
            ),
            (
                OsString::from("AGENT_OS_OFFICE"),
                OsString::from(&info.office_id),
            ),
            (OsString::from("AGENT_OS_ORG"), OsString::from(&info.org_id)),
            (
                OsString::from("AGENT_OS_AGENT_ID"),
                OsString::from(employee_id),
            ),
            (
                OsString::from("AGENT_OS_SESSION_ID"),
                OsString::from(session_id),
            ),
            (
                OsString::from("AGENT_OS_SESSION_TOKEN"),
                OsString::from(token),
            ),
        ];
        Environment::new(search_path, &captured)
            .and_then(|env| env.with_context(&context))
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not build session env"))
    }

    pub(crate) fn denied_office_globs(&self) -> Vec<PathPattern> {
        let mut out = Vec::new();
        let mut push_glob = |path: &Path| {
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let glob = format!("{}/**", resolved.display());
            if let Ok(pat) = PathPattern::try_from(glob.as_str()) {
                out.push(pat);
            }
        };
        push_glob(self.office.folder());
        let durable = self.office.durable_root();
        let office_local = durable
            .join("local")
            .join("offices")
            .join(&self.office.info().office_id);
        push_glob(&durable.join("state"));
        push_glob(&durable.join("journal"));
        push_glob(&office_local.join("auth"));
        push_glob(&office_local.join("sessions"));
        push_glob(&office_local.join("enroll-tmp"));
        if let Some(org_dir) = durable.parent() {
            push_glob(&org_dir.join("runtime"));
        }
        out
    }

    fn idle_view_for(&self, employee_id: &str) -> Result<SessionView, ApiError> {
        if let Some(last) = self
            .last_states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .cloned()
        {
            return Ok(last);
        }
        let persisted = self.store.read_active_turn(employee_id).ok().flatten();
        let (preset, sandbox_enabled) = self
            .load_preset_and_sandbox(employee_id)
            .unwrap_or((PermissionPreset::SafeAsk, true));
        let (state, consecutive_stalls, stall_locked, recovery_required, run_id, session_id, rsid) =
            match persisted {
                Some(rec) => {
                    let state = if rec.stall_locked {
                        SessionState::StallLocked
                    } else if rec.recovery_required {
                        SessionState::RecoveryRequired
                    } else {
                        SessionState::Idle
                    };
                    (
                        state,
                        rec.consecutive_stalls,
                        rec.stall_locked,
                        rec.recovery_required,
                        Some(rec.run_id),
                        Some(rec.session_id),
                        rec.runtime_session_id,
                    )
                }
                None => (SessionState::Idle, 0, false, false, None, None, None),
            };
        Ok(SessionView {
            employee_id: employee_id.to_owned(),
            office_id: self.office.info().office_id.clone(),
            state,
            run_id,
            session_id,
            runtime_id: None,
            runtime_session_id: rsid,
            private_home: self
                .office
                .agent_private_home_path(employee_id)
                .display()
                .to_string(),
            same_user_isolation_notice: SAME_USER_ISOLATION_NOTICE.to_owned(),
            permission_preset: preset.as_str().to_owned(),
            sandbox_enabled,
            consecutive_stalls,
            stall_locked,
            recovery_required,
            last_reply: None,
            last_error: None,
            background_tasks: Vec::new(),
            pending_approvals: Vec::new(),
            usage: Default::default(),
        })
    }
}

fn is_c3_rpc_method(method: &str) -> bool {
    matches!(
        method,
        "member.session_start"
            | "member.session_stop"
            | "member.session_inspect"
            | "member.session_grants"
            | "member.session_approval"
            | "member.session_clear_stall"
            | "provider.enrollment_start"
            | "provider.enrollment_status"
            | "provider.enrollment_submit"
            | "provider.enrollment_cancel"
            | "provider.credential_status"
            | "provider.credential_revoke"
    )
}

fn command_cache_meta(request: &Request) -> Option<(String, String)> {
    let cid = request.params.get("command_id")?.as_str()?.to_owned();
    // Hash params with SHA-256 and zeroize the serialized buffer so plaintext OAuth codes or
    // instructions are never retained in `command_cache` (`ENROLL-3`).
    let raw = zeroize::Zeroizing::new(serde_json::to_vec(&request.params).unwrap_or_default());
    let digest = ring::digest::digest(&ring::digest::SHA256, &raw);
    let mut fp = String::with_capacity(64);
    for b in digest.as_ref() {
        fp.push_str(&format!("{b:02x}"));
    }
    Some((cid, fp))
}

fn auth_env_var_for(driver: &str, mode: ProviderAuthMode, secret: &str) -> &'static str {
    let lower = driver.to_ascii_lowercase();
    if lower.contains("openai") || lower.contains("codex") {
        return "OPENAI_API_KEY";
    }
    if mode == ProviderAuthMode::SubscriptionCli || secret.starts_with("sk-ant-oat") {
        "CLAUDE_CODE_OAUTH_TOKEN"
    } else {
        "ANTHROPIC_API_KEY"
    }
}

fn parse_decision(decision_str: &str, reason: Option<String>) -> Result<Decision, ApiError> {
    match decision_str {
        "allow_once" | "allow_always" => Ok(Decision::AllowOnce),
        "deny" => Ok(Decision::Deny {
            message: reason.or_else(|| Some("Denied by organization owner.".to_owned())),
            stop_turn: false,
        }),
        _ => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "decision must be `allow_once`, `allow_always`, or `deny`",
        )),
    }
}

fn resolve_workspace(home: &Path, explicit: Option<&str>) -> Result<PathBuf, ApiError> {
    let path = explicit
        .map(PathBuf::from)
        .unwrap_or_else(|| home.to_path_buf());
    path.canonicalize().map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "workspace directory does not exist",
        )
    })
}

fn ensure_owner_only_dir(path: &Path) -> Result<(), ApiError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if !current.exists() {
            match org_platform::fs::create_owner_only_dir(&current) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(_) => {
                    return Err(ApiError::new(
                        ErrorCode::StorageError,
                        "could not create private session directory",
                    ))
                }
            }
        }
    }
    Ok(())
}

fn build_objective_text(employee: &Employee, requester: &str, text: &str) -> String {
    if employee.builtin_key == Some(BuiltinKey::Hr) {
        compose_turn_objective(&TurnTrigger::HrProposalRequest {
            requester: requester.to_owned(),
            request: text.to_owned(),
        })
    } else {
        compose_turn_objective(&TurnTrigger::DirectInstruction {
            text: text.to_owned(),
        })
    }
}

fn build_session_spec(
    runtime: RuntimeId,
    employee: &Employee,
    model: Option<Token>,
    workspace: SpecPath,
    instructions: String,
    preset_cfg: &PresetConfig,
) -> Result<SessionSpec, ApiError> {
    let safe_name = sanitize_agent_name(&employee.handle);
    let agent_name = AgentName::try_from(safe_name.as_str())
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid agent handle"))?;
    let title = employee.title_in_team.as_deref().unwrap_or("Agent");
    Ok(SessionSpec {
        runtime,
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: agent_name,
            description: format!("{} ({title})", employee.name),
            prompt: format!("You are {} (@{}).", employee.name, employee.handle),
        },
        model,
        effort: None,
        workspace,
        read_only_dirs: Vec::new(),
        instructions,
        permission_mode: preset_cfg.permission_mode,
        policy: preset_cfg.policy.clone(),
        sandbox: preset_cfg.sandbox.clone(),
        mcp_servers: Default::default(),
        tools: preset_cfg.allowed_tools.clone(),
    })
}

fn sanitize_agent_name(handle: &str) -> String {
    let mut out: String = handle
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if out.is_empty() || !out.as_bytes()[0].is_ascii_alphanumeric() {
        out.insert_str(0, "agent-");
    }
    out.truncate(64);
    out
}

fn office_env_names() -> Vec<EnvName> {
    [
        "AGENT_OS_SOCKET",
        "AGENT_OS_OFFICE",
        "AGENT_OS_ORG",
        "AGENT_OS_AGENT_ID",
        "AGENT_OS_SESSION_ID",
        "AGENT_OS_SESSION_TOKEN",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static env name"))
    .collect()
}

fn canonical_or_spec(path: &Path) -> Result<SpecPath, ApiError> {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.display().to_string();
    SpecPath::try_from(text.as_str())
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid workspace path"))
}

fn next_entropy_hex(bytes_len: usize) -> Result<String, ApiError> {
    random_hex(bytes_len)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "system entropy unavailable"))
}

fn handle_call<C: Call>(
    request: &Request,
    member: Option<&str>,
    f: impl FnOnce(&str, C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let Some(member_id) = member else {
        return Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
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
    match f(member_id, params) {
        Ok(res) => Response::ok::<C>(request.id, &res).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "could not encode response"),
            )
        }),
        Err(err) => Response::error(request.id, err),
    }
}

fn map_vault_err(err: EnrollmentVaultError) -> ApiError {
    let (code, msg) = match err {
        EnrollmentVaultError::InvalidInput(m) => (ErrorCode::InvalidRequest, m),
        EnrollmentVaultError::NotAuthorized(m) => (ErrorCode::NotAuthorized, m),
        EnrollmentVaultError::Conflict(m) => (ErrorCode::Conflict, m),
        EnrollmentVaultError::NotFound(m) => (ErrorCode::NotFound, m),
        EnrollmentVaultError::Expired(m) => (ErrorCode::Expired, m),
        EnrollmentVaultError::MissingSecretsKey
        | EnrollmentVaultError::Crypto(_)
        | EnrollmentVaultError::Io(_) => (
            ErrorCode::Unavailable,
            "provider credential operation unavailable".to_owned(),
        ),
    };
    ApiError::new(code, msg)
}
