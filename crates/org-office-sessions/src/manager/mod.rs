//! Production office `SessionManager`: spawns supervised agent sessions, coordinates
//! concurrency, enforces owner/agent authorization, persists stall/recovery state, and
//! dispatches all C3 RPCs with idempotent `command_id` replay (`TASK-42`).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use org_agent_runtime::LaunchContext;
use org_office::Office;
use org_proto::sessions::SessionView;
use org_proto::{ApiError, ErrorCode, Response};

use crate::binding::RunBindingRegistry;
use crate::credentials::{
    random_hex, CliEnrollmentDriver, CredentialVault, EnrollmentDriver, EnrollmentManager,
};
use crate::preset::PresetConfig;
use crate::pump::{PumpCommand, SessionRuntimeState};
use crate::recovery::{
    NativeProcessExitOracle, ProcessExitOracle, RecoveryAction, RecoveryReport, SessionStore,
    DEFAULT_STALL_TIMEOUT,
};
use crate::runtime_registry::RuntimeRegistry;
use crate::subscription_usage::{
    collect_subscription_usage, list_subscription_usage, revoke_subscription_usage_credential,
    set_subscription_usage_credential, ProviderSubscriptionUsageCollector,
    SubscriptionUsageCollector,
};

use crate::wake::{
    MidTurnLimits, MidTurnRateTracker, SuspendedAskEntry, DEFAULT_HOURLY_WAKE_LIMIT,
    DEFAULT_SUSPEND_GRACE, DEFAULT_WAKE_WINDOW,
};

#[cfg(test)]
mod agent_os_sandbox_tests;
#[cfg(test)]
mod agy_socket_mount_tests;
mod auth_guards;
mod chief_of_staff;
mod environment;
mod failover;
#[cfg(test)]
mod helper_tests;
mod hibernation;
mod install;
mod launch;
mod lifecycle_ops;
mod office_masks;
mod operator_home;
mod reap;
mod registry_guards;
mod rpc;
mod rust_toolchain;
mod sandbox_paths;
mod sandbox_rules;
mod sandbox_runner;
mod sessions_api;
mod spec;
mod turn_controls;
mod views;
mod warm;
mod worktree_refresh;

pub(crate) use environment::task_process_context;
#[cfg(test)]
pub(crate) use environment::SessionEnvInput;
#[cfg(test)]
pub(crate) use failover::{failover_route_opts, requeue_contended_failover, run_failover_relaunch};
pub use hibernation::{
    OfficeRssSnapshot, OsMemorySampler, ProcessMemorySampler, ResourceLimitsConfig, SessionRssEntry,
};
#[cfg(test)]
pub(crate) use install::{install_unrecovered_pre_pump_handle, PrePumpHandleMeta};
#[cfg(test)]
pub(crate) use lifecycle_ops::{take_clearable_live_handle, ClearableHandle};
#[cfg(test)]
pub(crate) use operator_home::clean_operator_rules;
pub(crate) use reap::{reap_live_handle, recycle_stale_warm_handle_in, ReapMode};
use registry_guards::BgSpawnGuard;
pub(crate) use registry_guards::StartSlotGuard;
#[cfg(test)]
pub(crate) use rust_toolchain::{
    append_rust_toolchain_env, host_rust_dir_env_from, RustToolchainEnvInput,
};
#[cfg(test)]
pub(crate) use sandbox_runner::{
    enforce_bwrap_sibling_isolation_for_test as enforce_bwrap_sibling_isolation,
    ensure_bwrap_deny_dirs,
};
pub use sandbox_runner::{
    LocalOsSandbox, SandboxConfinementMode, SandboxPlan, SandboxPrepareContext, SandboxRunner,
    SandboxVerifyContext,
};
#[cfg(test)]
pub(crate) use sessions_api::autonomous_resume;
#[cfg(test)]
pub(crate) use spec::effective_approval_writable_roots_from;
#[cfg(test)]
pub(crate) use spec::{build_session_spec, ModelChoice};
#[cfg(test)]
pub(crate) use warm::StartReservation;
pub use worktree_refresh::{
    PreparedWorktreeNotice, TaskWorktreePreparer, WorktreeSandboxRefreshOutcome,
    MID_TURN_SANDBOX_REFRESH_HINT,
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
    /// The live `agent-os` command table builder for standing instructions; `with_runtimes` takes
    /// the static catalogue ([`crate::instructions::installed_command_catalogue`]).
    /// Without one, the instructions' command catalogue section lists no commands.
    pub command_catalogue: Option<crate::instructions::CommandCatalogue>,
    /// The account's `agent-os` external sign-in state folders (workspace keys, fast tokens):
    /// every managed sandbox denies them and a Linux `bwrap` launch masks them (`TASK-137`).
    /// `with_runtimes` resolves them from `HOME` ([`sandbox_rules::agent_os_state_roots_from`]);
    /// this crate's unit tests start with none, so no test writes under the real account.
    pub agent_os_state_roots: Vec<PathBuf>,
    /// Optional consumer-owned attachment payload resolver (`TASK-98.3`). When `None`, defaults to
    /// [`crate::turn_attachments_adapter::LocalOfficeAttachmentResolver`]; satellites wire a
    /// federation-backed resolver in `org-office-server`.
    pub attachment_resolver: Option<Arc<dyn crate::turn_attachments::TurnAttachmentResolver>>,
    /// Per-office idle hibernation TTL, resident RSS ceiling, and child build concurrency (`TASK-133`).
    pub resource_limits: ResourceLimitsConfig,
}

