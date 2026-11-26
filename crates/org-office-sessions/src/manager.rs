//! Production office `SessionManager`: spawns supervised agent sessions, coordinates
//! concurrency, enforces owner/agent authorization, persists stall/recovery state, and
//! dispatches all C3 RPCs with idempotent `command_id` replay (`TASK-42`).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    AgentIdentity, AgentName, Decision, EnvName, LaunchContext, PathPattern, RuntimeId,
    SandboxSpec, SessionId, SessionKind, SessionSpec, SpecPath, Token,
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
use crate::preset::{
    apply_worktree_sandbox_rules, build_grants_summary, build_session_policy, PresetConfig,
};
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
    MidTurnLimits, MidTurnRateTracker, SuspendedAskEntry, DEFAULT_HOURLY_WAKE_LIMIT,
    DEFAULT_SUSPEND_GRACE, DEFAULT_WAKE_WINDOW,
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
    pub mid_turn_limits: MidTurnLimits,
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
            mid_turn_limits: MidTurnLimits::default(),
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
    pub oauth_expires_at_ms: Option<u64>,
    pub workspace: Option<String>,
    pub active_permit: Arc<Mutex<Option<(String, u64)>>>,
    pub supports_mid_turn: bool,
    /// Set of sibling employee IDs whose directories were pre-created and masked when this
    /// session launched under Linux Claude `bwrap` (`TASK-166`). `None` for non-bwrap sessions.
    pub bwrap_masked_siblings: Option<std::collections::BTreeSet<String>>,
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

pub(crate) struct StartSlotGuard<'a> {
    registry: &'a Mutex<SessionRegistry>,
    employee_id: String,
}

impl<'a> StartSlotGuard<'a> {
    pub(crate) fn new(registry: &'a Mutex<SessionRegistry>, employee_id: String) -> Self {
        Self {
            registry,
            employee_id,
        }
    }
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
    employee_name: String,
    employee_handle: String,
    run_id: String,
    session_id: String,
    mark_str: String,
    runtime_id: String,
    supports_mid_turn: bool,
    bwrap_masked_siblings: Option<std::collections::BTreeSet<String>>,
    connection_id: Option<String>,
    route_digest: String,
    oauth_expires_at_ms: Option<u64>,
    oauth_token_fp: Option<String>,
    oauth_refresh_attempted: bool,
    workspace_arg: Option<String>,
    permit: Option<(String, u64)>,
    task_ref: Option<String>,
    tried_connections: Vec<String>,
    home: PathBuf,
    workspace_real: PathBuf,
    runtime_dir: PathBuf,
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
    supports_mid_turn: bool,
    bwrap_masked_siblings: Option<std::collections::BTreeSet<String>>,
    connection_id: Option<String>,
    route_digest: String,
    oauth_expires_at_ms: Option<u64>,
    oauth_token_fp: Option<String>,
    oauth_refresh_attempted: bool,
    workspace_arg: Option<String>,
    permit: Option<(String, u64)>,
    task_ref: Option<String>,
    tried_connections: Vec<String>,
    home: PathBuf,
    tmpdir: PathBuf,
    binding_file: PathBuf,
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
    binding_file: &'a Path,
    armed: bool,
}

impl<'a> BindingGuard<'a> {
    fn new(
        bindings: &'a RunBindingRegistry,
        employee_id: &'a str,
        run_id: &'a str,
        binding_file: &'a Path,
    ) -> Self {
        Self {
            bindings,
            employee_id,
            run_id,
            binding_file,
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
            let _ = std::fs::remove_file(self.binding_file);
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
    pub(crate) waiting_messages: Arc<Mutex<HashMap<String, Arc<AtomicU32>>>>,
    pub(crate) mid_turn_rates: Arc<Mutex<HashMap<String, MidTurnRateTracker>>>,
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
            waiting_messages: Arc::new(Mutex::new(HashMap::new())),
            mid_turn_rates: Arc::new(Mutex::new(HashMap::new())),
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

    /// Returns the lock-free per-employee `AtomicU32` counter tracking pending wakes/messages
    /// (`TASK-115` AC #6). Initialized once from disk on first lookup and maintained in memory on
    /// every wake queue mutation so bound `agent-os` RPCs read it with zero disk I/O.
    pub fn waiting_messages_counter(&self, employee_id: &str) -> Arc<AtomicU32> {
        let mut map = self
            .waiting_messages
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = map.get(employee_id) {
            return Arc::clone(existing);
        }
        let initial = self
            .store
            .read_pending_wakes(employee_id)
            .ok()
            .flatten()
            .map_or(0, |q| q.items.len().min(u32::MAX as usize) as u32);
        let counter = Arc::new(AtomicU32::new(initial));
        map.insert(employee_id.to_owned(), Arc::clone(&counter));
        counter
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
    /// Refuses evicting handles already in `RecoveryRequired` and fails closed on unverified teardown.
    pub fn stop_session(&self, employee_id: &str, reason: &str) -> Result<SessionView, ApiError> {
        stop_session_impl(self, employee_id, reason)
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
        self.start_session_inner(actor_member_id, false, params)
    }

    fn start_session_inner(
        &self,
        actor_member_id: &str,
        hq_attested_owner: bool,
        params: &MemberSessionStartParams,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_or_person_inner(actor_member_id, hq_attested_owner)?;
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
        if let Some(view) = live_view {
            return Ok(view);
        }
        let _guard = StartSlotGuard::new(&self.registry, emp_id.to_owned());
        reap_replaced_handle(self, emp_id, finished_handle)?;
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
        if let Some(view) = live_view {
            let tokens = self.bindings.active_tokens();
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            let _ = self.store.append_history(emp_id, &trigger_entry, &refs);
            return Ok(view);
        }
        let _guard = StartSlotGuard::new(&self.registry, emp_id.to_owned());
        reap_replaced_handle(self, emp_id, finished_handle)?;
        self.check_not_blocked(emp_id)?;
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
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
        self.inspect_session_inner(caller_id, false, employee_id)
    }

    fn inspect_session_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        employee_id: Option<&str>,
    ) -> Result<SessionInspectResult, ApiError> {
        let target = employee_id.unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
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
        self.list_sessions_inner(caller_id, false, params)
    }

    fn list_sessions_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        params: &MemberSessionListParams,
    ) -> Result<SessionListResult, ApiError> {
        let target = params.employee_id.as_deref().unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
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
        self.read_session_transcript_inner(caller_id, false, params)
    }

    fn read_session_transcript_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        params: &MemberSessionTranscriptParams,
    ) -> Result<SessionTranscriptResult, ApiError> {
        let target = params.employee_id.as_deref().unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
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
        let Ok(exec_opt) = self.office.load_agent_execution(employee_id) else {
            return secrets;
        };
        let Some(exec) = exec_opt else {
            // Unconfigured agents fall back to Claude host subscription auth (`router.rs`),
            // so redact host Claude OAuth if present.
            if let Some(tok) = crate::host_auth::resolve_host_claude_oauth_token() {
                secrets.push(tok);
            }
            return secrets;
        };
        let Ok(Some(prof)) = self
            .office
            .load_model_profile(exec.model_profile_id.as_str())
        else {
            return secrets;
        };
        let mut uses_claude_host_subscription = false;
        let conn_ids = prof
            .generated_for_connection_id
            .iter()
            .chain(prof.connection_ids.iter());
        for conn_id in conn_ids {
            let mut has_vault_secret = false;
            if let Ok(Some(raw)) = self.enrollment.vault().load_unwrapped(conn_id.as_str()) {
                if let Ok(text) = std::str::from_utf8(&raw) {
                    has_vault_secret = true;
                    secrets.push(zeroize::Zeroizing::new(text.to_owned()));
                }
            }
            if !has_vault_secret && self.is_claude_subscription_connection(conn_id.as_str()) {
                uses_claude_host_subscription = true;
            }
        }
        if uses_claude_host_subscription {
            if let Some(tok) = crate::host_auth::resolve_host_claude_oauth_token() {
                secrets.push(tok);
            }
        }
        secrets
    }

    fn is_claude_subscription_connection(&self, connection_id: &str) -> bool {
        let Ok(Some(conn)) = self.office.load_provider_connection(connection_id) else {
            return false;
        };
        conn.auth_mode
            == org_office::organization_family::records::ProviderAuthMode::SubscriptionCli
            && matches!(
                conn.runtime_driver.trim().to_ascii_lowercase().as_str(),
                "anthropic" | "claude" | "claude-code"
            )
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
        self.list_grants_inner(caller_id, false, employee_id)
    }

