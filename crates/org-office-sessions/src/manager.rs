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
    AgentIdentity, AgentName, Decision, EnvName, LaunchContext, PathPattern, RuntimeId, SessionId,
    SessionKind, SessionSpec, SpecPath, Token,
};
use org_office::organization_family::records::{
    BuiltinKey, Employee, EmployeeKind, EmployeeStatus, PermissionPreset,
};
use org_office::Office;
use org_proto::sessions::{
    MemberSessionApprovalDecide, MemberSessionClearStall, MemberSessionGrants,
    MemberSessionInspect, MemberSessionList, MemberSessionListParams, MemberSessionStart,
    MemberSessionStartParams, MemberSessionStop, MemberSessionTranscript,
    MemberSessionTranscriptParams, ProviderCredentialRevoke, ProviderCredentialStatus,
    ProviderEnrollmentCancel, ProviderEnrollmentStart, ProviderEnrollmentStatus,
    ProviderEnrollmentSubmit, SessionGrantsResult, SessionInspectResult, SessionListResult,
    SessionState, SessionTranscriptResult, SessionView,
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
use crate::pump::{run_pump, FailoverRequest, PumpCommand, PumpContext, SessionRuntimeState};
use crate::recovery::{
    ActiveTurnRecord, NativeProcessExitOracle, PendingStartRecord, ProcessExitOracle,
    RecoveryAction, RecoveryReport, SessionStore, StallWatchdog, DEFAULT_STALL_TIMEOUT,
};
use crate::runtime_registry::RuntimeRegistry;
use crate::subagents::{
    build_general_subagent_definition, write_subagent_definition, BackgroundRegistry,
};
use crate::subscription_usage::{
    collect_subscription_usage, list_subscription_usage, revoke_subscription_usage_credential,
    set_subscription_usage_credential, ProviderSubscriptionUsageCollector,
    SubscriptionUsageCollector,
};

use crate::wake::{
    SuspendedAskEntry, DEFAULT_HOURLY_WAKE_LIMIT, DEFAULT_SUSPEND_GRACE, DEFAULT_WAKE_WINDOW,
};

/// Configuration for [`SessionManager`].
#[derive(Clone)]
pub struct SessionManagerConfig {
    pub office_socket: PathBuf,
    pub helper_dir: Option<PathBuf>,
    pub stall_timeout: Duration,
    pub enrollment_timeout: Duration,
    pub hourly_wake_limit: usize,
    pub wake_window: Duration,
    pub suspend_grace: Duration,
    pub runtimes: RuntimeRegistry,
    pub enrollment_driver: Arc<dyn EnrollmentDriver>,
    pub subscription_usage_collector: Arc<dyn SubscriptionUsageCollector>,
    pub hq_permits: Option<Arc<dyn crate::router::HqPermitCoordinator>>,
}

impl SessionManagerConfig {
    pub fn production(office_socket: PathBuf) -> Self {
        Self {
            office_socket,
            helper_dir: None,
            stall_timeout: DEFAULT_STALL_TIMEOUT,
            enrollment_timeout: crate::credentials::DEFAULT_ENROLLMENT_TIMEOUT,
            hourly_wake_limit: DEFAULT_HOURLY_WAKE_LIMIT,
            wake_window: DEFAULT_WAKE_WINDOW,
            suspend_grace: DEFAULT_SUSPEND_GRACE,
            runtimes: RuntimeRegistry::production(),
            enrollment_driver: Arc::new(CliEnrollmentDriver),
            subscription_usage_collector: Arc::new(ProviderSubscriptionUsageCollector::production()),
            hq_permits: None,
        }
    }
}

pub(crate) struct LiveSessionHandle {
    pub state: Arc<Mutex<SessionRuntimeState>>,
    pub tx: Sender<PumpCommand>,
    pub thread: Option<JoinHandle<()>>,
    pub connection_id: Option<String>,
    pub route_digest: String,
    pub workspace: Option<String>,
    pub active_permit: Arc<Mutex<Option<(String, u64)>>>,
}

#[derive(Default)]
pub(crate) struct SessionRegistry {
    pub live: HashMap<String, LiveSessionHandle>,
    pub starting: HashSet<String>,
    pub draining: bool,
    pub shutting_down: bool,
    pub bg_spawning: usize,
    pub bg_workers: Vec<JoinHandle<()>>,
}

struct BgSpawnGuard<'a> {
    registry: &'a Mutex<SessionRegistry>,
}

impl Drop for BgSpawnGuard<'_> {
    fn drop(&mut self) {
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.bg_spawning = reg.bg_spawning.saturating_sub(1);
    }
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

struct PermitGuard<'a> {
    office: &'a Office,
    hq_permits: Option<&'a dyn crate::router::HqPermitCoordinator>,
    permit: Option<(String, u64)>,
}

impl<'a> PermitGuard<'a> {
    fn new(
        office: &'a Office,
        hq_permits: Option<&'a dyn crate::router::HqPermitCoordinator>,
        permit: Option<(String, u64)>,
    ) -> Self {
        Self {
            office,
            hq_permits,
            permit,
        }
    }

    fn take(&mut self) -> Option<(String, u64)> {
        self.permit.take()
    }
}

impl Drop for PermitGuard<'_> {
    fn drop(&mut self) {
        if let Some((pid, _)) = self.permit.take() {
            crate::router::release_turn_permit_any(self.office, self.hq_permits, &pid);
        }
    }
}