impl SessionManagerConfig {
    /// Builds a production configuration with explicitly injected runtime adapters (`TASK-255`).
    pub fn with_runtimes(office_socket: PathBuf, runtimes: RuntimeRegistry) -> Self {
        Self {
            office_socket,
            helper_dir: None,
            stall_timeout: DEFAULT_STALL_TIMEOUT,
            enrollment_timeout: crate::credentials::DEFAULT_ENROLLMENT_TIMEOUT,
            hourly_wake_limit: DEFAULT_HOURLY_WAKE_LIMIT,
            wake_window: DEFAULT_WAKE_WINDOW,
            suspend_grace: DEFAULT_SUSPEND_GRACE,
            mid_turn_limits: MidTurnLimits::default(),
            runtimes,
            enrollment_driver: Arc::new(CliEnrollmentDriver),
            subscription_usage_collector: Arc::new(ProviderSubscriptionUsageCollector::production()),
            hq_permits: None,
            command_catalogue: crate::instructions::installed_command_catalogue(),
            agent_os_state_roots: if cfg!(test) {
                Vec::new()
            } else {
                sandbox_rules::agent_os_state_roots_from(
                    &|name| std::env::var_os(name),
                    cfg!(debug_assertions),
                )
            },
            attachment_resolver: None,
            resource_limits: ResourceLimitsConfig::from_env(),
        }
    }

    #[cfg(test)]
    pub fn production(office_socket: PathBuf) -> Self {
        Self::with_runtimes(office_socket, RuntimeRegistry::production())
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
    /// Opaque confinement plan produced by [`SandboxRunner::prepare_launch`] (`TASK-233`).
    pub sandbox_plan: SandboxPlan,
    /// The Chief of Staff partition this process serves, and so the caller its run acts for
    /// (`TASK-80.1`); `None` for every other agent.
    pub partition: Option<crate::partition::PartitionKey>,
}

#[derive(Default)]
pub(crate) struct SessionRegistry {
    pub live: HashMap<String, LiveSessionHandle>,
    pub starting: HashSet<String>,
    pub draining: bool,
    pub shutting_down: bool,
    pub bg_spawning: usize,
    pub bg_workers: Vec<JoinHandle<()>>,
    /// Per-employee count of accepted explicit `stop_session` calls (pause, offboard, owner stop).
    /// A run records its employee's epoch at install ([`crate::pump::PumpContext::stop_epoch`]);
    /// a pool failover whose epoch is behind was explicitly stopped and is abandoned rather than
    /// relaunched or requeued. Bounded by the number of employees ever stopped.
    pub stop_epochs: HashMap<String, u64>,
}

impl SessionRegistry {
    /// The current explicit-stop epoch of `employee_id` (`0` until its first explicit stop).
    pub(crate) fn stop_epoch(&self, employee_id: &str) -> u64 {
        self.stop_epochs.get(employee_id).copied().unwrap_or(0)
    }