    fn list_grants_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        employee_id: Option<&str>,
    ) -> Result<SessionGrantsResult, ApiError> {
        let target = employee_id.unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
        }
        let employee = self.require_agent(target)?;
        let home = self.office.agent_private_home_path(employee.id.as_str());
        let workspace = canonical_or_spec(&home)?;
        let (preset, sandbox_enabled) = self.load_preset_and_sandbox(employee.id.as_str())?;
        let cfg = self.build_employee_preset_cfg(
            employee.id.as_str(),
            preset,
            sandbox_enabled,
            &workspace,
            false,
        )?;
        Ok(build_grants_summary(employee.id.as_str(), &cfg, &[]))
    }

    fn build_employee_preset_cfg(
        &self,
        employee_id: &str,
        preset: PermissionPreset,
        sandbox_enabled: bool,
        workspace: &SpecPath,
        create_root: bool,
    ) -> Result<PresetConfig, ApiError> {
        let socket_str = self.config.office_socket.display().to_string();
        let socket_spec = SpecPath::try_from(socket_str.as_str()).ok();
        let denied_globs = self.denied_office_globs_for(Some(employee_id));
        let mut cfg = build_session_policy(
            preset,
            sandbox_enabled,
            workspace,
            socket_spec.as_ref(),
            &denied_globs,
            1,
        )
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid permission preset"))?;
        let (wt_writable, wt_write_denies) = self.worktree_sandbox_grants(employee_id, create_root);
        apply_worktree_sandbox_rules(&mut cfg, &wt_writable, &wt_write_denies);
        Ok(cfg)
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
        self.decide_approval_inner(
            actor_member_id,
            false,
            employee_id,
            approval_id,
            decision_str,
            reason,
        )
    }

    fn decide_approval_inner(
        &self,
        actor_member_id: &str,
        hq_attested_owner: bool,
        employee_id: &str,
        approval_id: &str,
        decision_str: &str,
        reason: Option<String>,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_only_inner(actor_member_id, hq_attested_owner)?;
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
        self.clear_stall_lock_inner(actor_member_id, false, employee_id)
    }

    fn clear_stall_lock_inner(
        &self,
        actor_member_id: &str,
        hq_attested_owner: bool,
        employee_id: &str,
    ) -> Result<SessionView, ApiError> {
        clear_stall_lock_impl(self, actor_member_id, hq_attested_owner, employee_id)
    }

    /// Dispatches all C3 session & provider-enrollment RPCs with caller-scoped `command_id` replay.
    pub fn dispatch_rpc(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        self.dispatch_rpc_inner(member, false, true, request)
    }

    /// Dispatches an HQ-authenticated, mTLS-proxied session-control RPC on a satellite office
    /// (`TASK-143` AC #3). Never dispatches provider-enrollment RPCs.
    pub fn dispatch_hq_proxied_rpc(
        &self,
        caller_id: &str,
        caller_is_owner: bool,
        request: &Request,
    ) -> Option<Response> {
        self.dispatch_rpc_inner(Some(caller_id), caller_is_owner, false, request)
    }

    fn dispatch_rpc_inner(
        &self,
        member: Option<&str>,
        hq_attested_owner: bool,
        allow_enrollment: bool,
        request: &Request,
    ) -> Option<Response> {
        if !is_c3_rpc_method(&request.method) {
            return None;
        }
        let Some(caller_id) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotAuthorized, "authenticated caller required"),
            ));
        };
        if let Err(err) = self.authorize_rpc_caller(caller_id, hq_attested_owner, &request.method) {
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
            .dispatch_session_rpcs(Some(caller_id), hq_attested_owner, request)
            .or_else(|| {
                allow_enrollment
                    .then(|| self.dispatch_enrollment_rpcs(Some(caller_id), request))
                    .flatten()
            })?;
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

    fn authorize_rpc_caller(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        method: &str,
    ) -> Result<(), ApiError> {
        match method {
            "provider.enrollment_start"
            | "provider.enrollment_status"
            | "provider.enrollment_submit"
            | "provider.enrollment_cancel"
            | "provider.credential_status"
            | "provider.credential_revoke"
            | "member.session_approval"
            | "member.session_clear_stall" => {
                self.authorize_owner_only_inner(caller_id, hq_attested_owner)
            }
            "member.session_start" | "member.session_stop" => {
                self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)
            }
            _ => Ok(()),
        }
    }

    fn dispatch_session_rpcs(
        &self,
        member: Option<&str>,
        hq_attested_owner: bool,
        request: &Request,
    ) -> Option<Response> {
        if request.is::<MemberSessionStart>() {
            return Some(handle_call::<MemberSessionStart>(
                request,
                member,
                |m, p| self.start_session_inner(m, hq_attested_owner, &p),
            ));
        }
        if request.is::<MemberSessionStop>() {
            return Some(handle_call::<MemberSessionStop>(request, member, |m, p| {
                self.authorize_owner_or_person_inner(m, hq_attested_owner)?;
                self.stop_session(&p.employee_id, p.reason.as_deref().unwrap_or("stopped"))
            }));
        }
        if request.is::<MemberSessionInspect>() {
            return Some(handle_call::<MemberSessionInspect>(
                request,
                member,
                |m, p| self.inspect_session_inner(m, hq_attested_owner, p.employee_id.as_deref()),
            ));
        }
        if request.is::<MemberSessionGrants>() {
            return Some(handle_call::<MemberSessionGrants>(
                request,
                member,
                |m, p| self.list_grants_inner(m, hq_attested_owner, p.employee_id.as_deref()),
            ));
        }
        if request.is::<MemberSessionApprovalDecide>() {
            return Some(handle_call::<MemberSessionApprovalDecide>(
                request,
                member,
                |m, p| {
                    self.decide_approval_inner(
                        m,
                        hq_attested_owner,
                        &p.employee_id,
                        &p.approval_id,
                        &p.decision,
                        p.reason,
                    )
                },
            ));
        }
        if request.is::<MemberSessionClearStall>() {
            return Some(handle_call::<MemberSessionClearStall>(
                request,
                member,
                |m, p| self.clear_stall_lock_inner(m, hq_attested_owner, &p.employee_id),
            ));
        }
        if request.is::<MemberSessionList>() {
            return Some(handle_call::<MemberSessionList>(request, member, |m, p| {
                self.list_sessions_inner(m, hq_attested_owner, &p)
            }));
        }
        if request.is::<MemberSessionTranscript>() {
            return Some(handle_call::<MemberSessionTranscript>(
                request,
                member,
                |m, p| self.read_session_transcript_inner(m, hq_attested_owner, &p),
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
        self.authorize_owner_only_inner(member_id, false)
    }

    fn authorize_owner_only_inner(
        &self,
        member_id: &str,
        hq_attested_owner: bool,
    ) -> Result<(), ApiError> {
        if hq_attested_owner || member_id == self.office.owner_member_id() {
            return Ok(());
        }
        Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner may perform this operation",
        ))
    }

    fn authorize_owner_or_person_inner(
        &self,
        member_id: &str,
        hq_attested_owner: bool,
    ) -> Result<(), ApiError> {
        if hq_attested_owner || member_id == self.office.owner_member_id() {
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
            ensure_live_handle_not_recovery_required(&state)?;
            let bwrap_topology_stale = is_bwrap_topology_stale(&self.office, emp_id, handle);
            let stop_reason = warm_handle_stop_reason(
                &self.office,
                emp_id,
                handle,
                requested_workspace,
                state.state,
                bwrap_topology_stale,
            );
            if stop_reason.is_none() && is_warm_state_reusable(state.state) {
                if let Some(text) = objective {
                    ensure_warm_bwrap_not_stale(bwrap_topology_stale)?;
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
            if let Some(reason) = stop_reason {
                let _ = handle.tx.send(PumpCommand::Stop {
                    reason: reason.into(),
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
            oauth_refresh_attempted: false,
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
        // Before creating this agent's runtime directory or session-binding token on disk,
        // ensure no already-running Linux `bwrap` sibling lacks a mount mask for `emp_id`
        // (`TASK-166`).
        crate::bwrap_isolation::enforce_bwrap_sibling_isolation(self, &emp_id)?;
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

        let is_linux_claude_bwrap =
            is_linux_claude_bwrap(admitted.sandbox_enabled, admitted.adapter.id().as_str());
        let bwrap_masked_siblings = if is_linux_claude_bwrap {
            Some(crate::bwrap_isolation::ensure_bwrap_deny_dirs(
                self, &emp_id,
            )?)
        } else {
            None
        };

        let preset_cfg = self.build_employee_preset_cfg(
            &emp_id,
            admitted.preset,
            admitted.sandbox_enabled,
            &workspace_spec,
            true,
        )?;
        let (runtime_dir, tmpdir) = self.prepare_run_dirs(&emp_id)?;
        let subagent_def =
            build_general_subagent_definition(preset_cfg.allowed_tools.as_deref(), None);
        let _ = write_subagent_definition(&runtime_dir, &subagent_def);

        let instructions =
            self.build_instructions(employee, &home, &workspace_real, &preset_cfg, opts.task_ref)?;
        let read_only_dirs = self.collect_session_read_only_dirs(
            admitted.adapter.id(),
            &home,
            &workspace_real,
            preset_cfg.sandbox.enabled,
        );
        let spec = build_session_spec_for_launch(
            admitted.adapter.id(),
            employee,
            admitted.model_token,
            workspace_spec,
            read_only_dirs,
            instructions,
            &preset_cfg,
            is_linux_claude_bwrap,
        )?;
        let spec_hash = compute_launch_spec_hash(&spec, &admitted.route_digest_suffix);
        let run_id = format!("run-{}", next_entropy_hex(10)?);
        let session_id = next_uuid_v4()?;
        let mark_str = format!("m-{}", next_entropy_hex(12)?);
        let allow_host_auth =
            crate::host_auth::allows_host_subscription_auth(&admitted.auth_env, admitted.auth_mode);
        let adapter_id = admitted.adapter.id();
        let runtime_name = adapter_id.as_str();
        let ctx = build_launch_context(
            binary,
            &runtime_dir,
            &tmpdir,
            &session_id,
            opts.resume_runtime_session_id,
            runtime_name,
            allow_host_auth,
        )?;
        let (plan, session) = admitted
            .adapter
            .plan_session(&spec, &ctx)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "runtime rejected session spec"))?;

        let (launch_env, oauth_expires_at_ms, oauth_token_fp) = assemble_launch_env(
            admitted.auth_env,
            admitted.endpoint_env,
            runtime_name,
            allow_host_auth,
            &ctx.runtime_path,
            &plan,
        );

        Ok(PlannedSessionLaunch {
            emp_id,
            employee_name: employee.name.clone(),
            employee_handle: employee.handle.clone(),
            run_id,
            session_id,
            mark_str,
            runtime_id: admitted.adapter.id().as_str().to_owned(),
            supports_mid_turn: admitted.adapter.capabilities().supports_mid_turn_delivery(),
            bwrap_masked_siblings,
            connection_id: admitted.connection_id,
            route_digest: admitted.route_digest_suffix,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted: opts.oauth_refresh_attempted,
            workspace_arg: params.workspace.clone(),
            permit: permit_guard.take(),
            task_ref: opts.task_ref.map(str::to_owned),
            tried_connections: opts.tried_connections,
            home,
            workspace_real,
            runtime_dir,
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
            employee_name,
            employee_handle,
            run_id,
            session_id,
            mark_str,
            runtime_id,
            supports_mid_turn,
            bwrap_masked_siblings,
            connection_id,
            route_digest,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted,
            workspace_arg,
            permit,
            task_ref,
            tried_connections,
            home,
            workspace_real,
            runtime_dir,
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
        let binding_file = runtime_dir.join(format!("session-binding-{run_id}.v1"));
        let mut binding_guard = BindingGuard::new(&self.bindings, &emp_id, &run_id, &binding_file);
        write_owner_only_binding_file(&binding_file, binding.expose_token())?;
        let env = self.build_environment(SessionEnvInput {
            home: &home,
            workspace: &workspace_real,
            tmpdir: &tmpdir,
            employee_id: &emp_id,
            employee_name: &employee_name,
            employee_handle: &employee_handle,
            session_id: &session_id,
            token: binding.expose_token(),
            binding_file: &binding_file,
            sandbox_enabled: preset_cfg.sandbox.enabled,
        })?;
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

        crate::bwrap_isolation::verify_bwrap_masked_before_spawn(
            self,
            &emp_id,
            bwrap_masked_siblings.as_ref(),
        )?;
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
            supports_mid_turn,
            bwrap_masked_siblings,
            connection_id,
            route_digest,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted,
            workspace_arg,
            permit: permit_guard.take(),
            task_ref,
            tried_connections,
            home,
            tmpdir,
            binding_file,
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
            runtime_id,
            supports_mid_turn,
            bwrap_masked_siblings,
            connection_id,
            route_digest,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted,
            workspace_arg,
            permit,
            task_ref,
            tried_connections,
            binding_file,
            objective,
            spawned,
            session,
            lifecycle,
            ..
        } = prepared;
        let active_permit = Arc::new(Mutex::new(permit));
        let (tx, rx) = mpsc::channel();
        let hooks = self.build_pump_hooks(
            &emp_id,
            route_digest.clone(),
            oauth_expires_at_ms,
            bwrap_masked_siblings.clone(),
        );
        let pump_ctx = PumpContext {
            employee_id: emp_id.clone(),
            run_id,
            runtime_id,
            supports_mid_turn,
            connection_id: connection_id.clone(),
            active_permit: Arc::clone(&active_permit),
            workspace: workspace_arg.clone(),
            task_ref,
            tried_connections,
            oauth_refresh_attempted,
            oauth_expires_at_ms,
            oauth_token_fp,
            store: self.store.clone(),
            bindings: self.bindings.clone(),
            state: Arc::clone(&runtime_state),
            stop_plan: lifecycle.stop,
            cleanup_paths: lifecycle.cleanup_after_exit,
            binding_file,
            hooks,
        };
        let pre_pump_meta = PrePumpHandleMeta {
            tx: tx.clone(),
            state: Arc::clone(&runtime_state),
            connection_id: connection_id.clone(),
            route_digest: route_digest.clone(),
            oauth_expires_at_ms,
            workspace: workspace_arg.clone(),
            active_permit: Arc::clone(&active_permit),
            supports_mid_turn,
            bwrap_masked_siblings: bwrap_masked_siblings.clone(),
        };
        let ready =
            ensure_pre_pump_isolation(self, &emp_id, spawned, session, pump_ctx, pre_pump_meta)?;
        // Enqueue the initial turn BEFORE starting the pump thread so `run_pump` always sees
        // Turn 1 immediately upon `Ready` without transitioning `Starting -> Idle` prematurely.
        if !objective.is_empty() {
            let _ = tx.send(PumpCommand::SendTurn(objective));
        }
        let thread = spawn_pump_thread(
            self,
            &emp_id,
            ready.spawned,
            ready.session,
            rx,
            ready.pump_ctx,
            ready.meta,
        )?;
        let view = runtime_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_view();
        register_live_session_or_abort(
            self,
            emp_id,
            LiveSessionHandle {
                state: runtime_state,
                tx,
                thread: Some(thread),
                connection_id,
                route_digest,
                oauth_expires_at_ms,
                workspace: workspace_arg,
                active_permit,
                supports_mid_turn,
                bwrap_masked_siblings,
            },
        )?;
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
        let office_id = &self.office.info().office_id;
        let base = self
            .office
            .durable_root()
            .join("local")
            .join("offices")
            .join(office_id)
            .join("agents")
            .join(employee_id);
        let runtime_dir = base.join("runtime");
        let tmpdir = crate::host_auth::resolve_agent_tmpdir(
            &self.config.office_socket,
            &base.join("tmp"),
            office_id,
            employee_id,
        );
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

    fn build_environment(&self, input: SessionEnvInput<'_>) -> Result<Environment, ApiError> {
        let helper_dir = RuntimeRegistry::resolve_helper_dir(self.config.helper_dir.as_deref());
        let captured_path =
            std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
        let forbidden = vec![input.home.to_path_buf(), input.workspace.to_path_buf()];
        let search_path = SearchPath::new(&helper_dir, &captured_path, &forbidden)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid helper search path"))?;
        let mut captured = vec![
            (
                OsString::from("HOME"),
                input.home.as_os_str().to_os_string(),
            ),
            (
                OsString::from("TMPDIR"),
                input.tmpdir.as_os_str().to_os_string(),
            ),
            (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
            (OsString::from("TERM"), OsString::from("dumb")),
        ];
        // Preserve host read-only Rust caches (`registry`, `git`, `.rustup`) inside the agent's
        // isolated home via safe symlinks (never linking host `credentials*` or `config*`) so
        // `rustc`/`cargo` work offline with a writable `<agent_home>/.cargo/.package-cache` even
        // when runtimes scrub `CARGO_HOME` (`TASK-126`, `TASK-159`). Host `~/.cargo/credentials*`
        // remain explicitly denied in policy. When sandboxed, set `CARGO_NET_OFFLINE=true` so
        // Cargo never hits closed egress (`TASK-143`).
        let rust_forbidden = vec![
            input.home.to_path_buf(),
            input.workspace.to_path_buf(),
            self.office.durable_root().to_path_buf(),
            self.office.runtime_root().to_path_buf(),
        ];
        append_rust_toolchain_env(
            &mut captured,
            Some(input.home),
            std::env::var_os("RUSTUP_HOME"),
            std::env::var_os("CARGO_HOME"),
            std::env::var_os("HOME"),
            &rust_forbidden,
            input.sandbox_enabled,
        );
        let info = self.office.info();
        let (git_name, git_email) = agent_git_identity(input.employee_name, input.employee_handle);
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
                OsString::from(input.employee_id),
            ),
            (
                OsString::from("AGENT_OS_SESSION_ID"),
                OsString::from(input.session_id),
            ),
            (
                OsString::from("AGENT_OS_SESSION_TOKEN"),
                OsString::from(input.token),
            ),
            (
                OsString::from("AGENT_OS_BINDING_FILE"),
                input.binding_file.as_os_str().to_os_string(),
            ),
            (OsString::from("GIT_AUTHOR_NAME"), OsString::from(&git_name)),
            (
                OsString::from("GIT_AUTHOR_EMAIL"),
                OsString::from(&git_email),
            ),
            (
                OsString::from("GIT_COMMITTER_NAME"),
                OsString::from(&git_name),
            ),
            (
                OsString::from("GIT_COMMITTER_EMAIL"),
                OsString::from(&git_email),
            ),
        ];
        Environment::new(search_path, &captured)
            .and_then(|env| env.with_context(&context))
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not build session env"))
    }

    fn collect_session_read_only_dirs(
        &self,
        runtime: RuntimeId,
        home: &Path,
        workspace: &Path,
        sandbox_enabled: bool,
    ) -> Vec<SpecPath> {
        if !sandbox_enabled || runtime.as_str() != "agy" {
            return Vec::new();
        }
        let helper_dir = RuntimeRegistry::resolve_helper_dir(self.config.helper_dir.as_deref());
        let forbidden = vec![
            home.to_path_buf(),
            workspace.to_path_buf(),
            self.office.folder().to_path_buf(),
            self.office.durable_root().to_path_buf(),
            self.office.runtime_root().to_path_buf(),
        ];
        collect_agy_read_only_dirs(
            &helper_dir,
            std::env::var_os("RUSTUP_HOME"),
            std::env::var_os("CARGO_HOME"),
            std::env::var_os("HOME"),
            &forbidden,
        )
    }

    #[cfg(test)]
    pub(crate) fn denied_office_globs(&self) -> Vec<PathPattern> {
        self.denied_office_globs_for(None)
    }

    pub(crate) fn denied_office_globs_for(
        &self,
        self_employee_id: Option<&str>,
    ) -> Vec<PathPattern> {
        let mut out = Vec::new();
        push_glob_pattern(&mut out, self.office.folder());
        push_glob_pattern(&mut out, &self.office.folder().join("git-credentials"));
        let durable = self.office.durable_root();
        let office_id = &self.office.info().office_id;
        let office_local = durable.join("local").join("offices").join(office_id);
        for sub in [
            "state",
            "journal",
            "authority",
            "private",
            "messaging",
            "budgets",
            "accounting",
            "recovery",
        ] {
            push_glob_pattern(&mut out, &durable.join(sub));
        }
        for sub in [
            "auth",
            "connections",
            "sessions",
            "enroll-tmp",
            "security",
            "subscription-usage",
        ] {
            push_glob_pattern(&mut out, &office_local.join(sub));
        }
        self.append_runtime_office_denies(
            &mut out,
            self.office.runtime_root(),
            office_id,
            self_employee_id,
        );
        self.append_sibling_and_socket_denies(&mut out, &office_local, office_id, self_employee_id);
        append_cargo_credential_denies(&mut out);
        append_agy_credential_denies(&mut out);
        out
    }

    fn append_runtime_office_denies(
        &self,
        out: &mut Vec<PathPattern>,
        runtime_root: &Path,
        office_id: &str,
        self_employee_id: Option<&str>,
    ) {
        push_glob_pattern(out, &runtime_root.join("snapshot-import"));
        push_glob_pattern(out, &runtime_root.join("snapshot-export"));
        if let Ok(office_ids) = self.office.enrolled_office_ids() {
            for other_office_id in office_ids {
                if other_office_id != office_id {
                    push_glob_pattern(out, &runtime_root.join(&other_office_id));
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(runtime_root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str != office_id
                    && name_str != "snapshot-import"
                    && name_str != "snapshot-export"
                {
                    let p = entry.path();
                    push_exact_pattern(out, &p);
                    push_glob_pattern(out, &p);
                }
            }
        }
        let office_rt = runtime_root.join(office_id);
        for file in [
            "office.db",
            "office.db-wal",
            "office.db-shm",
            "office.db-journal",
            "office.db.new",
            "office.db.new-wal",
            "office.db.new-shm",
            "office.db.new-journal",
            "git-state.json",
        ] {
            push_exact_pattern(out, &office_rt.join(file));
        }
        for dir in ["protected", "git-home"] {
            push_glob_pattern(out, &office_rt.join(dir));
        }
        match self_employee_id {
            Some(emp_id) => {
                self.append_runtime_team_sibling_denies(out, &office_rt, emp_id);
                self.append_runtime_git_denies(out, runtime_root, &office_rt, emp_id);
            }
            None => {
                push_glob_pattern(out, &office_rt.join("teams"));
                push_glob_pattern(out, &office_rt.join("repositories"));
            }
        }
    }

    fn append_runtime_git_denies(
        &self,
        out: &mut Vec<PathPattern>,
        runtime_root: &Path,
        office_rt: &Path,
        emp_id: &str,
    ) {
        let repos_root = office_rt.join("repositories");
        let Ok(git_state) =
            org_office::git::OfficeGitState::load(runtime_root, &self.office.info().office_id)
        else {
            // Fail closed on corrupt/unreadable git-state.json: deny all bare caches.
            push_glob_pattern(out, &repos_root);
            return;
        };
        let self_admins_by_repo = collect_self_active_admins(&git_state, emp_id);
        if self_admins_by_repo.is_empty() {
            // Zero active worktrees: blanket-deny `<office_rt>/repositories/**` so repositories
            // cloned after session launch can never be read by this session (`doc-35`).
            push_glob_pattern(out, &repos_root);
            return;
        }
        let Ok((cache_dirs, extra_entries)) =
            self.collect_bare_cache_entries(&repos_root, Some(&git_state))
        else {
            push_glob_pattern(out, &repos_root);
            return;
        };
        for extra in extra_entries {
            push_exact_pattern(out, &extra);
            push_glob_pattern(out, &extra);
        }
        for cache_dir in cache_dirs {
            let repo_id = cache_dir
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".git"));
            let Some(self_admins) = repo_id.and_then(|rid| self_admins_by_repo.get(rid)) else {
                // Untargeted repository cache: deny all read and write access (`doc-35`).
                push_glob_pattern(out, &cache_dir);
                continue;
            };
            let wt_dir = cache_dir.join("worktrees");
            for other in collect_non_self_admin_names(&wt_dir, repo_id, &git_state, self_admins) {
                let p = wt_dir.join(other);
                push_exact_pattern(out, &p);
                push_glob_pattern(out, &p);
            }
        }
    }

    fn append_runtime_team_sibling_denies(
        &self,
        out: &mut Vec<PathPattern>,
        office_rt: &Path,
        emp_id: &str,
    ) {
        let Some((chain, all_team_ids)) = self.resolve_employee_team_chain_and_ids(emp_id) else {
            push_glob_pattern(out, &office_rt.join("teams"));
            return;
        };
        let mut cursor = office_rt.to_path_buf();
        for (idx, team_id) in chain.iter().enumerate() {
            let teams_dir = cursor.join("teams");
            for other_team_id in &all_team_ids {
                if other_team_id != team_id {
                    push_glob_pattern(out, &teams_dir.join(other_team_id));
                }
            }
            if let Ok(entries) = std::fs::read_dir(&teams_dir) {
                for entry in entries.flatten() {
                    if entry.file_name().to_str() != Some(team_id.as_str()) {
                        let p = entry.path();
                        push_exact_pattern(out, &p);
                        push_glob_pattern(out, &p);
                    }
                }
            }
            cursor = teams_dir.join(team_id);
            // Ancestor teams' `agents/` directories belong to other teams' agents; deny them.
            if idx + 1 < chain.len() {
                push_glob_pattern(out, &cursor.join("agents"));
            }
        }
        // Deny any existing or future child subteams nested under the employee's own leaf team.
        push_glob_pattern(out, &cursor.join("teams"));
        let agents_dir = cursor.join("agents");
        for other in self.collect_sibling_agent_ids(&agents_dir, Some(emp_id)) {
            push_glob_pattern(out, &agents_dir.join(other));
        }
    }

    pub(crate) fn resolve_employee_team_chain_and_ids(
        &self,
        employee_id: &str,
    ) -> Option<(Vec<String>, Vec<String>)> {
        let emp = self.office.load_employee(employee_id).ok()??;
        let snapshot = self.office.org_snapshot().ok()?;
        let parent_by_id: HashMap<&str, Option<&str>> = snapshot
            .teams
            .iter()
            .map(|t| (t.id.as_str(), t.owner_team_id.as_deref()))
            .collect();
        if !parent_by_id.contains_key(emp.home_team_id.as_str()) {
            return None;
        }
        let mut chain = Vec::new();
        let mut cur = Some(emp.home_team_id.as_str());
        let mut visited = HashSet::new();
        while let Some(tid) = cur {
            if !visited.insert(tid) {
                // Fail closed on team hierarchy cycle.
                return None;
            }
            let &parent = parent_by_id.get(tid)?;
            chain.push(tid.to_owned());
            cur = parent;
        }
        chain.reverse();
        let all_team_ids = snapshot.teams.into_iter().map(|t| t.id).collect();
        Some((chain, all_team_ids))
    }

    pub(crate) fn resolve_employee_team_chain(&self, employee_id: &str) -> Vec<String> {
        self.resolve_employee_team_chain_and_ids(employee_id)
            .map(|(chain, _)| chain)
            .unwrap_or_default()
    }

    pub(crate) fn worktree_sandbox_grants(
        &self,
        employee_id: &str,
        create_root: bool,
    ) -> (Vec<SpecPath>, Vec<PathPattern>) {
        let office_rt = self
            .office
            .runtime_root()
            .join(&self.office.info().office_id);
        let mut writable = Vec::new();
        let mut write_denies = Vec::new();
        let git_state = org_office::git::OfficeGitState::load(
            self.office.runtime_root(),
            &self.office.info().office_id,
        )
        .ok();
        let chain = self.resolve_employee_team_chain(employee_id);
        if !chain.is_empty() {
            let mut my_root = office_rt.clone();
            for tid in &chain {
                my_root = my_root.join("teams").join(tid);
            }
            my_root = my_root.join("agents").join(employee_id);
            if create_root {
                let _ = ensure_owner_only_dir(&my_root);
            }
            let canon_root = canonicalize_with_tail(&my_root);
            if let Ok(spec) = SpecPath::try_from(canon_root.to_string_lossy().as_ref()) {
                writable.push(spec);
            }
            // Emit concrete per-worktree `.git` / `.git/hooks/**` denies (no interior `**`
            // wildcards, which would cause Codex `path_pattern_base` to downgrade `canon_root`
            // to read-only).
            let mut wt_dirs = std::collections::BTreeSet::new();
            if let Some(gs) = &git_state {
                for ws in gs.workspaces.values() {
                    let p = Path::new(&ws.worktree_path);
                    if ws.employee_id == employee_id && ws.state == "active" && p.is_absolute() {
                        wt_dirs.insert(canonicalize_with_tail(p));
                    }
                }
            }
            collect_existing_worktree_dirs(&canon_root, 0, &mut wt_dirs);
            for wt_dir in wt_dirs {
                let dot_git = wt_dir.join(".git");
                push_exact_pattern(&mut write_denies, &dot_git);
                // Worktrees have a regular `.git` file (not a directory); only deny `.git/hooks`
                // when `.git` is a directory so Linux `bwrap` never tries to mask under a file.
                if dot_git.is_dir() {
                    push_exact_pattern(&mut write_denies, &dot_git.join("hooks"));
                    push_glob_pattern(&mut write_denies, &dot_git.join("hooks"));
                }
            }
        }
        // Scope bare-cache writable grants strictly to repositories targeted by `employee_id`'s
        // concrete active worktrees (`doc-35` §Sandbox / `FC-103`). Never grant `<cache>/worktrees`
        // parent directory as writable.
        if let Some(gs) = &git_state {
            let repos_root = office_rt.join("repositories");
            let self_admins_by_repo = collect_self_active_admins(gs, employee_id);
            for (repo_id, self_admins) in &self_admins_by_repo {
                let cache_dir = repos_root.join(format!("{repo_id}.git"));
                append_bare_cache_sandbox_rules(
                    &cache_dir,
                    self_admins,
                    &mut writable,
                    &mut write_denies,
                );
            }
        }
        (writable, write_denies)
    }

    fn collect_bare_cache_entries(
        &self,
        repos_root: &Path,
        git_state: Option<&org_office::git::OfficeGitState>,
    ) -> Result<(Vec<PathBuf>, Vec<PathBuf>), ()> {
        let repos = self
            .office
            .list_repositories(None)
            .map_err(|_| ())?
            .map_err(|_| ())?;
        let mut dirs = std::collections::BTreeSet::new();
        let mut extra = std::collections::BTreeSet::new();
        for r in repos {
            dirs.insert(repos_root.join(format!("{}.git", r.record.id.as_str())));
        }
        if let Some(gs) = git_state {
            for rid in gs.caches.keys() {
                dirs.insert(repos_root.join(format!("{rid}.git")));
            }
            for ws in gs.workspaces.values() {
                dirs.insert(repos_root.join(format!("{}.git", ws.repository_id)));
            }
        }
        if let Ok(entries) = std::fs::read_dir(repos_root) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().ends_with(".git") {
                    dirs.insert(entry.path());
                } else {
                    extra.insert(entry.path());
                }
            }
        }
        Ok((dirs.into_iter().collect(), extra.into_iter().collect()))
    }

    fn append_sibling_and_socket_denies(
        &self,
        out: &mut Vec<PathPattern>,
        office_local: &Path,
        office_id: &str,
        self_employee_id: Option<&str>,
    ) {
        let agents_root = office_local.join("agents");
        let sibling_ids = self.collect_sibling_agent_ids(&agents_root, self_employee_id);
        for other in &sibling_ids {
            push_glob_pattern(out, &agents_root.join(other));
        }
        if let Some(sock_parent) = self.config.office_socket.parent() {
            push_exact_pattern(out, &sock_parent.join("host.sock"));
            if sock_parent.file_name().and_then(|s| s.to_str()) == Some("run") {
                if let Some(instance_root) = sock_parent.parent() {
                    push_exact_pattern(out, &instance_root.join("host.sock"));
                }
            }
            push_glob_pattern(out, &sock_parent.join("r"));
            let short_root = sock_parent.join("t");
            let own_slug =
                self_employee_id.map(|emp| crate::host_auth::agent_tmpdir_slug(office_id, emp));
            for other in &sibling_ids {
                let slug = crate::host_auth::agent_tmpdir_slug(office_id, other);
                if own_slug.as_deref() != Some(slug.as_str()) {
                    push_glob_pattern(out, &short_root.join(slug));
                }
            }
            if let Ok(entries) = std::fs::read_dir(&short_root) {
                for entry in entries.flatten() {
                    if own_slug.as_deref() != entry.file_name().to_str() {
                        push_glob_pattern(out, &entry.path());
                    }
                }
            }
        }
        append_privileged_socket_patterns(out);
    }

    pub(crate) fn collect_sibling_agent_ids(
        &self,
        agents_root: &Path,
        self_employee_id: Option<&str>,
    ) -> std::collections::BTreeSet<String> {
        let mut ids = std::collections::BTreeSet::new();
        if let Ok(entries) = std::fs::read_dir(agents_root) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    ids.insert(name.to_owned());
                }
            }
        }
        if let Ok(employees) = self.office.list_employees() {
            for emp in employees {
                ids.insert(emp.id.as_str().to_owned());
            }
        }
        if let Some(me) = self_employee_id {
            ids.remove(me);
        }
        ids
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
    let _ = spawn_bg_worker_checked(mgr, name, f);
}

pub(crate) fn spawn_bg_worker_checked(
    mgr: &Arc<SessionManager>,
    name: &str,
    f: impl FnOnce(Arc<SessionManager>) + Send + 'static,
) -> bool {
    {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.draining || reg.shutting_down {
            return false;
        }
        reg.bg_workers.retain(|h| !h.is_finished());
        reg.bg_spawning += 1;
    }
    spawn_bg_worker_reserved(mgr, name, f)
}

/// Spawns a background worker consuming a `bg_spawning` slot that the caller already reserved
/// atomically under `mgr.registry` lock (no unlocked gap against `stop_all_inner`).
pub(crate) fn spawn_bg_worker_reserved(
    mgr: &Arc<SessionManager>,
    name: &str,
    f: impl FnOnce(Arc<SessionManager>) + Send + 'static,
) -> bool {
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
        return true;
    }
    false
}