struct PlannedSessionLaunch {
    emp_id: String,
    run_id: String,
    session_id: String,
    mark_str: String,
    runtime_id: String,
    connection_id: Option<String>,
    route_digest: String,
    workspace_arg: Option<String>,
    permit: Option<(String, u64)>,
    task_ref: Option<String>,
    tried_connections: Vec<String>,
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
    connection_id: Option<String>,
    route_digest: String,
    workspace_arg: Option<String>,
    permit: Option<(String, u64)>,
    task_ref: Option<String>,
    tried_connections: Vec<String>,
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
/// Callback supplying drained watch-ended notices for an agent's next turn (`ADR-0024` d14).
pub type WatchNoticeProvider = Arc<dyn Fn(&str) -> Vec<String> + Send + Sync>;

pub struct SessionManager {
    pub(crate) office: Arc<Office>,
    pub(crate) config: SessionManagerConfig,
    pub(crate) store: SessionStore,
    pub(crate) bindings: RunBindingRegistry,
    enrollment: EnrollmentManager,
    pub(crate) registry: Arc<Mutex<SessionRegistry>>,
    pub(crate) last_states: Arc<Mutex<HashMap<String, SessionView>>>,
    pub(crate) open_required_asks: Arc<Mutex<HashMap<String, Vec<SuspendedAskEntry>>>>,
    pub(crate) budget_hold: Arc<Mutex<Option<String>>>,
    pub(crate) lifecycle_lock: Arc<Mutex<()>>,
    pub(crate) dispatching_wakes: Arc<Mutex<HashSet<String>>>,
    watch_notice_provider: Mutex<Option<WatchNoticeProvider>>,
    startup_recovery: Mutex<RecoveryReport>,
    command_cache: Mutex<HashMap<(String, String, String), CachedRpcResponse>>,
    self_ref: Mutex<Option<std::sync::Weak<SessionManager>>>,
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
            registry: Arc::new(Mutex::new(SessionRegistry::default())),
            last_states: Arc::new(Mutex::new(HashMap::new())),
            open_required_asks: Arc::new(Mutex::new(HashMap::new())),
            budget_hold: Arc::new(Mutex::new(None)),
            lifecycle_lock: Arc::new(Mutex::new(())),
            dispatching_wakes: Arc::new(Mutex::new(HashSet::new())),
            watch_notice_provider: Mutex::new(None),
            startup_recovery: Mutex::new(RecoveryReport::default()),
            command_cache: Mutex::new(HashMap::new()),
            self_ref: Mutex::new(None),
        }
    }

    /// Registers a weak self-reference so pump permit-release and §7 failover callbacks can wake
    /// waiting agents (`LIVE-2`) and relaunch on the next pool connection (`doc-88` §7).
    pub fn attach_self(this: &Arc<Self>) {
        *this.self_ref.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::downgrade(this));
    }

    pub(crate) fn weak_self(&self) -> Option<std::sync::Weak<Self>> {
        self.self_ref
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn bindings(&self) -> &RunBindingRegistry {
        &self.bindings
    }

    pub fn subscription_usage_list(
        &self,
        actor: &str,
        params: org_proto::costs::CostSubscriptionUsageListParams,
    ) -> Result<org_proto::costs::CostSubscriptionUsageListResult, ApiError> {
        list_subscription_usage(&self.office, self.enrollment.vault(), actor, params)
    }

    pub fn subscription_usage_collect(
        &self,
        actor: &str,
        params: org_proto::costs::CostSubscriptionUsageCollectParams,
    ) -> Result<org_proto::costs::CostSubscriptionUsageCollectResult, ApiError> {
        collect_subscription_usage(
            &self.office,
            self.enrollment.vault(),
            self.config.subscription_usage_collector.as_ref(),
            actor,
            params,
        )
    }

    pub fn subscription_usage_credential_set(
        &self,
        actor: &str,
        params: org_proto::costs::CostSubscriptionUsageCredentialSetParams,
    ) -> Result<org_proto::costs::CostSubscriptionUsageCredentialSetResult, ApiError> {
        set_subscription_usage_credential(&self.office, self.enrollment.vault(), actor, params)
    }

    pub fn subscription_usage_credential_revoke(
        &self,
        actor: &str,
        params: org_proto::costs::CostSubscriptionUsageCredentialRevokeParams,
    ) -> Result<org_proto::costs::CostSubscriptionUsageCredentialRevokeResult, ApiError> {
        revoke_subscription_usage_credential(&self.office, self.enrollment.vault(), actor, params)
    }

    /// Registers an office-local provider supplying pending channel watch-ended notices to prepend
    /// to an agent's next turn objective (`ADR-0024` d14).
    pub fn set_watch_notice_provider(&self, provider: WatchNoticeProvider) {
        *self
            .watch_notice_provider
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(provider);
    }

    fn prepend_watch_notices(&self, employee_id: &str, objective: String) -> String {
        let provider = self
            .watch_notice_provider
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let Some(provider) = provider else {
            return objective;
        };
        let notices = provider(employee_id);
        if notices.is_empty() {
            return objective;
        }
        let header = notices.join("\n");
        if objective.is_empty() {
            header
        } else {
            format!("{header}\n\n{objective}")
        }
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
            if let Some((pid, _)) = handle
                .active_permit
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
            {
                crate::router::release_turn_permit_any(
                    &self.office,
                    self.config.hq_permits.as_deref(),
                    &pid,
                );
                if !is_admissions_closed(self) {
                    self.process_due_wakes();
                }
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

    /// Stops every live session cleanly while waiting for any in-flight starts and background
    /// workers to settle, then flushes all deferred session usage into office accounting.
    pub fn stop_all(&self, reason: &str) {
        stop_all_inner(self, reason, false);
    }

    /// Permanently closes admissions, stops every live session, joins in-flight starts and
    /// background workers, and flushes all deferred session usage before the office shuts down.
    pub fn shutdown(&self, reason: &str) {
        stop_all_inner(self, reason, true);
    }

    /// Starts (or sends a direct turn to) an agent's supervised session (`AC #1`, `LIVE-9..13`).
    /// Per `ADR-0030` d12 (`LIVE-13`), a person's explicit `Run` clears a `stall_locked` state
    /// while still requiring explicit clearance for `recovery_required`.
    pub fn start_session(
        &self,
        actor_member_id: &str,
        params: &MemberSessionStartParams,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_or_person(actor_member_id)?;
        let employee = self.require_active_agent(&params.employee_id)?;
        let emp_id = employee.id.as_str();
        self.check_not_recovery_required(emp_id)?;
        self.clear_persisted_stall_only(emp_id);

        let initial_obj = params
            .instruction
            .as_deref()
            .map(|text| build_objective_text(&employee, actor_member_id, text));
        let (live_view, finished_handle) =
            self.reserve_start_or_send(&employee, initial_obj, None, params.workspace.as_deref())?;
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
        self.check_not_recovery_required(emp_id)?;
        self.clear_persisted_stall_only(emp_id);
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.append_history(
            emp_id,
            &serde_json::json!({
                "type": "turn_trigger",
                "kind": "run",
                "sender_id": actor_member_id,
                "source_id": params.command_id,
                "at": rfc3339_now(),
            }),
            &refs,
        );
        let prepared = self.prepare_and_spawn(actor_member_id, &employee, params, None)?;
        self.install_live_session(prepared)
    }

    /// Starts or resumes a supervised session autonomously (C6 required-ask resume, C8 peer wake,
    /// C10 always-available wake). Uses the exact same runtime, binding, and sandbox path as `Run`.
    pub(crate) fn spawn_or_send_autonomous(
        &self,
        employee: &Employee,
        objective: String,
        resume_runtime_session_id: Option<&str>,
        task_ref: Option<&str>,
        trigger_entry: serde_json::Value,
    ) -> Result<SessionView, ApiError> {
        let emp_id = employee.id.as_str();
        self.check_not_blocked(emp_id)?;
        let params = MemberSessionStartParams {
            command_id: format!("auto-{}", next_entropy_hex(8)?),
            employee_id: emp_id.to_owned(),
            instruction: Some(objective.clone()),
            workspace: None,
        };
        let (live_view, finished_handle) =
            self.reserve_start_or_send(employee, Some(objective.clone()), task_ref, None)?;
        if let Some(mut old) = finished_handle {
            if let Some(thread) = old.thread.take() {
                let _ = thread.join();
            }
        }
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        if let Some(view) = live_view {
            let _ = self.store.append_history(emp_id, &trigger_entry, &refs);
            return Ok(view);
        }
        let _guard = StartSlotGuard {
            registry: &self.registry,
            employee_id: emp_id.to_owned(),
        };
        self.check_not_blocked(emp_id)?;
        let _ = self.store.append_history(emp_id, &trigger_entry, &refs);
        let prepared = self.prepare_and_spawn_raw(
            employee,
            &params,
            objective,
            resume_runtime_session_id,
            task_ref,
        )?;
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
            let mut view = guard.to_view();
            let spec_hash = guard.spec_hash.clone();
            drop(guard);
            overlay_durable_wake_state(&self.store, target, &mut view);
            return Ok(SessionInspectResult {
                session: view,
                accepted_commands_count: accepted,
                spec_hash,
            });
        }
        let view = self.idle_view_for(employee.id.as_str())?;
        Ok(SessionInspectResult {
            session: view,
            accepted_commands_count: accepted,
            spec_hash: String::new(),
        })
    }

    /// Lists discovered live and historical conversation sessions in an agent's private home
    /// (`REQ-05`, `FC-150`). Only authorized people/owner or the agent itself may inspect.
    pub fn list_sessions(
        &self,
        caller_id: &str,
        params: &MemberSessionListParams,
    ) -> Result<SessionListResult, ApiError> {
        let target = params.employee_id.as_deref().unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person(caller_id)?;
        }
        if params.limit == Some(0) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "limit must be greater than zero",
            ));
        }
        let employee = self.require_agent(target)?;
        let home = self.office.agent_private_home_path(employee.id.as_str());
        let live_sid = self.live_runtime_session_id(employee.id.as_str());
        let secrets = self.active_redaction_secrets(employee.id.as_str());
        let refs: Vec<&str> = secrets.iter().map(|s| s.as_str()).collect();
        Ok(crate::transcript::list_agent_sessions(
            employee.id.as_str(),
            &home,
            live_sid.as_deref(),
            params.limit,
            &refs,
        ))
    }

    /// Reads, parses, nests sub-agents (`SUB-6`), filters, and redacts a conversation transcript
    /// on the office (`REQ-05`, `FC-150`).
    pub fn read_session_transcript(
        &self,
        caller_id: &str,
        params: &MemberSessionTranscriptParams,
    ) -> Result<SessionTranscriptResult, ApiError> {
        let target = params.employee_id.as_deref().unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person(caller_id)?;
        }
        if params.limit == Some(0) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "limit must be greater than zero",
            ));
        }
        if let Some(ref sid) = params.session_id {
            if !crate::transcript::is_safe_session_id(sid) {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid session_id",
                ));
            }
        }
        let employee = self.require_agent(target)?;
        let home = self.office.agent_private_home_path(employee.id.as_str());
        let live_sid = self.live_runtime_session_id(employee.id.as_str());
        let query = crate::transcript::TranscriptQuery {
            include_tool_calls: params.include_tool_calls.unwrap_or(true),
            include_file_changes: params.include_file_changes.unwrap_or(true),
            include_subagents: params.include_subagents.unwrap_or(true),
            cursor: params.cursor.unwrap_or(0),
            limit: params
                .limit
                .map(|n| n as usize)
                .unwrap_or(crate::transcript::DEFAULT_TRANSCRIPT_LIMIT),
        };
        let secrets = self.active_redaction_secrets(employee.id.as_str());
        let refs: Vec<&str> = secrets.iter().map(|s| s.as_str()).collect();
        crate::transcript::load_agent_transcript(
            employee.id.as_str(),
            &home,
            params.session_id.as_deref(),
            live_sid.as_deref(),
            query,
            &refs,
        )
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read session transcript"))?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "session transcript not found"))
    }

    fn active_redaction_secrets(&self, employee_id: &str) -> Vec<zeroize::Zeroizing<String>> {
        let mut secrets: Vec<zeroize::Zeroizing<String>> = self
            .bindings
            .active_tokens()
            .into_iter()
            .map(zeroize::Zeroizing::new)
            .collect();
        let Ok(Some(exec)) = self.office.load_agent_execution(employee_id) else {
            return secrets;
        };
        let Ok(Some(prof)) = self
            .office
            .load_model_profile(exec.model_profile_id.as_str())
        else {
            return secrets;
        };
        let conn_ids = prof
            .generated_for_connection_id
            .iter()
            .chain(prof.connection_ids.iter());
        for conn_id in conn_ids {
            if let Ok(Some(raw)) = self.enrollment.vault().load_unwrapped(conn_id.as_str()) {
                if let Ok(text) = std::str::from_utf8(&raw) {
                    secrets.push(zeroize::Zeroizing::new(text.to_owned()));
                }
            }
        }
        secrets
    }

    fn live_runtime_session_id(&self, employee_id: &str) -> Option<String> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let handle = reg.live.get(employee_id)?;
        let guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        if !matches!(
            guard.state,
            SessionState::Starting | SessionState::Running | SessionState::WaitingApproval
        ) {
            return None;
        }
        guard
            .runtime_session_id
            .clone()
            .or_else(|| guard.session_id.clone())
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
        if decision_str == "allow_always" && pending.escalation != "none" {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "allow_always is forbidden for escalated approval requests; use allow_once or deny",
            ));
        }
        let allow_always_tool = (decision_str == "allow_always" && pending.escalation == "none")
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
        let dead_handle = {
            let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            let should_remove = reg.live.get(employee_id).is_some_and(|handle| {
                let mut guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
                guard.watchdog.clear_stall_lock(Instant::now());
                guard.recovery_required = false;
                matches!(
                    guard.state,
                    SessionState::StallLocked
                        | SessionState::RecoveryRequired
                        | SessionState::Stopped
                        | SessionState::Suspended
                )
            });
            if should_remove {
                reg.live.remove(employee_id)
            } else {
                None
            }
        };
        if let Some(mut old) = dead_handle {
            if let Some(thread) = old.thread.take() {
                let _ = thread.join();
            }
        }
        self.last_states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(employee_id);
        let _ = self.process_due_wakes_for(employee_id);
        let live_view = {
            let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.get(employee_id).map(|handle| {
                handle
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .to_view()
            })
        };
        if let Some(mut view) = live_view {
            overlay_durable_wake_state(&self.store, employee_id, &mut view);
            return Ok(view);
        }
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
        if request.is::<MemberSessionList>() {
            return Some(handle_call::<MemberSessionList>(request, member, |m, p| {
                self.list_sessions(m, &p)
            }));
        }
        if request.is::<MemberSessionTranscript>() {
            return Some(handle_call::<MemberSessionTranscript>(
                request,
                member,
                |m, p| self.read_session_transcript(m, &p),
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
        if self
            .office
            .sync_status()
            .is_ok_and(|s| s.initial_snapshot_required)
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "satellite office is awaiting a complete HQ snapshot before running agents",
            ));
        }
        let emp = self.require_agent(employee_id)?;
        if emp.status != EmployeeStatus::Active {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "only active agents can start a session",
            ));
        }
        // Enforce local office placement at the shared admission boundary for both manual
        // `start_session` and autonomous wakes (`TASK-52` P7-#6).
        if self.office.is_multi_office().unwrap_or(false) {
            let owner_office = org_office::resolve_employee_office(&self.office, employee_id)
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::StorageError,
                        "could not resolve agent home office",
                    )
                })?;
            if owner_office != self.office.info().office_id {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    format!(
                        "agent is placed on office `{owner_office}` and cannot run on `{}`",
                        self.office.info().office_id
                    ),
                ));
            }
        }
        Ok(emp)
    }

    fn check_not_recovery_required(&self, employee_id: &str) -> Result<(), ApiError> {
        if let Ok(Some(record)) = self.store.read_active_turn(employee_id) {
            if record.recovery_required {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session requires manual recovery before starting",
                ));
            }
        }
        Ok(())
    }

    fn clear_persisted_stall_only(&self, employee_id: &str) {
        let _ = self.store.update_active_turn(employee_id, |persisted| {
            if persisted.stall_locked {
                persisted.consecutive_stalls = 0;
                persisted.stall_locked = false;
                persisted.updated_at = rfc3339_now();
            }
        });
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
        objective: Option<String>,
        task_ref: Option<&str>,
        requested_workspace: Option<&str>,
    ) -> Result<(Option<SessionView>, Option<LiveSessionHandle>), ApiError> {
        let emp_id = employee.id.as_str();
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        check_registry_startable(&reg, emp_id)?;
        let finished = if let Some(handle) = reg.live.get(emp_id) {
            let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            let workspace_matches = handle.workspace.as_deref() == requested_workspace;
            if workspace_matches && is_warm_state_reusable(state.state) {
                if let Some(text) = objective {
                    let text = self.prepend_watch_notices(emp_id, text);
                    let tx = handle.tx.clone();
                    let st = Arc::clone(&handle.state);
                    let conn_id = handle.connection_id.clone();
                    let route_digest = handle.route_digest.clone();
                    let active_permit = Arc::clone(&handle.active_permit);
                    drop(state);
                    // Reserve `starting` BEFORE releasing `reg` lock so no concurrent caller
                    // can race into a second spawn (`Finding #3`).
                    reg.starting.insert(emp_id.to_owned());
                    drop(reg);
                    return dispatch_warm_handle_turn(
                        self,
                        WarmHandleTurn {
                            emp_id,
                            task_ref,
                            tx,
                            state: st,
                            conn_id,
                            route_digest,
                            active_permit,
                            text,
                        },
                    );
                }
                return Ok((Some(state.to_view()), None));
            }
            if !workspace_matches {
                let _ = handle.tx.send(PumpCommand::Stop {
                    reason: "workspace_changed".into(),
                });
            }
            true
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
        resume_runtime_session_id: Option<&str>,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let objective = params
            .instruction
            .as_deref()
            .map(|text| build_objective_text(employee, actor_member_id, text))
            .unwrap_or_default();
        self.prepare_and_spawn_raw(employee, params, objective, resume_runtime_session_id, None)
    }

    fn prepare_and_spawn_raw(
        &self,
        employee: &Employee,
        params: &MemberSessionStartParams,
        objective: String,
        resume_runtime_session_id: Option<&str>,
        task_ref: Option<&str>,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let opts = LaunchRouteOpts {
            resume_runtime_session_id,
            task_ref,
            is_failover_switch: false,
            excluded_connections: HashSet::new(),
            tried_connections: Vec::new(),
        };
        let objective = self.prepend_watch_notices(employee.id.as_str(), objective);
        let planned = self.plan_session_launch(employee, params, opts)?;
        self.issue_and_spawn_launch(objective, planned)
    }

    /// Handles §7 connection failover after a pre-side-effect `rate_limited`, `quota_exhausted`,
    /// `auth_expired`, or `model_unavailable` turn failure (`doc-88` §7).
    pub(crate) fn failover_relaunch(&self, req: FailoverRequest) {
        run_failover_relaunch(self, req);
    }

    fn plan_session_launch(
        &self,
        employee: &Employee,
        params: &MemberSessionStartParams,
        opts: LaunchRouteOpts<'_>,
    ) -> Result<PlannedSessionLaunch, ApiError> {
        let emp_id = employee.id.as_str().to_owned();
        let home = self
            .office
            .ensure_private_home(&emp_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not prepare agent home"))?;
        let workspace_real = resolve_workspace(&home, params.workspace.as_deref())?;
        let workspace_spec = canonical_or_spec(&workspace_real)?;

        let admitted = crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
            office: &self.office,
            vault: self.enrollment.vault(),
            runtimes: &self.config.runtimes,
            hq_permits: self.config.hq_permits.as_deref(),
            employee_id: &emp_id,
            task_id: opts.task_ref,
            is_failover_switch: opts.is_failover_switch,
            excluded_connections: &opts.excluded_connections,
        })?;
        let mut permit_guard = PermitGuard::new(
            &self.office,
            self.config.hq_permits.as_deref(),
            admitted.permit,
        );
        let binary = self
            .config
            .runtimes
            .resolve_binary(admitted.adapter.as_ref(), None)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable"))?;

        let socket_str = self.config.office_socket.display().to_string();
        let office_socket_spec = SpecPath::try_from(socket_str.as_str()).ok();
        let denied_globs = self.denied_office_globs();
        let preset_cfg = build_session_policy(
            admitted.preset,
            admitted.sandbox_enabled,
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

        let instructions =
            self.build_instructions(employee, &home, &workspace_real, &preset_cfg, opts.task_ref)?;
        let spec = build_session_spec(
            admitted.adapter.id(),
            employee,
            admitted.model_token,
            workspace_spec,
            instructions,
            &preset_cfg,
        )?;
        let spec_hash = if admitted.route_digest_suffix.is_empty() {
            spec.spec_hash()
        } else {
            format!(
                "{}:route={}",
                spec.spec_hash(),
                admitted.route_digest_suffix
            )
        };
        let run_id = format!("run-{}", next_entropy_hex(10)?);
        // Claude Code (`--session-id`), Codex, and Agy accept canonical lowercase UUIDs;
        // Claude Code 2.1.323+ rejects non-UUID strings before the `initialize` handshake.
        let session_id = next_uuid_v4()?;
        let mark_str = format!("m-{}", next_entropy_hex(12)?);

        let resume_sid = opts
            .resume_runtime_session_id
            .and_then(|raw| SessionId::try_from(raw).ok());
        let new_sid = if resume_sid.is_some() {
            None
        } else {
            SessionId::try_from(session_id.as_str()).ok()
        };
        let host_config_dir = if admitted.auth_env.is_empty() {
            resolve_host_runtime_config_dir(admitted.adapter.id().as_str())
        } else {
            None
        };
        let ctx = LaunchContext {
            runtime_path: binary,
            runtime_version: None,
            runtime_dir: canonical_or_spec(&runtime_dir)?,
            tmpdir: canonical_or_spec(&tmpdir)?,
            office_env_names: office_env_names(),
            new_session_id: new_sid,
            resume: resume_sid,
            config_dir: host_config_dir,
        };
        let (plan, session) = admitted
            .adapter
            .plan_session(&spec, &ctx)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "runtime rejected session spec"))?;

        let mut launch_env = admitted.auth_env;
        launch_env.extend(admitted.endpoint_env);

        Ok(PlannedSessionLaunch {
            emp_id,
            run_id,
            session_id,
            mark_str,
            runtime_id: admitted.adapter.id().as_str().to_owned(),
            connection_id: admitted.connection_id,
            route_digest: admitted.route_digest_suffix,
            workspace_arg: params.workspace.clone(),
            permit: permit_guard.take(),
            task_ref: opts.task_ref.map(str::to_owned),
            tried_connections: opts.tried_connections,
            home,
            workspace_real,
            tmpdir,
            preset_cfg,
            spec_hash,
            auth_env: launch_env,
            ctx,
            plan,
            session,
        })
    }

    fn issue_and_spawn_launch(
        &self,
        objective: String,
        planned: PlannedSessionLaunch,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let PlannedSessionLaunch {
            emp_id,
            run_id,
            session_id,
            mark_str,
            runtime_id,
            connection_id,
            route_digest,
            workspace_arg,
            permit,
            task_ref,
            tried_connections,
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
        let mut permit_guard =
            PermitGuard::new(&self.office, self.config.hq_permits.as_deref(), permit);
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
            connection_id,
            route_digest,
            workspace_arg,
            permit: permit_guard.take(),
            task_ref,
            tried_connections,
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

    #[expect(
        clippy::too_many_arguments,
        reason = "spawn persistence records prepared process, identifiers, mark, objective, stalls, and runtime session"
    )]
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
        let runtime_state = Arc::new(Mutex::new(build_initial_runtime_state(
            self.office.info().office_id.clone(),
            self.config.stall_timeout,
            &prepared,
        )));
        let PreparedSessionLaunch {
            emp_id,
            run_id,
            connection_id,
            route_digest,
            workspace_arg,
            permit,
            task_ref,
            tried_connections,
            objective,
            spawned,
            mut session,
            lifecycle,
            ..
        } = prepared;
        let active_permit = Arc::new(Mutex::new(permit));
        let (tx, rx) = mpsc::channel();
        if !objective.is_empty() {
            let _ = tx.send(PumpCommand::SendTurn(objective));
        }
        let hooks = self.build_pump_hooks(&emp_id, route_digest.clone());
        let pump_ctx = PumpContext {
            employee_id: emp_id.clone(),
            run_id,
            connection_id: connection_id.clone(),
            active_permit: Arc::clone(&active_permit),
            workspace: workspace_arg.clone(),
            task_ref,
            tried_connections,
            store: self.store.clone(),
            bindings: self.bindings.clone(),
            state: Arc::clone(&runtime_state),
            stop_plan: lifecycle.stop,
            cleanup_paths: lifecycle.cleanup_after_exit,
            hooks,
        };
        if is_admissions_closed(self) {
            return Err(abort_pre_pump_shutdown(
                self,
                &emp_id,
                spawned,
                &mut session,
                &pump_ctx,
                &active_permit,
            ));
        }
        let thread = spawn_pump_thread(
            self,
            &emp_id,
            spawned,
            session,
            rx,
            pump_ctx,
            &active_permit,
        )?;
        let view = runtime_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_view();
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.draining || reg.shutting_down {
            drop(reg);
            return Err(abort_post_spawn_shutdown(
                self,
                &emp_id,
                tx,
                thread,
                &runtime_state,
                &active_permit,
            ));
        }
        reg.live.insert(
            emp_id,
            LiveSessionHandle {
                state: runtime_state,
                tx,
                thread: Some(thread),
                connection_id,
                route_digest,
                workspace: workspace_arg,
                active_permit,
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
        task_ref: Option<&str>,
    ) -> Result<String, ApiError> {
        let info = self.office.info();
        let role = employee
            .role_id
            .as_ref()
            .and_then(|id| self.office.load_role(id.as_str()).ok().flatten());
        let rendered_processes = match self
            .office
            .render_agent_process_instructions(
                employee.id.as_str(),
                None,
                task_ref,
                std::collections::BTreeMap::new(),
                128 * 1024,
            )
            .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
        {
            Ok(rendered) => rendered,
            Err(err) if task_ref.is_some() && err.code == ErrorCode::NotFound => self
                .office
                .render_agent_process_instructions(
                    employee.id.as_str(),
                    None,
                    None,
                    std::collections::BTreeMap::new(),
                    128 * 1024,
                )
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))??,
            Err(err) => return Err(err),
        };
        let exec_prompt = self
            .office
            .load_agent_execution(employee.id.as_str())
            .ok()
            .flatten()
            .map(|cfg| cfg.system_prompt_markdown);
        let combined_agent_instructions =
            combine_agent_instructions(&employee.body, exec_prompt.as_deref());
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
            process_instructions: (!rendered_processes.text.is_empty())
                .then_some(rendered_processes.text.as_str()),
            agent_instructions: &combined_agent_instructions,
            is_hr_lead: employee.builtin_key == Some(BuiltinKey::Hr),
            private_home: &home.display().to_string(),
            workspace: &workspace.display().to_string(),
            permission_preset: preset_cfg.preset.as_str(),
            sandbox_enabled: preset_cfg.sandbox.enabled,
        };
        Ok(assemble_standing_instructions(&input, &table))
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
        push_glob(&durable.join("authority"));
        push_glob(&durable.join("private"));
        push_glob(&durable.join("messaging"));
        push_glob(&durable.join("budgets"));
        push_glob(&durable.join("accounting"));
        push_glob(&durable.join("recovery"));
        push_glob(&office_local.join("auth"));
        push_glob(&office_local.join("connections"));
        push_glob(&office_local.join("sessions"));
        push_glob(&office_local.join("enroll-tmp"));
        push_glob(&office_local.join("security"));
        push_glob(&office_local.join("subscription-usage"));
        if let Some(org_dir) = durable.parent() {
            push_glob(&org_dir.join("runtime"));
        }
        out
    }

    fn idle_view_for(&self, employee_id: &str) -> Result<SessionView, ApiError> {
        if let Some(mut last) = self
            .last_states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .cloned()
        {
            overlay_durable_wake_state(&self.store, employee_id, &mut last);
            return Ok(last);
        }
        let (preset, sandbox_enabled) = self
            .load_preset_and_sandbox(employee_id)
            .unwrap_or((PermissionPreset::SafeAsk, true));
        Ok(build_idle_session_view(
            &self.store,
            employee_id,
            &self.office.info().office_id,
            self.office.agent_private_home_path(employee_id),
            preset,
            sandbox_enabled,
        ))
    }
}