    /// Records an accepted explicit stop of `employee_id`, so any failover of a run installed
    /// before it is abandoned.
    pub(crate) fn record_explicit_stop(&mut self, employee_id: &str) {
        let epoch = self.stop_epochs.entry(employee_id.to_owned()).or_insert(0);
        *epoch = epoch.saturating_add(1);
    }

    /// Whether `employee_id` has a live *managed* turn (starting, running or awaiting approval,
    /// or an unmanaged socket binding) while the caller already holds `registry`.
    pub(crate) fn has_active_managed_turn(
        &self,
        bindings: &RunBindingRegistry,
        employee_id: &str,
    ) -> bool {
        if self.starting.contains(employee_id) {
            return true;
        }
        if let Some(handle) = self.live.get(employee_id) {
            let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            return matches!(
                state.state,
                org_proto::sessions::SessionState::Starting
                    | org_proto::sessions::SessionState::Running
                    | org_proto::sessions::SessionState::WaitingApproval
            );
        }
        // Only treat an unmanaged socket binding (no SessionManager pump) as active (`High #1`).
        bindings.has_active(employee_id)
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
    runtime: Arc<dyn org_agent_runtime::AgentRuntime>,
    supports_mid_turn: bool,
    model_supports_vision: bool,
    allow_host_auth: bool,
    sandbox_plan: SandboxPlan,
    connection_id: Option<String>,
    route_digest: String,
    oauth_expires_at_ms: Option<u64>,
    oauth_token_fp: Option<String>,
    oauth_refresh_attempted: bool,
    workspace_arg: Option<String>,
    permit: Option<(String, u64)>,
    task_ref: Option<String>,
    tried_connections: Vec<String>,
    start_stop_epoch: u64,
    /// The Chief of Staff partition the run serves; its caller is the one the run acts for.
    partition: Option<crate::partition::PartitionKey>,
    home: PathBuf,
    workspace_real: PathBuf,
    runtime_dir: PathBuf,
    attachments_root: PathBuf,
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
    runtime: Arc<dyn org_agent_runtime::AgentRuntime>,
    supports_mid_turn: bool,
    model_supports_vision: bool,
    allow_host_auth: bool,
    sandbox_plan: SandboxPlan,
    connection_id: Option<String>,
    route_digest: String,
    oauth_expires_at_ms: Option<u64>,
    oauth_token_fp: Option<String>,
    oauth_refresh_attempted: bool,
    workspace_arg: Option<String>,
    permit: Option<(String, u64)>,
    task_ref: Option<String>,
    tried_connections: Vec<String>,
    /// The explicit-stop epoch when the start slot was reserved; registration refuses the
    /// handle when a stop was accepted or refused in between (`TASK-190`).
    start_stop_epoch: u64,
    /// The Chief of Staff partition the run serves (`TASK-80.1`).
    partition: Option<crate::partition::PartitionKey>,
    home: PathBuf,
    attachments_root: PathBuf,
    tmpdir: PathBuf,
    binding_file: PathBuf,
    preset_cfg: PresetConfig,
    spec_hash: String,
    objective: String,
    attachment_ids: Vec<String>,
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
    pub(crate) sandbox_runner: Arc<dyn SandboxRunner>,
    pub(crate) attachment_resolver: Arc<dyn crate::turn_attachments::TurnAttachmentResolver>,
    pub(crate) store: SessionStore,
    pub(crate) bindings: RunBindingRegistry,
    /// The Chief of Staff's per-caller partitions (`TASK-80.1`).
    pub(crate) partitions: crate::partition::PartitionLedger,
    /// External (BYOA) pending logins, nonces and the lease cache (`TASK-137`).
    pub(crate) external: crate::external::ExternalAuthState,
    /// Every run's post-redaction coarse live activity (`TASK-103`).
    pub(crate) stream_hub: crate::stream_hub::SessionStreamHub,
    enrollment: EnrollmentManager,
    pub(crate) vault_oauth_refresh: crate::host_auth::VaultRefreshCoordinator,
    pub(crate) registry: Arc<Mutex<SessionRegistry>>,
    pub(crate) last_states: Arc<Mutex<HashMap<String, SessionView>>>,
    pub(crate) open_required_asks: Arc<Mutex<HashMap<String, Vec<SuspendedAskEntry>>>>,
    pub(crate) budget_hold: Arc<Mutex<Option<String>>>,
    pub(crate) employee_locks: Arc<crate::employee_locks::EmployeeLockMap>,
    pub(crate) dispatching_wakes: Arc<Mutex<HashSet<String>>>,
    pub(crate) waiting_messages: Arc<Mutex<HashMap<String, Arc<AtomicU32>>>>,
    pub(crate) mid_turn_rates: Arc<Mutex<HashMap<String, MidTurnRateTracker>>>,
    pub(crate) hibernation: hibernation::HibernationState,
    pub(crate) task_worktree_preparer: Mutex<Option<Arc<dyn TaskWorktreePreparer>>>,
    watch_notice_provider: Mutex<Option<WatchNoticeProvider>>,
    startup_recovery: Mutex<RecoveryReport>,
    command_cache: Mutex<HashMap<(String, String, String), CachedRpcResponse>>,
    self_ref: Mutex<Option<std::sync::Weak<SessionManager>>>,
}

impl SessionManager {
    pub fn new(office: Arc<Office>, config: SessionManagerConfig) -> Self {
        Self::with_sandbox_runner(office, config, Arc::new(LocalOsSandbox))
    }