fn take_clearable_live_handle(
    mgr: &SessionManager,
    employee_id: &str,
) -> Result<Option<LiveSessionHandle>, ApiError> {
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if reg.starting.contains(employee_id) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session start or teardown is already in progress",
        ));
    }
    let should_remove = reg.live.get(employee_id).is_some_and(|handle| {
        let mut guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        let removable = guard.recovery_required
            || matches!(
                guard.state,
                SessionState::StallLocked
                    | SessionState::RecoveryRequired
                    | SessionState::Stopped
                    | SessionState::Suspended
            );
        if !removable {
            guard.watchdog.clear_stall_lock(Instant::now());
        }
        removable
    });
    reg.starting.insert(employee_id.to_owned());
    let old = if should_remove {
        reg.live.remove(employee_id)
    } else {
        None
    };
    if let Some(ref handle) = old {
        prepare_handle_for_clear_reap(handle);
    }
    Ok(old)
}

fn prepare_handle_for_clear_reap(handle: &LiveSessionHandle) {
    if handle.thread.is_none() {
        return;
    }
    let mut guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
    guard.watchdog.clear_stall_lock(Instant::now());
    guard.recovery_required = false;
    if guard.state == SessionState::RecoveryRequired {
        guard.state = SessionState::Stopping;
    }
}