pub(crate) fn is_admissions_closed(mgr: &SessionManager) -> bool {
    let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    reg.draining || reg.shutting_down
}

pub(crate) fn spawn_bg_worker(
    mgr: &Arc<SessionManager>,
    name: &str,
    f: impl FnOnce(Arc<SessionManager>) + Send + 'static,
) {
    {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.draining || reg.shutting_down {
            return;
        }
        reg.bg_workers.retain(|h| !h.is_finished());
        reg.bg_spawning += 1;
    }
    let _spawn_guard = BgSpawnGuard {
        registry: &mgr.registry,
    };
    let cloned = Arc::clone(mgr);
    if let Ok(handle) = thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || f(cloned))
    {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.bg_workers.push(handle);
    }
}

fn stop_all_inner(mgr: &SessionManager, reason: &str, permanent: bool) {
    {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.draining = true;
        if permanent {
            reg.shutting_down = true;
        }
    }
    loop {
        let ids: Vec<String> = {
            let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.keys().cloned().collect()
        };
        for id in ids {
            let _ = mgr.stop_session(&id, reason);
        }
        let bg_handles: Vec<JoinHandle<()>> = {
            let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
            std::mem::take(&mut reg.bg_workers)
        };
        for handle in bg_handles {
            let _ = handle.join();
        }
        let settled = {
            let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.is_empty()
                && reg.starting.is_empty()
                && reg.bg_spawning == 0
                && reg.bg_workers.is_empty()
        };
        if settled {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let _ = mgr.flush_all_pending_usage();
    if !permanent {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.draining = false;
    }
}

fn check_registry_startable(reg: &SessionRegistry, emp_id: &str) -> Result<(), ApiError> {
    if reg.draining || reg.shutting_down {
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "office session manager is shutting down",
        ));
    }
    if reg.starting.contains(emp_id) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session start is already in progress",
        ));
    }
    Ok(())
}