    pub fn with_sandbox_runner(
        office: Arc<Office>,
        config: SessionManagerConfig,
        sandbox_runner: Arc<dyn SandboxRunner>,
    ) -> Self {
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
        let attachment_resolver = config.attachment_resolver.clone().unwrap_or_else(|| {
            Arc::new(
                crate::turn_attachments_adapter::LocalOfficeAttachmentResolver::new(Arc::clone(
                    &office,
                )),
            )
        });
        Self {
            office,
            config,
            sandbox_runner,
            attachment_resolver,
            store,
            bindings: RunBindingRegistry::new(),
            partitions: crate::partition::PartitionLedger::new(),
            external: crate::external::ExternalAuthState::default(),
            stream_hub: crate::stream_hub::SessionStreamHub::new(),
            enrollment,
            vault_oauth_refresh: crate::host_auth::VaultRefreshCoordinator::noop(),
            registry: Arc::new(Mutex::new(SessionRegistry::default())),
            last_states: Arc::new(Mutex::new(HashMap::new())),
            open_required_asks: Arc::new(Mutex::new(HashMap::new())),
            budget_hold: Arc::new(Mutex::new(None)),
            employee_locks: Arc::new(crate::employee_locks::EmployeeLockMap::default()),
            dispatching_wakes: Arc::new(Mutex::new(HashSet::new())),
            waiting_messages: Arc::new(Mutex::new(HashMap::new())),
            mid_turn_rates: Arc::new(Mutex::new(HashMap::new())),
            hibernation: hibernation::HibernationState::default(),
            task_worktree_preparer: Mutex::new(None),
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
        Self::maybe_start_resource_sweeper(this);
    }

    pub(crate) fn weak_self(&self) -> Option<std::sync::Weak<Self>> {
        self.self_ref
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The live stream of every run's coarse activity, reply deltas and turn ends; subscribers
    /// authorize what they hand on (`TASK-103`).
    pub fn stream_hub(&self) -> &crate::stream_hub::SessionStreamHub {
        &self.stream_hub
    }

    pub fn bindings(&self) -> &RunBindingRegistry {
        &self.bindings
    }

    /// Registered runtime adapters configured on this session manager.
    pub fn runtimes(&self) -> &RuntimeRegistry {
        &self.config.runtimes
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
    /// An externally bound connection's command is kept in RAM only, on its binding (bounded):
    /// the bound hot path does no disk write and no sync (`doc-7` #18), and
    /// `external-history.jsonl` keeps the lease's lifecycle and security events alone.
    pub fn record_accepted_command(&self, employee_id: &str, method: &str) {
        let mode = self
            .bindings
            .record_accepted_command(employee_id, method, method);
        if matches!(mode, Some(crate::binding::BindingMode::External { .. })) {
            return;
        }
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
        crate::turn_attachments::purge_office_stale_attachments(
            self.office.durable_root(),
            &self.office.info().office_id,
        );
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

fn next_entropy_hex(bytes_len: usize) -> Result<String, ApiError> {
    random_hex(bytes_len)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "system entropy unavailable"))
}