fn clear_persisted_recovery_state(mgr: &SessionManager, employee_id: &str) {
    let _ = mgr.store.clear_pending_start(employee_id);
    let _ = mgr.store.update_active_turn(employee_id, |persisted| {
        persisted.consecutive_stalls = 0;
        persisted.stall_locked = false;
        persisted.recovery_required = false;
        persisted.updated_at = rfc3339_now();
    });
    mgr.last_states
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(employee_id);
}

fn clear_stall_lock_impl(
    mgr: &SessionManager,
    actor_member_id: &str,
    hq_attested_owner: bool,
    employee_id: &str,
) -> Result<SessionView, ApiError> {
    mgr.authorize_owner_only_inner(actor_member_id, hq_attested_owner)?;
    let _ = mgr.require_agent(employee_id)?;
    let dead_handle = take_clearable_live_handle(mgr, employee_id)?;
    let slot_guard = StartSlotGuard::new(&mgr.registry, employee_id.to_owned());
    if let Some(old) = dead_handle {
        reap_cleared_handle(mgr, employee_id, old)?;
    }
    clear_persisted_recovery_state(mgr, employee_id);
    drop(slot_guard);
    let _ = mgr.process_due_wakes_for(employee_id);
    let live_view = {
        let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.live.get(employee_id).map(|handle| {
            handle
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .to_view()
        })
    };
    if let Some(mut view) = live_view {
        overlay_durable_wake_state(&mgr.store, employee_id, &mut view);
        return Ok(view);
    }
    mgr.idle_view_for(employee_id)
}

fn reap_cleared_handle(
    mgr: &SessionManager,
    employee_id: &str,
    old: LiveSessionHandle,
) -> Result<(), ApiError> {
    if old.thread.is_some() {
        crate::bwrap_isolation::reap_live_handle(
            mgr,
            employee_id,
            old,
            None,
            crate::bwrap_isolation::ReapMode::FailClosedRestore {
                conflict_message: format!(
                    "could not verify teardown of session `{employee_id}` during recovery clear; resolve recovery first"
                ),
            },
        )?;
        return Ok(());
    }
    mgr.bindings.revoke(employee_id, None);
    if let Some((pid, _)) = old
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        let _ = mgr.office.mark_turn_permit_release_pending(&pid);
    }
    Ok(())
}

fn stop_session_impl(
    mgr: &SessionManager,
    employee_id: &str,
    reason: &str,
) -> Result<SessionView, ApiError> {
    let removed = {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(handle) = reg.live.get(employee_id) {
            let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Err(err) = ensure_live_handle_not_recovery_required(&st) {
                drop(st);
                drop(reg);
                mgr.bindings.revoke(employee_id, None);
                return Err(err);
            }
            if reg.starting.contains(employee_id) {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session start or teardown is already in progress",
                ));
            }
            drop(st);
            let handle = reg.live.remove(employee_id);
            if handle.is_some() {
                reg.starting.insert(employee_id.to_owned());
            }
            handle
        } else {
            None
        }
    };
    let Some(handle) = removed else {
        mgr.bindings.revoke(employee_id, None);
        return mgr.idle_view_for(employee_id);
    };
    let slot_guard = StartSlotGuard::new(&mgr.registry, employee_id.to_owned());
    let reaped = crate::bwrap_isolation::reap_live_handle(
        mgr,
        employee_id,
        handle,
        Some(reason),
        crate::bwrap_isolation::ReapMode::FailClosedRestore {
            conflict_message: format!(
                "could not verify teardown of session `{employee_id}`; resolve recovery first"
            ),
        },
    )?;
    drop(slot_guard);
    if reaped.permit_released && !is_admissions_closed(mgr) {
        mgr.process_due_wakes();
    }
    Ok(reaped.view)
}

fn reap_replaced_handle(
    mgr: &SessionManager,
    emp_id: &str,
    old: Option<LiveSessionHandle>,
) -> Result<(), ApiError> {
    let Some(handle) = old else {
        return Ok(());
    };
    crate::bwrap_isolation::reap_live_handle(
        mgr,
        emp_id,
        handle,
        None,
        crate::bwrap_isolation::ReapMode::FailClosedRestore {
            conflict_message: format!(
                "could not verify teardown of prior session `{emp_id}`; resolve recovery first"
            ),
        },
    )?;
    Ok(())
}

fn stop_session_for_shutdown(mgr: &SessionManager, employee_id: &str, reason: &str) {
    let removed = {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.starting.contains(employee_id) {
            return;
        }
        let handle = reg.live.remove(employee_id);
        if handle.is_some() {
            reg.starting.insert(employee_id.to_owned());
        }
        handle
    };
    if let Some(handle) = removed {
        let _slot_guard = StartSlotGuard::new(&mgr.registry, employee_id.to_owned());
        let _ = crate::bwrap_isolation::reap_live_handle(
            mgr,
            employee_id,
            handle,
            Some(reason),
            crate::bwrap_isolation::ReapMode::ShutdownDrain,
        );
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
            stop_session_for_shutdown(mgr, &id, reason);
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

fn ensure_live_handle_not_recovery_required(state: &SessionRuntimeState) -> Result<(), ApiError> {
    if state.recovery_required || state.state == SessionState::RecoveryRequired {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session requires manual recovery before starting",
        ));
    }
    Ok(())
}

fn is_bwrap_topology_stale(office: &Office, emp_id: &str, handle: &LiveSessionHandle) -> bool {
    handle.bwrap_masked_siblings.as_ref().is_some_and(|masked| {
        crate::bwrap_isolation::has_unmasked_bwrap_sibling_in_office(office, emp_id, masked)
    })
}

fn ensure_warm_bwrap_not_stale(bwrap_topology_stale: bool) -> Result<(), ApiError> {
    if bwrap_topology_stale {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "sandboxed session topology is stale while busy; wait for current turn to finish or stop session first",
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
        writable_roots: effective_approval_writable_roots(prepared),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
    }
}

fn effective_approval_writable_roots(prepared: &PreparedSessionLaunch) -> Vec<SpecPath> {
    effective_approval_writable_roots_from(
        prepared.preset_cfg.preset,
        &prepared.preset_cfg.sandbox.writable_roots,
        &prepared.tmpdir,
    )
}

pub(crate) fn effective_approval_writable_roots_from(
    preset: PermissionPreset,
    sandbox_writable_roots: &[SpecPath],
    tmpdir: &Path,
) -> Vec<SpecPath> {
    let mut roots = sandbox_writable_roots.to_vec();
    if preset == PermissionPreset::ReadOnly {
        return roots;
    }
    let canon_tmp = canonicalize_with_tail(tmpdir);
    if let Some(spec) = canon_tmp
        .to_str()
        .and_then(|s| SpecPath::try_from(s).ok())
        .filter(|spec| !roots.contains(spec))
    {
        roots.push(spec);
    }
    roots
}

pub(crate) struct PrePumpHandleMeta {
    pub(crate) tx: Sender<PumpCommand>,
    pub(crate) state: Arc<Mutex<SessionRuntimeState>>,
    pub(crate) connection_id: Option<String>,
    pub(crate) route_digest: String,
    pub(crate) oauth_expires_at_ms: Option<u64>,
    pub(crate) workspace: Option<String>,
    pub(crate) active_permit: Arc<Mutex<Option<(String, u64)>>>,
    pub(crate) supports_mid_turn: bool,
    pub(crate) bwrap_masked_siblings: Option<std::collections::BTreeSet<String>>,
}

struct VerifiedPrePump {
    spawned: org_supervise::Spawned,
    session: Box<dyn org_agent_runtime::Session>,
    pump_ctx: PumpContext,
    meta: PrePumpHandleMeta,
}

fn ensure_pre_pump_isolation(
    mgr: &SessionManager,
    emp_id: &str,
    spawned: org_supervise::Spawned,
    mut session: Box<dyn org_agent_runtime::Session>,
    pump_ctx: PumpContext,
    meta: PrePumpHandleMeta,
) -> Result<VerifiedPrePump, ApiError> {
    let bwrap_pre_raced = crate::bwrap_isolation::verify_bwrap_masked_before_spawn(
        mgr,
        emp_id,
        meta.bwrap_masked_siblings.as_ref(),
    )
    .is_err();
    let shutting_down = is_admissions_closed(mgr);
    if !shutting_down && !bwrap_pre_raced {
        return Ok(VerifiedPrePump {
            spawned,
            session,
            pump_ctx,
            meta,
        });
    }
    let cleanup_verified = abort_pre_pump_shutdown(
        mgr,
        emp_id,
        spawned,
        &mut session,
        &pump_ctx,
        &meta.active_permit,
    );
    if !cleanup_verified && !shutting_down {
        install_unrecovered_pre_pump_handle(mgr, emp_id, meta);
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "could not verify teardown of aborted sandboxed session `{emp_id}`; resolve recovery first"
            ),
        ));
    }
    Err(if bwrap_pre_raced {
        ApiError::new(
            ErrorCode::Conflict,
            "a newly started sibling agent was not masked in this sandbox; retry launch",
        )
    } else {
        ApiError::new(
            ErrorCode::Unavailable,
            "office session manager is shutting down",
        )
    })
}

pub(crate) fn install_unrecovered_pre_pump_handle(
    mgr: &SessionManager,
    emp_id: &str,
    meta: PrePumpHandleMeta,
) {
    let (run_id, session_id, runtime_session_id, view) = {
        let mut st = meta.state.lock().unwrap_or_else(|p| p.into_inner());
        st.recovery_required = true;
        st.state = SessionState::RecoveryRequired;
        (
            st.run_id.clone(),
            st.session_id.clone(),
            st.runtime_session_id.clone(),
            st.to_view(),
        )
    };
    mgr.last_states
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(emp_id.to_owned(), view);
    let _ = mgr.store.persist_recovery_required(
        emp_id,
        run_id.as_deref(),
        session_id.as_deref(),
        runtime_session_id.as_deref(),
    );
    if !is_admissions_closed(mgr) {
        let handle = LiveSessionHandle {
            state: meta.state,
            tx: meta.tx,
            thread: None,
            connection_id: meta.connection_id,
            route_digest: meta.route_digest,
            oauth_expires_at_ms: meta.oauth_expires_at_ms,
            workspace: meta.workspace,
            active_permit: meta.active_permit,
            supports_mid_turn: meta.supports_mid_turn,
            bwrap_masked_siblings: meta.bwrap_masked_siblings,
        };
        mgr.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .live
            .insert(emp_id.to_owned(), handle);
    }
}