fn is_warm_state_reusable(state: SessionState) -> bool {
    !matches!(
        state,
        SessionState::Stopped
            | SessionState::Stopping
            | SessionState::Suspended
            | SessionState::StallLocked
            | SessionState::RecoveryRequired
    )
}

fn build_initial_runtime_state(
    office_id: String,
    stall_timeout: Duration,
    prepared: &PreparedSessionLaunch,
) -> SessionRuntimeState {
    SessionRuntimeState {
        employee_id: prepared.emp_id.clone(),
        office_id,
        state: SessionState::Starting,
        run_id: Some(prepared.run_id.clone()),
        session_id: Some(prepared.session_id.clone()),
        runtime_id: Some(prepared.runtime_id.clone()),
        runtime_session_id: prepared
            .lifecycle
            .runtime_session_id
            .as_ref()
            .map(|s| s.as_str().to_owned()),
        private_home: prepared.home.display().to_string(),
        permission_preset: prepared.preset_cfg.preset.as_str().to_owned(),
        sandbox_enabled: prepared.preset_cfg.sandbox.enabled,
        spec_hash: prepared.spec_hash.clone(),
        watchdog: StallWatchdog::from_persisted(
            stall_timeout,
            Instant::now(),
            prepared.persisted_stalls,
            false,
        ),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: BackgroundRegistry::new(),
        pending_approvals: Vec::new(),
        policy: prepared.preset_cfg.policy.clone(),
        preset: prepared.preset_cfg.preset,
        allowed_tools: prepared.preset_cfg.allowed_tools.clone(),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
    }
}

fn spawn_pump_thread(
    mgr: &SessionManager,
    emp_id: &str,
    spawned: org_supervise::Spawned,
    session: Box<dyn org_agent_runtime::Session>,
    rx: mpsc::Receiver<PumpCommand>,
    pump_ctx: PumpContext,
    active_permit: &Arc<Mutex<Option<(String, u64)>>>,
) -> Result<JoinHandle<()>, ApiError> {
    let thread_name = format!("org-session-{}", &emp_id[..emp_id.len().min(8)]);
    match thread::Builder::new()
        .name(thread_name)
        .spawn(move || run_pump(spawned, session, rx, pump_ctx))
    {
        Ok(handle) => Ok(handle),
        Err(_) => {
            mgr.bindings.revoke(emp_id, None);
            if let Some((pid, _)) = active_permit
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
            {
                crate::router::release_turn_permit_any(
                    &mgr.office,
                    mgr.config.hq_permits.as_deref(),
                    &pid,
                );
            }
            Err(ApiError::new(
                ErrorCode::Unavailable,
                "could not start session pump thread",
            ))
        }
    }
}

fn abort_pre_pump_shutdown(
    mgr: &SessionManager,
    emp_id: &str,
    spawned: org_supervise::Spawned,
    session: &mut Box<dyn org_agent_runtime::Session>,
    pump_ctx: &PumpContext,
    active_permit: &Arc<Mutex<Option<(String, u64)>>>,
) -> ApiError {
    let cleanup_verified = crate::pump::finalize_child(
        spawned.child,
        spawned.stdin,
        session,
        pump_ctx,
        org_supervise::StopMode::Graceful,
        false,
    );
    if let Some((pid, _)) = active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        if cleanup_verified {
            crate::router::release_turn_permit_any(
                &mgr.office,
                mgr.config.hq_permits.as_deref(),
                &pid,
            );
        } else {
            let _ = mgr.office.mark_turn_permit_release_pending(&pid);
        }
    }
    persist_shutdown_cancellation(mgr, emp_id, &pump_ctx.run_id, !cleanup_verified)
}