fn spawn_pump_thread(
    mgr: &SessionManager,
    emp_id: &str,
    spawned: org_supervise::Spawned,
    session: Box<dyn org_agent_runtime::Session>,
    rx: mpsc::Receiver<PumpCommand>,
    pump_ctx: PumpContext,
    meta: PrePumpHandleMeta,
) -> Result<JoinHandle<()>, ApiError> {
    let thread_name = format!("org-session-{}", &emp_id[..emp_id.len().min(8)]);
    let payload = Arc::new(Mutex::new(Some((spawned, session, rx, pump_ctx))));
    let worker_payload = Arc::clone(&payload);
    match thread::Builder::new().name(thread_name).spawn(move || {
        let Some((spawned, session, rx, pump_ctx)) = worker_payload
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        else {
            return;
        };
        run_pump(spawned, session, rx, pump_ctx);
    }) {
        Ok(handle) => Ok(handle),
        Err(_) => {
            let cleanup_verified = if let Some((spawned, mut session, _rx, pump_ctx)) =
                payload.lock().unwrap_or_else(|p| p.into_inner()).take()
            {
                crate::pump::finalize_child(
                    spawned.child,
                    spawned.stdin,
                    &mut session,
                    &pump_ctx,
                    org_supervise::StopMode::Graceful,
                    false,
                )
            } else {
                mgr.bindings.revoke(emp_id, None);
                false
            };
            if let Some((pid, _)) = meta
                .active_permit
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
            if !cleanup_verified {
                install_unrecovered_pre_pump_handle(mgr, emp_id, meta);
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
) -> bool {
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
    if !cleanup_verified {
        let session_id = pump_ctx
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .session_id
            .clone();
        let _ = mgr.store.persist_recovery_required(
            emp_id,
            Some(&pump_ctx.run_id),
            session_id.as_deref(),
            None,
        );
    }
    let _ = persist_shutdown_cancellation(mgr, emp_id, &pump_ctx.run_id, !cleanup_verified);
    cleanup_verified
}

fn register_live_session_or_abort(
    mgr: &SessionManager,
    emp_id: String,
    mut handle: LiveSessionHandle,
) -> Result<(), ApiError> {
    let disk_unmasked = handle.bwrap_masked_siblings.as_ref().is_some_and(|masked| {
        crate::bwrap_isolation::has_unmasked_bwrap_sibling_in_office(&mgr.office, &emp_id, masked)
    });
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    let bwrap_raced_unmasked = disk_unmasked
        || handle.bwrap_masked_siblings.as_ref().is_some_and(|masked| {
            reg.live
                .keys()
                .chain(reg.starting.iter())
                .any(|other| other != &emp_id && !masked.contains(other))
        });
    if reg.draining || reg.shutting_down || bwrap_raced_unmasked {
        let shutting_down = reg.draining || reg.shutting_down;
        drop(reg);
        if bwrap_raced_unmasked && !shutting_down {
            let _ = crate::bwrap_isolation::reap_live_handle(
                mgr,
                &emp_id,
                handle,
                Some("bwrap_sibling_race_abort"),
                crate::bwrap_isolation::ReapMode::FailClosedRestore {
                    conflict_message: format!(
                        "could not verify teardown of raced sandboxed session `{emp_id}`; resolve recovery first"
                    ),
                },
            )?;
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "a newly started sibling agent was not masked in this sandbox; retry launch",
            ));
        }
        if let Some(thread) = handle.thread.take() {
            let _ = abort_post_spawn_shutdown(
                mgr,
                &emp_id,
                handle.tx,
                thread,
                &handle.state,
                &handle.active_permit,
            );
        }
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "office session manager is shutting down",
        ));
    }
    reg.live.insert(emp_id, handle);
    Ok(())
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
        (
            st.run_id.clone(),
            !joined_ok || st.recovery_required || st.state == SessionState::RecoveryRequired,
        )
    };
    if let Some((pid, _)) = active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        // If `run_pump` did not finish cleanly to take the permit itself, fail closed (`AE-07`).
        let _ = mgr.office.mark_turn_permit_release_pending(&pid);
    }
    if recovery_required {
        let _ =
            mgr.store
                .persist_recovery_required(emp_id, cancelled_run_id.as_deref(), None, None);
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
    oauth_refresh_attempted: bool,
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
    let mut installed_pid: Option<String> = None;
    {
        let mut slot = turn.active_permit.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            if let Some(pair) = pguard.take() {
                installed_pid = Some(pair.0.clone());
                *slot = Some(pair);
            }
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
    if let Some(ref pid) = installed_pid {
        let mut slot = turn.active_permit.lock().unwrap_or_else(|p| p.into_inner());
        if slot.as_ref().is_some_and(|(cur, _)| cur == pid) {
            let _ = slot.take();
            drop(slot);
            crate::router::release_turn_permit_any(
                &mgr.office,
                mgr.config.hq_permits.as_deref(),
                pid,
            );
        }
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

pub(crate) fn is_host_claude_subscription_refresh_eligible(
    mgr: &SessionManager,
    connection_id: &str,
) -> Option<PathBuf> {
    let conn = mgr.office.load_provider_connection(connection_id).ok()??;
    if conn.auth_mode != org_office::organization_family::records::ProviderAuthMode::SubscriptionCli
    {
        return None;
    }
    let adapter = mgr.config.runtimes.resolve(&conn.runtime_driver).ok()?;
    if adapter.id().as_str() != "claude" {
        return None;
    }
    if mgr
        .enrollment
        .vault()
        .load_unwrapped(conn.id.as_str())
        .ok()
        .flatten()
        .is_some()
    {
        return None;
    }
    mgr.config
        .runtimes
        .resolve_binary(adapter.as_ref(), None)
        .ok()
}

fn assemble_launch_env(
    mut launch_env: Vec<(OsString, OsString)>,
    endpoint_env: Vec<(OsString, OsString)>,
    runtime_name: &str,
    allow_host_auth: bool,
    runtime_path: &Path,
    plan: &org_agent_runtime::LaunchPlan,
) -> (Vec<(OsString, OsString)>, Option<u64>, Option<String>) {
    let host_oauth = crate::host_auth::append_host_claude_oauth(
        runtime_name,
        allow_host_auth,
        runtime_path,
        plan,
        &mut launch_env,
    );
    let expires_at_ms = host_oauth.as_ref().and_then(|m| m.expires_at_ms);
    let token_fp = host_oauth.map(|m| m.token_fp);
    launch_env.extend(endpoint_env);
    (launch_env, expires_at_ms, token_fp)
}

fn try_refresh_host_claude_subscription(mgr: &SessionManager, req: &FailoverRequest) -> bool {
    if req.reason != "auth_expired" || req.oauth_refresh_attempted {
        return false;
    }
    let Some(binary) = is_host_claude_subscription_refresh_eligible(mgr, &req.failed_connection_id)
    else {
        return false;
    };
    crate::host_auth::refresh_host_claude_oauth_reactive(&binary, req.oauth_token_fp.as_deref())
}

fn warm_handle_stop_reason(
    office: &Office,
    emp_id: &str,
    handle: &LiveSessionHandle,
    requested_workspace: Option<&str>,
    state: SessionState,
    bwrap_topology_stale: bool,
) -> Option<&'static str> {
    if handle.workspace.as_deref() != requested_workspace {
        return Some("workspace_changed");
    }
    if state != SessionState::Idle {
        return None;
    }
    if bwrap_topology_stale {
        return Some("bwrap_sibling_topology_changed");
    }
    if crate::host_auth::is_oauth_expiring(handle.oauth_expires_at_ms, crate::credentials::now_ms())
    {
        return Some("oauth_token_expiring");
    }
    if handle.connection_id.is_none()
        && office.load_agent_execution(emp_id).ok().flatten().is_some()
    {
        return Some("warm_route_stale");
    }
    None
}

fn collect_non_self_admin_names(
    wt_dir: &Path,
    repo_id: Option<&str>,
    git_state: &org_office::git::OfficeGitState,
    self_admins: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    let mut other_admins = std::collections::BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(wt_dir) {
        for name in entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !self_admins.contains(n))
        {
            other_admins.insert(name);
        }
    }
    if let Some(rid) = repo_id {
        for ws in git_state.workspaces.values() {
            if ws.repository_id == rid
                && is_safe_worktree_admin_name(&ws.admin_dir_name)
                && !self_admins.contains(&ws.admin_dir_name)
            {
                other_admins.insert(ws.admin_dir_name.clone());
            }
        }
    }
    other_admins
}

fn record_failover_connection_health(mgr: &SessionManager, req: &FailoverRequest) {
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
}

fn take_failed_live_handle(
    mgr: &SessionManager,
    req: &FailoverRequest,
) -> Option<Option<LiveSessionHandle>> {
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if reg.draining || reg.shutting_down || reg.starting.contains(&req.employee_id) {
        return None;
    }
    if let Some(existing) = reg.live.get(&req.employee_id) {
        let st = existing.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.recovery_required
            || st.state == SessionState::RecoveryRequired
            || st.run_id.as_deref() != Some(req.failed_run_id.as_str())
        {
            return None;
        }
    }
    let old = reg.live.remove(&req.employee_id);
    reg.starting.insert(req.employee_id.clone());
    Some(old)
}

pub(crate) fn run_failover_relaunch(mgr: &SessionManager, req: FailoverRequest) {
    if is_admissions_closed(mgr) {
        return;
    }
    let is_stale_recycle = req.reason == "warm_route_stale";
    let refreshed_same_conn = !is_stale_recycle && try_refresh_host_claude_subscription(mgr, &req);
    if !is_stale_recycle && !refreshed_same_conn {
        record_failover_connection_health(mgr, &req);
    }
    let Some(dead_handle) = take_failed_live_handle(mgr, &req) else {
        if is_stale_recycle && !is_admissions_closed(mgr) {
            enqueue_recovery_wake(mgr, &req, None);
        }
        return;
    };
    let _guard = StartSlotGuard::new(&mgr.registry, req.employee_id.clone());
    if let Some(old) = dead_handle {
        if crate::bwrap_isolation::reap_live_handle(
            mgr,
            &req.employee_id,
            old,
            Some("pool_failover"),
            crate::bwrap_isolation::ReapMode::FailClosedRestore {
                conflict_message: format!(
                    "could not verify teardown of failed session `{}`; resolve recovery first",
                    req.employee_id
                ),
            },
        )
        .is_err()
        {
            return;
        }
    }
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
    let mut excluded: HashSet<String> = if is_stale_recycle {
        HashSet::new()
    } else {
        req.tried_connections.iter().cloned().collect()
    };
    if refreshed_same_conn {
        excluded.remove(&req.failed_connection_id);
    }
    let opts = LaunchRouteOpts {
        resume_runtime_session_id: None,
        task_ref: req.task_ref.as_deref(),
        is_failover_switch: !is_stale_recycle,
        oauth_refresh_attempted: req.oauth_refresh_attempted || refreshed_same_conn,
        excluded_connections: excluded,
        tried_connections: if is_stale_recycle {
            Vec::new()
        } else {
            req.tried_connections.clone()
        },
    };
    let objective = mgr.prepend_watch_notices(employee.id.as_str(), req.objective.clone());
    let launch_res = mgr
        .plan_session_launch(&employee, &params, opts)
        .and_then(|planned| mgr.issue_and_spawn_launch(objective, planned))
        .and_then(|prepared| mgr.install_live_session(prepared));
    if let Err(err) = launch_res {
        record_failover_exhausted(mgr, &req, &err);
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
    enqueue_recovery_wake(
        mgr,
        req,
        Some(crate::wake::WakeHoldReason::AdmissionRefused),
    );
}

fn enqueue_recovery_wake(
    mgr: &SessionManager,
    req: &FailoverRequest,
    hold: Option<crate::wake::WakeHoldReason>,
) {
    let ts = crate::credentials::now_ms();
    let _lock = mgr.lifecycle_lock.lock().unwrap_or_else(|p| p.into_inner());
    let mut queue = mgr
        .store
        .read_pending_wakes(&req.employee_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::wake::PendingWakeQueueRecord::empty(&req.employee_id));
    queue.push_dedup(crate::wake::PendingWakeItem {
        item_id: format!(
            "failover:{}:{}:{}",
            req.reason, req.failed_connection_id, ts
        ),
        kind: crate::wake::WakeKind::Recovery,
        sender_id: mgr.office.owner_member_id().to_owned(),
        sender_name: "Office Supervisor".into(),
        sender_handle: None,
        sender_relation: Some("office_supervisor".into()),
        sender_is_person: true,
        conversation_id: None,
        source_id: req.failed_connection_id.clone(),
        summary: req.objective.clone(),
        enqueued_at_ms: ts,
    });
    if let Some(reason) = hold {
        queue.set_hold(reason, ts, None);
    }
    let tokens = mgr.bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let _ = mgr.store.write_pending_wakes_redacted(&queue, &refs);
    set_waiting_messages_count(&mgr.waiting_messages, &req.employee_id, queue.items.len());
}

pub(crate) fn set_waiting_messages_count(
    map: &Mutex<HashMap<String, Arc<AtomicU32>>>,
    employee_id: &str,
    count: usize,
) {
    let clamped = count.min(u32::MAX as usize) as u32;
    let mut guard = map.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(counter) = guard.get(employee_id) {
        counter.store(clamped, Ordering::Relaxed);
    } else {
        guard.insert(employee_id.to_owned(), Arc::new(AtomicU32::new(clamped)));
    }
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

pub(crate) fn ensure_owner_only_dir(path: &Path) -> Result<(), ApiError> {
    org_platform::fs::create_owner_only_dir_all(path)
        .and_then(|()| org_platform::fs::set_owner_only_dir(path))
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not create private session directory",
            )
        })
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

fn is_linux_claude_bwrap(sandbox_enabled: bool, runtime_id: &str) -> bool {
    sandbox_enabled
        && runtime_id == "claude"
        && !org_platform::confinement::unix_socket_path_filter_supported()
}

fn apply_linux_claude_bwrap_pruning(spec: &mut SessionSpec) {
    // Keep `preset_cfg.policy.deny` 100% complete in `SessionManager` (enforced by
    // `decide_escalation_with_roots` over `--permission-mode manual` on every tool call),
    // while pruning absent concrete paths from Linux Claude's `spec.sandbox` and
    // `spec.policy.deny` so Claude 2.1.x `@anthropic-ai/sandbox-runtime` (`bwrap`), which
    // merges `permissions.deny` into `bwrap` mounts, never materializes permanent 0-byte
    // `0444` files on the host (`TASK-144`).
    if prune_nonexistent_bwrap_denies(&mut spec.sandbox, &mut spec.policy) {
        // Pin approval-bridge mediation explicitly whenever any policy deny rule is pruned:
        // never rely on `capabilities().enforces_deny_rules` or surviving deny count to clear
        // `permissions.allow` or downgrade `AcceptEdits -> Ask`.
        spec.policy.allow.clear();
        if spec.permission_mode == org_agent_runtime::PermissionMode::AcceptEdits {
            spec.permission_mode = org_agent_runtime::PermissionMode::Ask;
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "launch spec builder takes runtime, employee, model, workspace, read-only dirs, instructions, preset, and bwrap flag"
)]
fn build_session_spec_for_launch(
    runtime: RuntimeId,
    employee: &Employee,
    model: Option<Token>,
    workspace: SpecPath,
    read_only_dirs: Vec<SpecPath>,
    instructions: String,
    preset_cfg: &PresetConfig,
    linux_claude_bwrap: bool,
) -> Result<SessionSpec, ApiError> {
    let mut spec = build_session_spec(
        runtime,
        employee,
        model,
        workspace,
        read_only_dirs,
        instructions,
        preset_cfg,
    )?;
    if linux_claude_bwrap {
        apply_linux_claude_bwrap_pruning(&mut spec);
    }
    Ok(spec)
}

fn compute_launch_spec_hash(spec: &SessionSpec, route_digest_suffix: &str) -> String {
    if route_digest_suffix.is_empty() {
        spec.spec_hash()
    } else {
        format!("{}:route={route_digest_suffix}", spec.spec_hash())
    }
}

fn build_launch_context(
    runtime_path: PathBuf,
    runtime_dir: &Path,
    tmpdir: &Path,
    session_id: &str,
    resume_runtime_session_id: Option<&str>,
    runtime_name: &str,
    allow_host_auth: bool,
) -> Result<LaunchContext, ApiError> {
    let resume = resume_runtime_session_id.and_then(|raw| SessionId::try_from(raw).ok());
    // Claude Code (`--session-id`), Codex, and Agy accept canonical lowercase UUIDs;
    // Claude Code 2.1.323+ rejects non-UUID strings before the `initialize` handshake.
    let new_session_id = if resume.is_some() {
        None
    } else {
        SessionId::try_from(session_id).ok()
    };
    Ok(LaunchContext {
        runtime_path,
        runtime_version: None,
        runtime_dir: canonical_or_spec(runtime_dir)?,
        tmpdir: canonical_or_spec(tmpdir)?,
        office_env_names: office_env_names(),
        new_session_id,
        resume,
        config_dir: crate::host_auth::host_config_dir_for(runtime_name, allow_host_auth),
    })
}

fn build_session_spec(
    runtime: RuntimeId,
    employee: &Employee,
    model: Option<Token>,
    workspace: SpecPath,
    read_only_dirs: Vec<SpecPath>,
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
        read_only_dirs,
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

pub(crate) fn agent_git_identity(name: &str, handle: &str) -> (String, String) {
    let safe_handle = sanitize_agent_name(handle);
    let clean_name: String = name
        .chars()
        .filter(|c| !matches!(c, '<' | '>'))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = clean_name.trim();
    let has_ident_char = trimmed.chars().any(char::is_alphanumeric);
    let display = if has_ident_char {
        trimmed.to_owned()
    } else {
        safe_handle.clone()
    };
    let email = format!("{safe_handle}@agents.agentic-org.local");
    (display, email)
}

fn push_safe_ro_dir(
    out: &mut Vec<SpecPath>,
    candidate: &Path,
    forbidden_canon: &[PathBuf],
    host_home_canon: Option<&Path>,
    disallowed_containers: &[PathBuf],
) {
    if !candidate.is_absolute() || !candidate.is_dir() {
        return;
    }
    let Ok(resolved) = candidate.canonicalize() else {
        return;
    };
    let overlaps_forbidden = resolved.parent().is_none()
        || host_home_canon.is_some_and(|h| resolved == h || h.starts_with(&resolved))
        || forbidden_canon
            .iter()
            .any(|root| resolved.starts_with(root) || root.starts_with(&resolved))
        || disallowed_containers
            .iter()
            .any(|c| c.starts_with(&resolved));
    if overlaps_forbidden {
        return;
    }
    let text = resolved.display().to_string();
    if let Ok(spec) = SpecPath::try_from(text.as_str()) {
        if !spec.is_root() && !out.contains(&spec) {
            out.push(spec);
        }
    }
}

/// Collects safe read-only directories for Agy `--sandbox` (`--add-dir`) so `agent-os` and the
/// host Rust toolchain execute without `BypassSandbox` (`TASK-164`). Mounts only individual safe
/// Cargo subdirs (`bin`, `registry`, `git`) — NEVER host `~/.cargo` root, `$HOME`, or any
/// ancestor of a forbidden office/workspace directory, so secrets stay inaccessible.
pub(crate) fn collect_agy_read_only_dirs(
    helper_dir: &Path,
    rustup_explicit: Option<OsString>,
    cargo_explicit: Option<OsString>,
    host_home: Option<OsString>,
    forbidden: &[PathBuf],
) -> Vec<SpecPath> {
    let forbidden_canon: Vec<PathBuf> = forbidden
        .iter()
        .map(|p| canonicalize_with_tail(p))
        .collect();
    let host_home_canon = host_home
        .as_deref()
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .and_then(|p| p.canonicalize().ok());
    let mut out = Vec::new();
    push_safe_ro_dir(
        &mut out,
        helper_dir,
        &forbidden_canon,
        host_home_canon.as_deref(),
        &[],
    );
    // If `<helper_dir>/agent-os` is a symlink into a release-store directory, also mount the
    // canonical release directory holding the target binary.
    if let Ok(agent_os_real) = helper_dir.join("agent-os").canonicalize() {
        if let Some(parent) = agent_os_real.parent() {
            push_safe_ro_dir(
                &mut out,
                parent,
                &forbidden_canon,
                host_home_canon.as_deref(),
                &[],
            );
        }
    }
    if let Some(rustup) =
        host_rust_dir_env_from(rustup_explicit, host_home.clone(), ".rustup", forbidden)
    {
        push_safe_ro_dir(
            &mut out,
            Path::new(&rustup),
            &forbidden_canon,
            host_home_canon.as_deref(),
            &[],
        );
    }
    if let Some(cargo) = host_rust_dir_env_from(cargo_explicit, host_home, ".cargo", forbidden) {
        let cargo_path = Path::new(&cargo);
        let cargo_canon = canonicalize_with_tail(cargo_path);
        for sub in ["bin", "registry", "git"] {
            push_safe_ro_dir(
                &mut out,
                &cargo_path.join(sub),
                &forbidden_canon,
                host_home_canon.as_deref(),
                std::slice::from_ref(&cargo_canon),
            );
        }
    }
    out
}

fn write_owner_only_binding_file(path: &Path, token: &str) -> Result<(), ApiError> {
    let _ = std::fs::remove_file(path);
    org_platform::fs::write_owner_only_file(path, token.as_bytes())
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not write session binding"))
}

struct SessionEnvInput<'a> {
    home: &'a Path,
    workspace: &'a Path,
    tmpdir: &'a Path,
    employee_id: &'a str,
    employee_name: &'a str,
    employee_handle: &'a str,
    session_id: &'a str,
    token: &'a str,
    binding_file: &'a Path,
    sandbox_enabled: bool,
}

fn ensure_real_agent_subdir(parent: &Path, child: &Path) -> bool {
    let Ok(parent_meta) = std::fs::symlink_metadata(parent) else {
        return false;
    };
    if !parent_meta.file_type().is_dir() {
        return false;
    }
    match std::fs::symlink_metadata(child) {
        Ok(meta) => meta.file_type().is_dir(), // Rejects symlinks and regular files (B1).
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            org_platform::fs::create_owner_only_dir(child).is_ok()
        }
        Err(_) => false,
    }
}

fn ensure_readonly_cache_symlink(
    link_path: &Path,
    target: &Path,
    forbidden_canon: &[PathBuf],
) -> bool {
    if !target.is_dir() {
        return false;
    }
    let Ok(resolved_target) = target.canonicalize() else {
        return false;
    };
    if forbidden_canon
        .iter()
        .any(|root| resolved_target.starts_with(root))
    {
        return false;
    }
    match std::fs::symlink_metadata(link_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if std::fs::read_link(link_path).ok().as_deref() == Some(resolved_target.as_path()) {
                return true;
            }
            let _ = std::fs::remove_file(link_path);
        }
        Ok(meta) => return meta.file_type().is_dir(), // Regular file/non-dir is NOT a valid cache.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return false,
    }
    org_platform::fs::create_symlink(&resolved_target, link_path).is_ok()
}