fn abort_post_spawn_shutdown(
    mgr: &SessionManager,
    emp_id: &str,
    tx: Sender<PumpCommand>,
    thread: JoinHandle<()>,
    runtime_state: &Arc<Mutex<SessionRuntimeState>>,
    active_permit: &Arc<Mutex<Option<(String, u64)>>>,
) -> ApiError {
    mgr.bindings.revoke(emp_id, None);
    let _ = tx.send(PumpCommand::Stop {
        reason: "office shutdown".into(),
    });
    let joined_ok = thread.join().is_ok();
    let (cancelled_run_id, recovery_required) = {
        let st = runtime_state.lock().unwrap_or_else(|p| p.into_inner());
        (st.run_id.clone(), !joined_ok || st.recovery_required)
    };
    if let Some((pid, _)) = active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        // If `run_pump` did not finish cleanly to take the permit itself, fail closed (`AE-07`).
        let _ = mgr.office.mark_turn_permit_release_pending(&pid);
    }
    if let Some(cancelled_run_id) = cancelled_run_id {
        return persist_shutdown_cancellation(mgr, emp_id, &cancelled_run_id, recovery_required);
    }
    ApiError::new(
        ErrorCode::Unavailable,
        "office session manager is shutting down",
    )
}

fn persist_shutdown_cancellation(
    mgr: &SessionManager,
    emp_id: &str,
    run_id: &str,
    recovery_required: bool,
) -> ApiError {
    if let Err(io_err) = mgr.store.update_active_turn(emp_id, |rec| {
        if rec.run_id == run_id {
            rec.active = false;
            if recovery_required {
                rec.recovery_required = true;
            }
            rec.updated_at = rfc3339_now();
        }
    }) {
        eprintln!(
            "org-office-sessions: error: failed to persist shutdown cancellation for {emp_id}: {io_err}"
        );
        return ApiError::new(
            ErrorCode::StorageError,
            "could not persist session shutdown cancellation",
        );
    }
    ApiError::new(
        ErrorCode::Unavailable,
        "office session manager is shutting down",
    )
}

struct LaunchRouteOpts<'a> {
    resume_runtime_session_id: Option<&'a str>,
    task_ref: Option<&'a str>,
    is_failover_switch: bool,
    excluded_connections: HashSet<String>,
    tried_connections: Vec<String>,
}

struct WarmHandleTurn<'a> {
    emp_id: &'a str,
    task_ref: Option<&'a str>,
    tx: Sender<PumpCommand>,
    state: Arc<Mutex<SessionRuntimeState>>,
    conn_id: Option<String>,
    route_digest: String,
    active_permit: Arc<Mutex<Option<(String, u64)>>>,
    text: String,
}

fn dispatch_warm_handle_turn(
    mgr: &SessionManager,
    turn: WarmHandleTurn<'_>,
) -> Result<(Option<SessionView>, Option<LiveSessionHandle>), ApiError> {
    // Caller already reserved `reg.starting` for `turn.emp_id`. Release it on warm success or
    // error, and keep it reserved only when transferring to cold-start (`Ok((None, old))`).
    let slot_guard = StartSlotGuard {
        registry: &mgr.registry,
        employee_id: turn.emp_id.to_owned(),
    };
    let needs_permit = turn.conn_id.is_some()
        && turn
            .active_permit
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_none();
    if !needs_permit {
        if turn.tx.send(PumpCommand::SendTurn(turn.text)).is_ok() {
            let view = turn
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .to_view();
            return Ok((Some(view), None));
        }
        let old = mgr
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .live
            .remove(turn.emp_id);
        std::mem::forget(slot_guard);
        return Ok((None, old));
    }
    let cid = turn.conn_id.as_deref().unwrap_or_default();
    let digest = (!turn.route_digest.is_empty()).then_some(turn.route_digest.as_str());
    let granted =
        match acquire_or_reroute_warm_permit(mgr, cid, turn.emp_id, turn.task_ref, digest)? {
            Ok(p) => p,
            Err(old) => {
                std::mem::forget(slot_guard);
                return Ok((None, old));
            }
        };
    let mut pguard = PermitGuard::new(&mgr.office, mgr.config.hq_permits.as_deref(), Some(granted));
    {
        let mut slot = turn.active_permit.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            *slot = pguard.take();
        }
    }
    if turn.tx.send(PumpCommand::SendTurn(turn.text)).is_ok() {
        let view = turn
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_view();
        return Ok((Some(view), None));
    }
    if let Some((pid, _)) = turn
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        crate::router::release_turn_permit_any(&mgr.office, mgr.config.hq_permits.as_deref(), &pid);
    }
    let old = mgr
        .registry
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .live
        .remove(turn.emp_id);
    std::mem::forget(slot_guard);
    Ok((None, old))
}

fn acquire_or_reroute_warm_permit(
    mgr: &SessionManager,
    cid: &str,
    emp_id: &str,
    task_ref: Option<&str>,
    route_digest: Option<&str>,
) -> Result<Result<(String, u64), Option<LiveSessionHandle>>, ApiError> {
    match crate::router::acquire_warm_permit_for_office(
        &mgr.office,
        mgr.config.hq_permits.as_deref(),
        cid,
        emp_id,
        task_ref,
        route_digest,
    ) {
        crate::router::WarmPermitOutcome::Granted(p) => Ok(Ok(p)),
        crate::router::WarmPermitOutcome::Refused(err) => Err(err),
        crate::router::WarmPermitOutcome::Unavailable {
            multi_connection: false,
            reason,
        } => Err(ApiError::new(
            ErrorCode::Conflict,
            format!("POOL_EXHAUSTED: [{reason}]"),
        )),
        crate::router::WarmPermitOutcome::StaleWarm
        | crate::router::WarmPermitOutcome::Unavailable {
            multi_connection: true,
            ..
        } => {
            let old = mgr
                .registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .live
                .remove(emp_id);
            if let Some(ref handle) = old {
                let _ = handle.tx.send(PumpCommand::Stop {
                    reason: "pool_reroute".into(),
                });
            }
            Ok(Err(old))
        }
    }
}

fn run_failover_relaunch(mgr: &SessionManager, req: FailoverRequest) {
    match req.reason {
        "rate_limited" | "quota_exhausted" => {
            let _ = mgr.office.record_connection_cooldown_internal(
                &req.failed_connection_id,
                None,
                req.rate_limit_resets_at.as_deref(),
                None,
            );
            crate::router::report_cooldown_any(
                &mgr.office,
                mgr.config.hq_permits.as_deref(),
                &req.failed_connection_id,
                req.rate_limit_resets_at.as_deref(),
                Some(req.reason),
            );
        }
        "auth_expired" => {
            let _ = mgr
                .office
                .mark_connection_auth_expired_internal(&req.failed_connection_id, req.reason);
        }
        _ => {}
    }
    let dead_handle = {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.draining || reg.shutting_down || reg.starting.contains(&req.employee_id) {
            return;
        }
        if let Some(existing) = reg.live.get(&req.employee_id) {
            let matches_failed_run = existing
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .run_id
                .as_deref()
                == Some(req.failed_run_id.as_str());
            if !matches_failed_run {
                // A newer replacement pump is already registered; leave it intact so shutdown can stop it.
                return;
            }
        }
        let old = reg.live.remove(&req.employee_id);
        reg.starting.insert(req.employee_id.clone());
        old
    };
    if let Some(mut old) = dead_handle {
        let _ = old.tx.send(PumpCommand::Stop {
            reason: "pool_failover".into(),
        });
        if let Some(thread) = old.thread.take() {
            let _ = thread.join();
        }
    }
    let _guard = StartSlotGuard {
        registry: &mgr.registry,
        employee_id: req.employee_id.clone(),
    };
    let Ok(employee) = mgr.require_active_agent(&req.employee_id) else {
        return;
    };
    let Ok(cmd_suffix) = next_entropy_hex(8) else {
        return;
    };
    let params = MemberSessionStartParams {
        command_id: format!("failover-{cmd_suffix}"),
        employee_id: req.employee_id.clone(),
        instruction: Some(req.objective.clone()),
        workspace: req.workspace.clone(),
    };
    let excluded: HashSet<String> = req.tried_connections.iter().cloned().collect();
    let opts = LaunchRouteOpts {
        resume_runtime_session_id: None,
        task_ref: req.task_ref.as_deref(),
        is_failover_switch: true,
        excluded_connections: excluded,
        tried_connections: req.tried_connections.clone(),
    };
    let objective = mgr.prepend_watch_notices(employee.id.as_str(), req.objective.clone());
    let launch_res = mgr
        .plan_session_launch(&employee, &params, opts)
        .and_then(|planned| mgr.issue_and_spawn_launch(objective, planned));
    match launch_res {
        Ok(prepared) => {
            let _ = mgr.install_live_session(prepared);
        }
        Err(err) => record_failover_exhausted(mgr, &req, &err),
    }
}