/// Links host read-only Rust cache subdirs (`registry`, `git`, `.rustup`) into an isolated agent
/// Symlinks safe read-only host Cargo caches (`.cargo/registry`, `.cargo/git`) into an agent's
/// isolated home (`TASK-159`) without ever linking host Cargo credentials or config files
/// (`credentials*`, `config*`). Does NOT symlink `.rustup` into `<agent_home>`: `RUSTUP_HOME` is
/// preserved directly in the environment (surviving `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1`), and
/// removing any legacy `<agent_home>/.rustup` symlink prevents Linux `bwrap` sibling-directory
/// traversal from ever masking host `.rustup` (`TASK-163`). Refuses symlinked/non-directory
/// `<agent_home>` or `<agent_home>/.cargo` so an agent can never redirect unsandboxed office
/// writes outside its home (`B1`).
pub(crate) fn link_host_rust_caches(
    agent_home: &Path,
    _rustup_dir: Option<&Path>,
    cargo_dir: Option<&Path>,
    forbidden: &[PathBuf],
) -> Option<PathBuf> {
    let Ok(home_meta) = std::fs::symlink_metadata(agent_home) else {
        return None;
    };
    if !home_meta.file_type().is_dir() {
        return None;
    }
    // Clean up any pre-TASK-163 `<agent_home>/.rustup` symlink without touching real directories.
    let legacy_rustup = agent_home.join(".rustup");
    if let Ok(meta) = std::fs::symlink_metadata(&legacy_rustup) {
        if meta.file_type().is_symlink() {
            let _ = std::fs::remove_file(&legacy_rustup);
        }
    }
    let forbidden_canon: Vec<PathBuf> = forbidden
        .iter()
        .map(|p| canonicalize_with_tail(p))
        .collect();
    let host_cargo = cargo_dir?;
    let agent_cargo = agent_home.join(".cargo");
    if !ensure_real_agent_subdir(agent_home, &agent_cargo) {
        return None;
    }
    let linked_registry = ensure_readonly_cache_symlink(
        &agent_cargo.join("registry"),
        &host_cargo.join("registry"),
        &forbidden_canon,
    );
    let linked_git = ensure_readonly_cache_symlink(
        &agent_cargo.join("git"),
        &host_cargo.join("git"),
        &forbidden_canon,
    );
    (linked_registry || linked_git).then_some(agent_cargo)
}