fn record_failover_exhausted(mgr: &SessionManager, req: &FailoverRequest, err: &ApiError) {
    let note = serde_json::json!({
        "type": "pool_failover_exhausted",
        "failed_connection_id": req.failed_connection_id,
        "reason": req.reason,
        "error": err.message,
        "at": rfc3339_now(),
    });
    let _ = mgr.store.append_history(&req.employee_id, &note, &[]);
    let mut last_view = mgr.idle_view_for(&req.employee_id).unwrap_or_else(|_| {
        build_idle_session_view(
            &mgr.store,
            &req.employee_id,
            &mgr.office.info().office_id,
            mgr.office.agent_private_home_path(&req.employee_id),
            PermissionPreset::SafeAsk,
            true,
        )
    });
    last_view.last_error = Some(err.message.clone());
    mgr.last_states
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(req.employee_id.clone(), last_view);
    // Queue a Recovery wake so the exhausted turn automatically retries when cooldown/capacity clears.
    let ts = crate::credentials::now_ms();
    let _lock = mgr.lifecycle_lock.lock().unwrap_or_else(|p| p.into_inner());
    let mut queue = mgr
        .store
        .read_pending_wakes(&req.employee_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::wake::PendingWakeQueueRecord::empty(&req.employee_id));
    queue.push_dedup(crate::wake::PendingWakeItem {
        item_id: format!("failover:{}:{}", req.failed_connection_id, ts),
        kind: crate::wake::WakeKind::Recovery,
        sender_id: mgr.office.owner_member_id().to_owned(),
        sender_name: "Office Supervisor".into(),
        sender_is_person: true,
        conversation_id: None,
        source_id: req.failed_connection_id.clone(),
        summary: req.objective.clone(),
        enqueued_at_ms: ts,
    });
    queue.set_hold(crate::wake::WakeHoldReason::AdmissionRefused, ts, None);
    let tokens = mgr.bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let _ = mgr.store.write_pending_wakes_redacted(&queue, &refs);
}

fn overlay_durable_wake_state(store: &SessionStore, employee_id: &str, view: &mut SessionView) {
    let suspended = store.read_suspended_state(employee_id).ok().flatten();
    let pending_wakes = store.read_pending_wakes(employee_id).ok().flatten();
    let blocked = view.stall_locked || view.recovery_required;
    if let Some(ref susp) = suspended {
        if !blocked
            && !matches!(
                view.state,
                SessionState::Starting | SessionState::Running | SessionState::WaitingApproval
            )
        {
            view.state = SessionState::Suspended;
        }
        let (w_id, w_name) = susp.primary_waiting_target();
        view.suspended_on_asks = susp.asks.iter().map(|a| a.ask_id.clone()).collect();
        view.waiting_for_member_id = w_id;
        view.waiting_for_name = w_name;
        view.suspended_since = Some(susp.suspended_since.clone());
        if view.runtime_session_id.is_none() {
            view.runtime_session_id = susp.runtime_session_id.clone();
        }
    } else {
        view.suspended_on_asks.clear();
        view.waiting_for_member_id = None;
        view.waiting_for_name = None;
        view.suspended_since = None;
    }
    let (count, reason) = extract_pending_wake_fields(pending_wakes.as_ref(), suspended.as_ref());
    view.pending_wake_count = count;
    view.pending_wake_reason = reason;
}

fn build_idle_session_view(
    store: &SessionStore,
    employee_id: &str,
    office_id: &str,
    private_home: PathBuf,
    preset: PermissionPreset,
    sandbox_enabled: bool,
) -> SessionView {
    let suspended = store.read_suspended_state(employee_id).ok().flatten();
    let pending_wakes = store.read_pending_wakes(employee_id).ok().flatten();
    let persisted = store.read_active_turn(employee_id).ok().flatten();
    let (mut state, consecutive_stalls, stall_locked, recovery_required, run_id, session_id, rsid) =
        extract_persisted_turn(persisted);
    let (suspended_on_asks, waiting_for_member_id, waiting_for_name, suspended_since) =
        extract_suspended_fields(
            suspended.as_ref(),
            stall_locked || recovery_required,
            &mut state,
        );
    let (pending_wake_count, pending_wake_reason) =
        extract_pending_wake_fields(pending_wakes.as_ref(), suspended.as_ref());
    SessionView {
        employee_id: employee_id.to_owned(),
        office_id: office_id.to_owned(),
        state,
        run_id,
        session_id,
        runtime_id: None,
        runtime_session_id: rsid.or_else(|| suspended.and_then(|s| s.runtime_session_id)),
        private_home: private_home.display().to_string(),
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
        suspended_on_asks,
        waiting_for_member_id,
        waiting_for_name,
        suspended_since,
        pending_wake_count,
        pending_wake_reason,
    }
}

fn extract_persisted_turn(
    persisted: Option<ActiveTurnRecord>,
) -> (
    SessionState,
    u32,
    bool,
    bool,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let Some(rec) = persisted else {
        return (SessionState::Idle, 0, false, false, None, None, None);
    };
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

fn extract_suspended_fields(
    suspended: Option<&crate::wake::SuspendedStateRecord>,
    blocked: bool,
    state: &mut SessionState,
) -> (Vec<String>, Option<String>, Option<String>, Option<String>) {
    let Some(susp) = suspended else {
        return (Vec::new(), None, None, None);
    };
    if !blocked {
        *state = SessionState::Suspended;
    }
    let (w_id, w_name) = susp.primary_waiting_target();
    (
        susp.asks.iter().map(|a| a.ask_id.clone()).collect(),
        w_id,
        w_name,
        Some(susp.suspended_since.clone()),
    )
}

fn extract_pending_wake_fields(
    pending_wakes: Option<&crate::wake::PendingWakeQueueRecord>,
    suspended: Option<&crate::wake::SuspendedStateRecord>,
) -> (u32, Option<String>) {
    match pending_wakes {
        Some(q) if !q.items.is_empty() => (
            q.items.len() as u32,
            q.hold_reason.map(|r| r.as_str().to_owned()),
        ),
        _ => (
            0,
            suspended.and_then(|s| s.hold_reason.map(|r| r.as_str().to_owned())),
        ),
    }
}

fn is_c3_rpc_method(method: &str) -> bool {
    matches!(
        method,
        "member.session_start"
            | "member.session_stop"
            | "member.session_inspect"
            | "member.session_list"
            | "member.session_transcript"
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

fn combine_agent_instructions(profile_body: &str, system_prompt: Option<&str>) -> String {
    let body = profile_body.trim();
    let prompt = system_prompt.map(str::trim).unwrap_or("");
    match (prompt.is_empty(), body.is_empty()) {
        (true, true) => String::new(),
        (false, true) => prompt.to_owned(),
        (true, false) => body.to_owned(),
        (false, false) if prompt == body => prompt.to_owned(),
        (false, false) => format!("{prompt}\n\n{body}"),
    }
}

fn next_uuid_v4() -> Result<String, ApiError> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "system entropy unavailable"))?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    ))
}

/// Locates the host user's existing runtime configuration directory (e.g. `~/.claude`, `~/.codex`,
/// `~/.gemini`) when no vault credential is injected, so local OAuth-authenticated CLI runtimes
/// can authenticate even while `HOME` is isolated to the agent's private home. Adapters add
/// S2 deny rules over `config_dir` so agent tools cannot read or modify credentials.
fn resolve_host_runtime_config_dir(runtime_id: &str) -> Option<SpecPath> {
    let subdir = match runtime_id {
        "claude" => ".claude",
        "codex" => ".codex",
        "agy" => ".gemini",
        _ => return None,
    };
    let host_home = std::env::var_os("HOME").map(PathBuf::from)?;
    let candidate = host_home.join(subdir);
    if !candidate.is_dir() {
        return None;
    }
    let resolved = candidate.canonicalize().unwrap_or(candidate);
    let text = resolved.display().to_string();
    SpecPath::try_from(text.as_str()).ok()
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