fn append_rust_toolchain_env(
    captured: &mut Vec<(OsString, OsString)>,
    agent_home: Option<&Path>,
    rustup_explicit: Option<OsString>,
    cargo_explicit: Option<OsString>,
    host_home: Option<OsString>,
    forbidden: &[PathBuf],
    sandbox_enabled: bool,
) {
    let host_rustup =
        host_rust_dir_env_from(rustup_explicit, host_home.clone(), ".rustup", forbidden);
    let host_cargo = host_rust_dir_env_from(cargo_explicit, host_home, ".cargo", forbidden);
    let linked_cargo = agent_home.and_then(|home| {
        link_host_rust_caches(
            home,
            host_rustup.as_deref().map(Path::new),
            host_cargo.as_deref().map(Path::new),
            forbidden,
        )
    });
    if let Some(dir) = host_rustup {
        captured.push((OsString::from("RUSTUP_HOME"), dir));
    }
    if let Some(agent_cargo) = linked_cargo {
        captured.push((OsString::from("CARGO_HOME"), agent_cargo.into_os_string()));
        if sandbox_enabled {
            captured.push((OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")));
        }
    } else if agent_home.is_none() {
        if let Some(dir) = host_cargo {
            captured.push((OsString::from("CARGO_HOME"), dir));
            if sandbox_enabled {
                captured.push((OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")));
            }
        }
    } else if host_cargo.is_some() && sandbox_enabled {
        // Agent home was present (do not expose raw host CARGO_HOME on tamper/empty cache),
        // still keep Cargo offline inside the sandbox.
        captured.push((OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")));
    }
}

fn pattern_exists_for_bwrap(pat: &PathPattern) -> bool {
    let raw = pat.as_str();
    let stripped = raw
        .strip_suffix("/**")
        .or_else(|| raw.strip_suffix("/*"))
        .unwrap_or(raw);
    let prefix = if stripped.is_empty() { "/" } else { stripped };
    if prefix.contains('*') || prefix.contains('?') || prefix.contains('[') {
        return true;
    }
    std::fs::symlink_metadata(prefix).is_ok()
}

/// Normalizes a concrete existing directory glob (`<dir>/**` or `<dir>/*` with no internal
/// wildcards) into `<dir>` for Linux Claude `bwrap` so `@anthropic-ai/sandbox-runtime` mounts
/// `--tmpfs <dir>` atomically at the directory boundary instead of traversing inside `<dir>` and
/// following symlinks (such as a sibling agent's `.cargo/registry` or `.rustup`, `TASK-163`).
fn normalize_bwrap_concrete_dir_pattern(pat: &PathPattern) -> (PathPattern, bool) {
    let raw = pat.as_str();
    let Some(stripped) = raw.strip_suffix("/**").or_else(|| raw.strip_suffix("/*")) else {
        return (pat.clone(), false);
    };
    if stripped.is_empty()
        || stripped.contains('*')
        || stripped.contains('?')
        || stripped.contains('[')
    {
        return (pat.clone(), false);
    }
    if std::fs::symlink_metadata(stripped).is_ok_and(|m| m.file_type().is_dir()) {
        if let Ok(normalized) = PathPattern::try_from(stripped) {
            return (normalized, true);
        }
    }
    (pat.clone(), false)
}

/// Drops non-existent concrete paths from Linux Claude `bwrap` `sandbox.deny_read`,
/// `sandbox.deny_write`, and `policy.deny` `RuleScope::Path` entries (`TASK-144`), and normalizes
/// concrete existing directory globs (`dir/**` -> `dir`) on `deny_read` and `policy.deny` so
/// `@anthropic-ai/sandbox-runtime` never traverses symlinks inside denied sibling agent homes
/// (`TASK-163`). Returns `true` if any `policy.deny` rule was pruned or normalized. Callers must
/// leave `preset_cfg.policy.deny` untouched so `SessionManager`'s approval bridge
/// (`decide_escalation_with_roots`) retains 100% coverage of absent/future and glob paths.
pub(crate) fn prune_nonexistent_bwrap_denies(
    sandbox: &mut SandboxSpec,
    policy: &mut org_agent_runtime::PermissionPolicy,
) -> bool {
    sandbox.deny_read.retain(pattern_exists_for_bwrap);
    for pat in &mut sandbox.deny_read {
        let (norm, _) = normalize_bwrap_concrete_dir_pattern(pat);
        *pat = norm;
    }
    sandbox.deny_write.retain(pattern_exists_for_bwrap);
    let before = policy.deny.len();
    policy.deny.retain(|rule| match &rule.scope {
        org_agent_runtime::RuleScope::Path(pat) => pattern_exists_for_bwrap(pat),
        _ => true,
    });
    let mut modified = policy.deny.len() < before;
    for rule in &mut policy.deny {
        if let org_agent_runtime::RuleScope::Path(pat) = &mut rule.scope {
            let (norm, changed) = normalize_bwrap_concrete_dir_pattern(pat);
            if changed {
                *pat = norm;
                modified = true;
            }
        }
    }
    modified
}

fn office_env_names() -> Vec<EnvName> {
    [
        "AGENT_OS_SOCKET",
        "AGENT_OS_OFFICE",
        "AGENT_OS_ORG",
        "AGENT_OS_AGENT_ID",
        "AGENT_OS_SESSION_ID",
        "AGENT_OS_SESSION_TOKEN",
        "AGENT_OS_BINDING_FILE",
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "CARGO_NET_OFFLINE",
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

fn canonicalize_with_tail(path: &Path) -> PathBuf {
    if let Ok(resolved) = path.canonicalize() {
        return resolved;
    }
    let mut tail = Vec::new();
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(mut base) = parent.canonicalize() {
            for seg in tail.into_iter().rev() {
                base.push(seg);
            }
            return base;
        }
        cur = parent;
    }
    path.to_path_buf()
}

pub(crate) fn host_rust_dir_env_from(
    explicit: Option<OsString>,
    home: Option<OsString>,
    default_rel: &str,
    forbidden: &[PathBuf],
) -> Option<OsString> {
    let candidate = match explicit {
        Some(raw) => PathBuf::from(raw),
        None => PathBuf::from(home?).join(default_rel),
    };
    if !candidate.is_absolute() || !candidate.is_dir() {
        return None;
    }
    let resolved = candidate.canonicalize().ok()?;
    let forbidden_canon: Vec<PathBuf> = forbidden
        .iter()
        .map(|p| canonicalize_with_tail(p))
        .collect();
    if forbidden_canon
        .iter()
        .any(|root| resolved.starts_with(root))
    {
        return None;
    }
    Some(resolved.into_os_string())
}

fn collect_existing_worktree_dirs(
    dir: &Path,
    depth: usize,
    out: &mut std::collections::BTreeSet<PathBuf>,
) {
    if depth > 6 {
        return;
    }
    if dir.join(".git").exists() {
        out.insert(canonicalize_with_tail(dir));
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            collect_existing_worktree_dirs(&entry.path(), depth + 1, out);
        }
    }
}

fn is_safe_worktree_admin_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub(crate) fn collect_self_active_admins(
    git_state: &org_office::git::OfficeGitState,
    employee_id: &str,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut by_repo: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for ws in git_state.workspaces.values() {
        if ws.employee_id == employee_id
            && ws.state == "active"
            && is_safe_worktree_admin_name(&ws.repository_id)
            && is_safe_worktree_admin_name(&ws.admin_dir_name)
        {
            by_repo
                .entry(ws.repository_id.clone())
                .or_default()
                .insert(ws.admin_dir_name.clone());
        }
    }
    by_repo
}

fn append_cargo_credential_denies(out: &mut Vec<PathPattern>) {
    let mut dirs = Vec::new();
    if let Some(raw) = std::env::var_os("CARGO_HOME") {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            dirs.push(p);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".cargo");
        if p.is_absolute() {
            dirs.push(p);
        }
    }
    // Deny Cargo registry token files (`credentials`, `credentials.toml`) only. Do NOT read-deny
    // `config` / `config.toml`: Cargo unconditionally opens them on every `cargo` invocation, and
    // Linux `bwrap` turns read-denies on non-existent files into `EACCES` stubs, breaking `cargo`.
    // `$CARGO_HOME` stays outside `writable_roots`, so agents still cannot modify `config.toml`.
    for dir in dirs {
        for file in ["credentials", "credentials.toml"] {
            push_exact_pattern(out, &dir.join(file));
        }
    }
}

fn append_agy_credential_denies(out: &mut Vec<PathPattern>) {
    if let Some(home) = std::env::var_os("HOME") {
        let gemini = PathBuf::from(home).join(".gemini");
        if gemini.is_absolute() {
            for rel in org_agent_runtime_agy::AGY_CONFIG_CREDENTIAL_FILES {
                push_exact_pattern(out, &gemini.join(rel));
            }
        }
    }
}

fn append_bare_cache_sandbox_rules(
    cache_dir: &Path,
    self_admin_names: &std::collections::BTreeSet<String>,
    writable: &mut Vec<SpecPath>,
    write_denies: &mut Vec<PathPattern>,
) {
    let canon = canonicalize_with_tail(cache_dir);
    for rel in ["objects", "refs/heads/task", "logs/refs/heads/task"] {
        let p = canon.join(rel);
        if let Ok(spec) = SpecPath::try_from(p.to_string_lossy().as_ref()) {
            writable.push(spec);
        }
    }
    // Grant ONLY concrete self worktree admin directories (`doc-35` §Sandbox / `FC-103`),
    // never the parent `<cache>/worktrees` directory.
    for admin in self_admin_names {
        let admin_dir = canon.join("worktrees").join(admin);
        if let Ok(spec) = SpecPath::try_from(admin_dir.to_string_lossy().as_ref()) {
            writable.push(spec);
        }
        for admin_file in ["commondir", "gitdir", "config.worktree"] {
            push_raw_pattern(
                write_denies,
                &format!("{}/worktrees/{admin}/{admin_file}", canon.display()),
            );
        }
    }
    // Always protect directories inside writable roots (`objects/info`) and core bare-repo dirs
    // (`hooks`, `info`, `refs/remotes`, `refs/tags`). Note `org-git::cache::init_bare_cache`
    // deletes `<cache>/hooks`; keeping `hooks` write-denied prevents re-creation. Guard optional
    // dirs (`refs/office`, `refs/replace`) via `symlink_metadata` (catches dangling symlinks too)
    // so `bwrap` never turns absent `refs/replace` into an `EACCES` stub; `<cache>` and
    // `<cache>/refs` remain outside `writable_roots` (enforced by OS sandbox and approval bridge).
    for rel_dir in ["hooks", "info", "objects/info", "refs/remotes", "refs/tags"] {
        push_raw_pattern(write_denies, &format!("{}/{rel_dir}", canon.display()));
        push_raw_pattern(write_denies, &format!("{}/{rel_dir}/**", canon.display()));
    }
    for rel_dir in ["refs/office", "refs/replace"] {
        if std::fs::symlink_metadata(canon.join(rel_dir)).is_ok() {
            push_raw_pattern(write_denies, &format!("{}/{rel_dir}", canon.display()));
            push_raw_pattern(write_denies, &format!("{}/{rel_dir}/**", canon.display()));
        }
    }
    // Always protect core bare-repo metadata files (`config`, `HEAD`, `description`). Guard
    // optional Git files (`packed-refs`, `shallow`, `FETCH_HEAD`, `config.worktree`) ONLY when
    // present (`symlink_metadata().is_ok()` catches dangling symlinks too): `<cache>` stays
    // outside `writable_roots` (enforced by both OS sandbox and approval bridge), and listing
    // absent `packed-refs` in `denyWrite` makes Linux `bwrap` return `EACCES` instead of `ENOENT`.
    for rel_file in ["config", "HEAD", "description"] {
        push_raw_pattern(write_denies, &format!("{}/{rel_file}", canon.display()));
    }
    for rel_file in ["packed-refs", "shallow", "FETCH_HEAD", "config.worktree"] {
        if std::fs::symlink_metadata(canon.join(rel_file)).is_ok() {
            push_raw_pattern(write_denies, &format!("{}/{rel_file}", canon.display()));
        }
    }
}

fn push_raw_pattern(out: &mut Vec<PathPattern>, raw: &str) {
    if let Ok(pat) = PathPattern::try_from(raw) {
        if !out.contains(&pat) {
            out.push(pat);
        }
    }
}

fn push_glob_pattern(out: &mut Vec<PathPattern>, path: &Path) {
    let resolved = canonicalize_with_tail(path);
    push_raw_pattern(out, &format!("{}/**", resolved.display()));
}

fn push_exact_pattern(out: &mut Vec<PathPattern>, path: &Path) {
    push_raw_pattern(out, &path.display().to_string());
    let resolved = canonicalize_with_tail(path);
    push_raw_pattern(out, &resolved.display().to_string());
}

fn append_privileged_socket_patterns(out: &mut Vec<PathPattern>) {
    // Keep non-wildcard system socket paths plus concrete `$XDG_RUNTIME_DIR` children here
    // (avoiding `/run/user/*/...` wildcards which Codex's `path_pattern_base` truncates to
    // `/run/user`, blocking `/run/user/<uid>` sockets/tmpdir; Claude adds `/run/user/*/...`
    // privately in `org-agent-runtime-claude/src/plan.rs`).
    for privileged in [
        "/run/dbus/system_bus_socket",
        "/var/run/dbus/system_bus_socket",
        "/var/run/docker.sock",
        "/run/docker.sock",
        "/run/podman/**",
    ] {
        push_raw_pattern(out, privileged);
    }
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        let xdg_path = PathBuf::from(xdg);
        if xdg_path.is_absolute() {
            push_exact_pattern(out, &xdg_path.join("bus"));
            push_glob_pattern(out, &xdg_path.join("systemd"));
            push_glob_pattern(out, &xdg_path.join("podman"));
        }
    }
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

#[cfg(test)]
mod manager_helper_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn combine_agent_instructions_merges_and_deduplicates_cleanly() {
        assert_eq!(combine_agent_instructions("", None), "");
        assert_eq!(
            combine_agent_instructions("  role body  ", None),
            "role body"
        );
        assert_eq!(
            combine_agent_instructions("", Some("  exec prompt  ")),
            "exec prompt"
        );
        assert_eq!(
            combine_agent_instructions("same text", Some("same text")),
            "same text"
        );
        assert_eq!(
            combine_agent_instructions("profile", Some("exec")),
            "exec\n\nprofile"
        );
    }

    #[test]
    fn binding_file_is_owner_only_and_removed_on_guard_drop() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session-binding-run-1.v1");
        let bindings = RunBindingRegistry::new();
        let issued = bindings.issue("emp-1", "run-1", "sess-1", 0).unwrap();
        {
            let _guard = BindingGuard::new(&bindings, "emp-1", "run-1", &file);
            write_owner_only_binding_file(&file, issued.expose_token()).unwrap();
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                issued.expose_token()
            );
        }
        assert!(!file.exists(), "BindingGuard drop must unlink binding file");
    }

    #[test]
    fn prune_nonexistent_bwrap_denies_drops_absent_paths_and_preserves_policy_enforcement() {
        use crate::preset::{decide_escalation_with_roots, EscalationDisposition};
        use org_agent_runtime::{
            AgentRuntime, ApprovalId, ApprovalKind, ApprovalRequest, Escalation, ToolCapability,
        };

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        let existing_db = root.join("office.db");
        let existing_dir = root.join("objects").join("info");
        let existing_sibling = root.join("agents").join("sibling-live");
        let absent_journal = root.join("office.db-journal");
        let absent_sibling = root.join("agents").join("sibling-1");
        let absent_hooks = root.join("repo.git").join("hooks");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&existing_dir).unwrap();
        std::fs::create_dir_all(existing_sibling.join("home")).unwrap();
        std::fs::write(&existing_db, b"sqlite").unwrap();
        // Plant a symlink inside an existing sibling agent's home pointing to a shared host path
        // (`TASK-163`): Linux bwrap denyRead must mask `sibling-live` at its directory boundary
        // without `/**` traversal so bwrap never follows the symlink onto host `.rustup`/`.cargo`.
        let fake_host_rustup = root.join("host-rustup");
        std::fs::create_dir_all(&fake_host_rustup).unwrap();
        org_platform::fs::create_symlink(&fake_host_rustup, &existing_sibling.join("home/.rustup"))
            .unwrap();

        let denied = vec![
            PathPattern::try_from(existing_db.to_str().unwrap()).unwrap(),
            PathPattern::try_from(format!("{}/**", existing_dir.display()).as_str()).unwrap(),
            PathPattern::try_from(format!("{}/**", existing_sibling.display()).as_str()).unwrap(),
            PathPattern::try_from(absent_journal.to_str().unwrap()).unwrap(),
            PathPattern::try_from(format!("{}/**", absent_sibling.display()).as_str()).unwrap(),
            PathPattern::try_from("/run/user/*/bus").unwrap(),
        ];
        let ws_spec = SpecPath::try_from(workspace.to_str().unwrap()).unwrap();
        let mut preset_cfg = build_session_policy(
            PermissionPreset::AutonomousSandboxed,
            true,
            &ws_spec,
            None,
            &denied,
            1,
        )
        .unwrap();
        let absent_hooks_pat =
            PathPattern::try_from(format!("{}/**", absent_hooks.display()).as_str()).unwrap();
        apply_worktree_sandbox_rules(&mut preset_cfg, &[], &[absent_hooks_pat]);

        // Exercise `apply_linux_claude_bwrap_pruning` directly with `AcceptEdits` and a non-empty
        // allow rule so we verify absent-path pruning clears `spec.policy.allow` and downgrades
        // `AcceptEdits -> Ask`, while `preset_cfg.policy` stays 100% intact for
        // `decide_escalation_with_roots`.
        let mut spec_policy = preset_cfg.policy.clone();
        spec_policy.allow.push(org_agent_runtime::Rule {
            tool: org_agent_runtime::RuleTool::try_from("Read").unwrap(),
            scope: org_agent_runtime::RuleScope::Any,
        });
        let mut spec = SessionSpec {
            runtime: RuntimeId::try_from("claude").unwrap(),
            kind: SessionKind::Home {},
            agent: AgentIdentity {
                name: AgentName::try_from("eng").unwrap(),
                description: "Engineer".to_owned(),
                prompt: "Engineer".to_owned(),
            },
            model: None,
            effort: None,
            workspace: ws_spec,
            read_only_dirs: Vec::new(),
            instructions: String::new(),
            permission_mode: org_agent_runtime::PermissionMode::AcceptEdits,
            policy: spec_policy,
            sandbox: preset_cfg.sandbox.clone(),
            mcp_servers: Default::default(),
            tools: preset_cfg.allowed_tools.clone(),
        };
        apply_linux_claude_bwrap_pruning(&mut spec);
        assert_eq!(spec.permission_mode, org_agent_runtime::PermissionMode::Ask);
        assert!(spec.policy.allow.is_empty());

        let deny_read: Vec<&str> = spec
            .sandbox
            .deny_read
            .iter()
            .map(PathPattern::as_str)
            .collect();
        let deny_write: Vec<&str> = spec
            .sandbox
            .deny_write
            .iter()
            .map(PathPattern::as_str)
            .collect();
        assert!(deny_read.contains(&existing_db.to_str().unwrap()));
        // Concrete existing directory globs normalize to directory paths without trailing `/**`
        // so Linux `bwrap` masks them atomically without traversing symlinks (`TASK-163`).
        assert!(deny_read.contains(&existing_dir.to_str().unwrap()));
        assert!(!deny_read.iter().any(|p| p.ends_with("objects/info/**")));
        assert!(deny_read.contains(&existing_sibling.to_str().unwrap()));
        assert!(!deny_read.iter().any(|p| p.ends_with("sibling-live/**")));
        assert!(deny_read.contains(&"/run/user/*/bus"));
        assert!(!deny_read.contains(&absent_journal.to_str().unwrap()));
        assert!(!deny_read.iter().any(|p| p.contains("sibling-1")));
        assert!(!deny_write.iter().any(|p| p.contains("repo.git/hooks")));
        assert!(!deny_write.iter().any(|p| p.contains(".git/hooks")));
        let runtime_dir = root.join("rt");
        let tmpdir = root.join("tmp");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&tmpdir).unwrap();
        let ctx = LaunchContext {
            runtime_path: PathBuf::from("/usr/local/bin/claude"),
            runtime_version: None,
            runtime_dir: SpecPath::try_from(runtime_dir.to_str().unwrap()).unwrap(),
            tmpdir: SpecPath::try_from(tmpdir.to_str().unwrap()).unwrap(),
            office_env_names: office_env_names(),
            new_session_id: Some(
                SessionId::try_from("11111111-1111-4111-8111-111111111111").unwrap(),
            ),
            resume: None,
            config_dir: None,
        };
        let (plan, _) = org_agent_runtime_claude::ClaudeRuntime::new()
            .plan_session(&spec, &ctx)
            .unwrap();
        let settings_bytes = &plan
            .files
            .iter()
            .find(|f| f.rel_path.as_str() == "claude-settings.json")
            .unwrap()
            .contents;
        let settings: serde_json::Value = serde_json::from_slice(settings_bytes).unwrap();
        let perm_denies: Vec<&str> = settings["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert!(perm_denies.contains(&format!("Read(/{})", existing_db.display()).as_str()));
        assert!(!perm_denies.contains(&format!("Read(/{})", absent_journal.display()).as_str()));
        assert!(!perm_denies.contains(&format!("Read(/{}/**)", absent_sibling.display()).as_str()));
        assert!(!perm_denies.contains(&format!("Write(/{}/**)", absent_hooks.display()).as_str()));
        assert_eq!(settings["permissions"]["allow"], serde_json::json!([]));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--permission-mode", "manual"]),
            "pruned Linux Claude spec must stay pinned to --permission-mode manual: {:?}",
            plan.args
        );

        let mut write_req = ApprovalRequest::new(
            ApprovalId::new("req-absent-sibling"),
            ApprovalKind::FileChange,
            Escalation::None,
            "Write",
            serde_json::json!({
                "file_path": format!("{}/secret.txt", absent_sibling.display())
            }),
        );
        write_req.capability = Some(ToolCapability::Edit);
        assert!(matches!(
            decide_escalation_with_roots(
                &write_req,
                &preset_cfg.policy,
                preset_cfg.preset,
                None,
                Some(&preset_cfg.sandbox.writable_roots),
            ),
            EscalationDisposition::AutoDeny { .. }
        ));
    }

    #[test]
    fn cargo_net_offline_and_zero_copy_rust_cache_symlinks_never_expose_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let cargo_dir = root.join("host-cargo");
        let rustup_dir = root.join("host-rustup");
        let helper_dir = root.join("helpers");
        let home_dir = root.join("home");
        let ws_dir = root.join("ws");
        for d in [
            &cargo_dir,
            &cargo_dir.join("registry"),
            &cargo_dir.join("git"),
            &rustup_dir,
            &helper_dir,
            &home_dir,
            &ws_dir,
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(cargo_dir.join("credentials"), b"secret-1").unwrap();
        std::fs::write(cargo_dir.join("credentials.toml"), b"secret-2").unwrap();
        std::fs::write(cargo_dir.join("config.toml"), b"host-config").unwrap();
        let search = SearchPath::new(
            &helper_dir,
            std::ffi::OsStr::new("/usr/bin"),
            &[home_dir.clone(), ws_dir.clone()],
        )
        .unwrap();

        let mut sandboxed = Vec::new();
        append_rust_toolchain_env(
            &mut sandboxed,
            Some(&home_dir),
            Some(rustup_dir.as_os_str().to_os_string()),
            Some(cargo_dir.as_os_str().to_os_string()),
            None,
            &[home_dir.clone(), ws_dir.clone()],
            true,
        );
        assert!(sandboxed
            .iter()
            .any(|(k, v)| k == "CARGO_NET_OFFLINE" && v == "true"));
        assert!(sandboxed
            .iter()
            .any(|(k, v)| k == "CARGO_HOME" && v == home_dir.join(".cargo").as_os_str()));
        assert!(Environment::new(search, &sandboxed).is_ok());

        // Safe Cargo cache dirs are symlinked; credentials, config, and redundant `.rustup` are
        // NEVER linked into agent home (`TASK-159`, `TASK-163`: `RUSTUP_HOME` is preserved in env).
        assert_eq!(
            std::fs::read_link(home_dir.join(".cargo").join("registry")).unwrap(),
            cargo_dir.join("registry")
        );
        assert_eq!(
            std::fs::read_link(home_dir.join(".cargo").join("git")).unwrap(),
            cargo_dir.join("git")
        );
        assert!(!home_dir.join(".rustup").exists());
        assert!(sandboxed
            .iter()
            .any(|(k, v)| k == "RUSTUP_HOME" && v == rustup_dir.as_os_str()));
        assert!(!home_dir.join(".cargo").join("credentials").exists());
        assert!(!home_dir.join(".cargo").join("credentials.toml").exists());
        assert!(!home_dir.join(".cargo").join("config.toml").exists());

        // Legacy `<agent_home>/.rustup` symlink is automatically cleaned up (`TASK-163`).
        org_platform::fs::create_symlink(&rustup_dir, &home_dir.join(".rustup")).unwrap();
        assert!(home_dir.join(".rustup").symlink_metadata().is_ok());
        assert!(link_host_rust_caches(
            &home_dir,
            Some(&rustup_dir),
            Some(&cargo_dir),
            &[home_dir.clone(), ws_dir.clone()],
        )
        .is_some());
        assert!(home_dir.join(".rustup").symlink_metadata().is_err());

        // B1: symlinked `<agent_home>/.cargo` must be refused without planting symlinks outside home
        // or exporting raw host CARGO_HOME.
        let escape_target = root.join("external-target");
        std::fs::create_dir_all(&escape_target).unwrap();
        let tampered_home = root.join("tampered-home");
        std::fs::create_dir_all(&tampered_home).unwrap();
        org_platform::fs::create_symlink(&escape_target, &tampered_home.join(".cargo")).unwrap();
        let mut tampered_env = Vec::new();
        append_rust_toolchain_env(
            &mut tampered_env,
            Some(&tampered_home),
            Some(rustup_dir.as_os_str().to_os_string()),
            Some(cargo_dir.as_os_str().to_os_string()),
            None,
            &[tampered_home.clone(), ws_dir.clone()],
            true,
        );
        assert!(
            !escape_target.join("registry").exists(),
            "office must never plant symlinks through a symlinked .cargo dir"
        );
        assert!(
            !tampered_env.iter().any(|(k, _)| k == "CARGO_HOME"),
            "tampered .cargo must not fall back to raw host CARGO_HOME: {tampered_env:?}"
        );
        assert!(tampered_env
            .iter()
            .any(|(k, v)| k == "CARGO_NET_OFFLINE" && v == "true"));

        // B1: regular file at `<agent_home>/.cargo/registry` must NOT be treated as a valid cache.
        let file_home = root.join("file-home");
        std::fs::create_dir_all(file_home.join(".cargo")).unwrap();
        std::fs::write(file_home.join(".cargo").join("registry"), b"not-a-dir").unwrap();
        std::fs::write(file_home.join(".cargo").join("git"), b"not-a-dir").unwrap();
        assert!(
            link_host_rust_caches(
                &file_home,
                None,
                Some(&cargo_dir),
                &[file_home.clone(), ws_dir.clone()],
            )
            .is_none(),
            "regular file at registry/git must not count as linked cache"
        );

        let mut unsandboxed = Vec::new();
        append_rust_toolchain_env(
            &mut unsandboxed,
            None,
            None,
            Some(cargo_dir.as_os_str().to_os_string()),
            None,
            &[home_dir, ws_dir],
            false,
        );
        assert!(!unsandboxed.iter().any(|(k, _)| k == "CARGO_NET_OFFLINE"));
    }

    #[test]
    fn pattern_exists_for_bwrap_preserves_root_glob_and_agy_credential_denies_match_adapter() {
        assert!(pattern_exists_for_bwrap(
            &PathPattern::try_from("/**").unwrap()
        ));
        assert!(pattern_exists_for_bwrap(
            &PathPattern::try_from("/*").unwrap()
        ));
        let mut agy_denies = Vec::new();
        append_agy_credential_denies(&mut agy_denies);
        if std::env::var_os("HOME").is_some() {
            for rel in org_agent_runtime_agy::AGY_CONFIG_CREDENTIAL_FILES {
                assert!(
                    agy_denies.iter().any(|p| p.as_str().ends_with(rel)),
                    "missing agy credential deny for `{rel}`: {agy_denies:?}"
                );
            }
        }
    }

    #[test]
    fn agy_read_only_dirs_mounts_release_store_and_toolchain_subdirs_never_cargo_root() {
        use org_agent_runtime::AgentRuntime as _;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let release_bin = root.join("release-store/releases/0.1.0-v14");
        let shim_bin = root.join("bin");
        let rustup_dir = root.join("host-rustup");
        let cargo_dir = root.join("host-cargo");
        let home_dir = root.join("agent-home");
        let ws_dir = root.join("workspace");
        let rt_dir = root.join("runtime");
        let tmp_dir = root.join("tmp");
        for d in [
            &release_bin,
            &shim_bin,
            &rustup_dir,
            &cargo_dir.join("bin"),
            &cargo_dir.join("registry"),
            &cargo_dir.join("git"),
            &home_dir,
            &ws_dir,
            &rt_dir,
            &tmp_dir,
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(release_bin.join("agent-os"), b"#!/bin/sh\n").unwrap();
        org_platform::fs::create_symlink(&release_bin.join("agent-os"), &shim_bin.join("agent-os"))
            .unwrap();
        std::fs::write(cargo_dir.join("credentials.toml"), b"secret").unwrap();

        let forbidden = vec![home_dir.clone(), ws_dir.clone()];
        let ro_dirs = collect_agy_read_only_dirs(
            &shim_bin,
            Some(rustup_dir.as_os_str().to_os_string()),
            Some(cargo_dir.as_os_str().to_os_string()),
            None,
            &forbidden,
        );
        let ro_strs: Vec<&str> = ro_dirs.iter().map(SpecPath::as_str).collect();
        assert!(ro_strs.contains(&shim_bin.to_str().unwrap()));
        assert!(ro_strs.contains(&release_bin.to_str().unwrap()));
        assert!(ro_strs.contains(&rustup_dir.to_str().unwrap()));
        assert!(ro_strs.contains(&cargo_dir.join("bin").to_str().unwrap()));
        assert!(ro_strs.contains(&cargo_dir.join("registry").to_str().unwrap()));
        assert!(ro_strs.contains(&cargo_dir.join("git").to_str().unwrap()));
        assert!(
            !ro_strs.contains(&cargo_dir.to_str().unwrap()),
            "must NEVER mount raw host CARGO_HOME root into Agy sandbox: {ro_strs:?}"
        );
        assert!(!ro_strs.contains(&"/"));

        // Ancestors of forbidden dirs (`root` contains `home_dir`/`ws_dir`), raw `$HOME`, and
        // symlinked `CARGO_HOME/bin -> CARGO_HOME` MUST all be refused (`TASK-164`).
        let symlink_cargo = root.join("symlink-cargo");
        std::fs::create_dir_all(&symlink_cargo).unwrap();
        org_platform::fs::create_symlink(&symlink_cargo, &symlink_cargo.join("bin")).unwrap();
        let hostile_ro = collect_agy_read_only_dirs(
            &root,
            Some(root.as_os_str().to_os_string()),
            Some(symlink_cargo.as_os_str().to_os_string()),
            Some(root.as_os_str().to_os_string()),
            &forbidden,
        );
        assert!(
            hostile_ro.is_empty(),
            "ancestors of forbidden dirs, $HOME, and escaped CARGO_HOME/bin must be refused: {hostile_ro:?}"
        );

        // Verify `AgyRuntime::plan_session` emits `--add-dir` for every collected RO dir.
        let ws_spec = SpecPath::try_from(ws_dir.to_str().unwrap()).unwrap();
        let preset_cfg = build_session_policy(
            PermissionPreset::AutonomousSandboxed,
            true,
            &ws_spec,
            None,
            &[],
            1,
        )
        .unwrap();
        let spec = SessionSpec {
            runtime: RuntimeId::try_from("agy").unwrap(),
            kind: SessionKind::Home {},
            agent: AgentIdentity {
                name: AgentName::try_from("worker-eu-agy").unwrap(),
                description: "Worker".to_owned(),
                prompt: "Worker".to_owned(),
            },
            model: None,
            effort: None,
            workspace: ws_spec,
            read_only_dirs: ro_dirs,
            instructions: String::new(),
            permission_mode: preset_cfg.permission_mode,
            policy: preset_cfg.policy,
            sandbox: preset_cfg.sandbox,
            mcp_servers: Default::default(),
            tools: preset_cfg.allowed_tools,
        };
        let ctx = LaunchContext {
            runtime_path: PathBuf::from("/usr/local/bin/agy"),
            runtime_version: None,
            runtime_dir: SpecPath::try_from(rt_dir.to_str().unwrap()).unwrap(),
            tmpdir: SpecPath::try_from(tmp_dir.to_str().unwrap()).unwrap(),
            office_env_names: office_env_names(),
            new_session_id: None,
            resume: None,
            config_dir: None,
        };
        let (plan, _) = org_agent_runtime_agy::AgyRuntime::new()
            .plan_session(&spec, &ctx)
            .unwrap();
        for expected in [
            release_bin.to_str().unwrap(),
            rustup_dir.to_str().unwrap(),
            cargo_dir.join("bin").to_str().unwrap(),
            cargo_dir.join("registry").to_str().unwrap(),
        ] {
            assert!(
                plan.args.windows(2).any(|w| w == ["--add-dir", expected]),
                "Agy LaunchPlan missing --add-dir {expected}: {:?}",
                plan.args
            );
        }
        assert!(
            !plan
                .args
                .windows(2)
                .any(|w| w == ["--add-dir", cargo_dir.to_str().unwrap()]),
            "Agy LaunchPlan must NEVER emit --add-dir for host CARGO_HOME root: {:?}",
            plan.args
        );
    }

    #[test]
    fn agent_git_identity_allows_plain_git_commit_without_global_config() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let helper_dir = root.join("helpers");
        let home_dir = root.join("home");
        let repo_dir = root.join("repo");
        for d in [&helper_dir, &home_dir, &repo_dir] {
            std::fs::create_dir_all(d).unwrap();
        }
        let (git_name, git_email) = agent_git_identity("Worker EU\nAgy", "Worker_EU.Agy");
        assert_eq!(git_name, "Worker EU Agy");
        assert_eq!(git_email, "worker_eu-agy@agents.agentic-org.local");
        assert_eq!(
            agent_git_identity("<>...", "Worker_EU.Agy").0,
            "worker_eu-agy"
        );

        let search = SearchPath::new(
            &helper_dir,
            std::ffi::OsStr::new("/usr/bin:/bin"),
            &[home_dir.clone(), repo_dir.clone()],
        )
        .unwrap();
        let git_context = [
            (OsString::from("GIT_AUTHOR_NAME"), OsString::from(&git_name)),
            (
                OsString::from("GIT_AUTHOR_EMAIL"),
                OsString::from(&git_email),
            ),
            (
                OsString::from("GIT_COMMITTER_NAME"),
                OsString::from(&git_name),
            ),
            (
                OsString::from("GIT_COMMITTER_EMAIL"),
                OsString::from(&git_email),
            ),
        ];
        let env = Environment::new(
            search,
            &[(OsString::from("HOME"), home_dir.as_os_str().to_os_string())],
        )
        .unwrap()
        .with_context(&git_context)
        .unwrap();
        let env_debug = format!("{env:?}");
        let names = office_env_names();
        for required in [
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
        ] {
            assert!(
                names.iter().any(|n| n.as_str() == required),
                "office_env_names missing {required}"
            );
            assert!(
                env_debug.contains(required),
                "Environment missing {required}: {env_debug}"
            );
        }

        // Run a real plain `git commit` with global & system gitconfigs disabled and zero `-c user.*`.
        let init = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo_dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(init.success());
        std::fs::write(repo_dir.join("hello.txt"), b"hi\n").unwrap();
        let add = std::process::Command::new("git")
            .args(["add", "hello.txt"])
            .current_dir(&repo_dir)
            .status()
            .unwrap();
        assert!(add.success());

        let mut commit_cmd = std::process::Command::new("git");
        commit_cmd
            .args(["commit", "-q", "-m", "plain commit"])
            .current_dir(&repo_dir)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &home_dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for (k, v) in &git_context {
            commit_cmd.env(k, v);
        }
        let commit_out = commit_cmd.output().unwrap();
        assert!(
            commit_out.status.success(),
            "plain git commit failed: {}",
            String::from_utf8_lossy(&commit_out.stderr)
        );
        let log = std::process::Command::new("git")
            .args(["log", "-1", "--format=%an|%ae|%cn|%ce"])
            .current_dir(&repo_dir)
            .output()
            .unwrap();
        assert!(log.status.success());
        assert_eq!(
            String::from_utf8_lossy(&log.stdout).trim(),
            format!("{git_name}|{git_email}|{git_name}|{git_email}")
        );
    }
}
