//! Durable office-local session state, restart continuation, and stall watchdog (`ADR-0030`,
//! `LIVE-9..13`, `FC-100`).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::credentials::{is_safe_id, rfc3339_now};

mod external_store;
mod startup;
mod usage_store;
mod wake_store;

pub use usage_store::{
    DeferredUsageEntry, RejectedUsageEntry, UsageCaptureState, UsageWatermarkRecord,
};

/// Default turn stall timeout: 15 minutes (`ADR-0030` / `LIVE-12`).
pub const DEFAULT_STALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Maximum automatic restart resume attempts before entering `recovery_required` (`LIVE-11`).
pub const MAX_RESUME_ATTEMPTS: u32 = 3;
/// Consecutive stalls that lock automatic starts until an owner clears the lock (`LIVE-12`).
pub const MAX_CONSECUTIVE_STALLS: u32 = 3;

/// Marker persisted BEFORE spawning a child process (`LIVE-9`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingStartRecord {
    pub version: u32,
    pub employee_id: String,
    pub run_id: String,
    pub session_id: String,
    pub mark: String,
    pub started_at: String,
}

/// Durable active-turn record persisted under `<durable>/local/offices/<office_id>/sessions/<employee_id>/active-turn.v1.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveTurnRecord {
    pub version: u32,
    pub employee_id: String,
    pub run_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_session_id: Option<String>,
    pub mark: String,
    pub pid: Option<u32>,
    pub pgid: Option<u32>,
    pub started_identity: Option<String>,
    pub boot_identity: Option<String>,
    pub objective: String,
    /// Lightweight IDs of bound attachments for this active turn (`TASK-98.3`). Never base64.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    #[serde(default)]
    pub accepted_commands: Vec<String>,
    #[serde(default)]
    pub resume_attempts: u32,
    #[serde(default)]
    pub consecutive_stalls: u32,
    #[serde(default)]
    pub stall_locked: bool,
    #[serde(default)]
    pub recovery_required: bool,
    pub active: bool,
    pub updated_at: String,
}

/// Outcome of scanning one employee's session directory on office startup (`LIVE-10`, `LIVE-11`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Clean idle session; no restart action needed.
    Clean,
    /// Prior run processes proven exited; ready to resume with recorded `accepted_commands`.
    Resume {
        employee_id: String,
        runtime_session_id: Option<String>,
        previous_objective: String,
        accepted_commands: Vec<String>,
        attempt: u32,
    },
    /// Prior run processes could not be proven exited or resume attempts exceeded [`MAX_RESUME_ATTEMPTS`].
    RecoveryRequired { employee_id: String, reason: String },
}

/// Aggregated report returned by startup recovery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub actions: Vec<RecoveryAction>,
}

/// Abstraction over proving whether a prior run's processes are gone (real `org-supervise`
/// recovery scan or injectable unit-test oracle).
pub trait ProcessExitOracle: Send + Sync {
    /// Returns `Ok(true)` ONLY when the prior process (and any marked descendants) are proven gone.
    /// Returns `Ok(false)` if still alive or unreadable (`MarkEvidence::Unreadable`), failing closed.
    fn prove_exited(
        &self,
        mark: &str,
        pid: Option<u32>,
        started_identity: Option<&str>,
        boot: Option<&str>,
    ) -> io::Result<bool>;
}

/// Production oracle backed by `org_platform::process::RecoveryScan`.
/// Fails closed if any live process carries `mark`, matches `(pid, started_identity)`,
/// or has an unreadable environment mark (`MarkEvidence::Unreadable`).
#[derive(Debug, Default)]
pub struct NativeProcessExitOracle;

impl ProcessExitOracle for NativeProcessExitOracle {
    fn prove_exited(
        &self,
        mark: &str,
        pid: Option<u32>,
        started_identity: Option<&str>,
        boot: Option<&str>,
    ) -> io::Result<bool> {
        let mut scan = org_platform::process::RecoveryScan::new("ORG_SESSION_MARK")?;
        if let Some(expected_boot) = boot {
            if !expected_boot.is_empty() && scan.context().boot != expected_boot {
                return Ok(true);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let processes = scan.snapshot(deadline)?;
        for proc_obs in processes {
            if matches_process_identity(&proc_obs, pid, started_identity) {
                return Ok(false);
            }
            match &proc_obs.mark {
                org_platform::process::MarkEvidence::Present(value) if value == mark => {
                    return Ok(false);
                }
                org_platform::process::MarkEvidence::Unreadable => {
                    return Ok(false);
                }
                _ => {}
            }
        }
        Ok(true)
    }
}

fn matches_process_identity(
    proc_obs: &org_platform::process::ProcessObservation,
    pid: Option<u32>,
    started_identity: Option<&str>,
) -> bool {
    let Some(target_pid) = pid else {
        return false;
    };
    if proc_obs.pid != target_pid {
        return false;
    }
    match (started_identity, &proc_obs.identity) {
        (Some(expected), Some(obs_id)) => obs_id.started == expected,
        // Missing start identity on either side fails closed.
        _ => true,
    }
}

/// Manages durable office-local session files under `<durable>/local/offices/<office_id>/sessions/`.
#[derive(Debug, Clone)]
pub struct SessionStore {
    sessions_root: PathBuf,
    /// Serializes session file mutations AND holds the ordered per-employee in-memory overflow
    /// buffer used ONLY if `usage-capture.v1.json` is transiently unwritable/unreadable
    /// (`TASK-97` / Final Gate Pass 13).
    lock: Arc<
        Mutex<std::collections::BTreeMap<String, std::collections::VecDeque<DeferredUsageEntry>>>,
    >,
}

impl SessionStore {
    pub fn new(durable_root: &Path, office_id: &str) -> Self {
        Self {
            sessions_root: durable_root
                .join("local")
                .join("offices")
                .join(office_id)
                .join("sessions"),
            lock: Arc::new(Mutex::new(std::collections::BTreeMap::new())),
        }
    }

    pub fn employee_dir(&self, employee_id: &str) -> io::Result<PathBuf> {
        if !is_safe_id(employee_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid employee_id for session store",
            ));
        }
        Ok(self.sessions_root.join(employee_id))
    }

    fn employee_file(&self, employee_id: &str, name: &str) -> io::Result<PathBuf> {
        Ok(self.employee_dir(employee_id)?.join(name))
    }

    pub fn pending_start_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        self.employee_file(employee_id, "pending-start.v1.json")
    }

    pub fn active_turn_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        self.employee_file(employee_id, "active-turn.v1.json")
    }

    pub fn history_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        self.employee_file(employee_id, "history.jsonl")
    }

    pub fn pending_wakes_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        self.employee_file(employee_id, "pending-wakes.v1.json")
    }

    pub fn suspended_state_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        self.employee_file(employee_id, "suspended-asks.v1.json")
    }

    pub fn wake_quota_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        self.employee_file(employee_id, "wake-quota.v1.json")
    }

    pub fn usage_capture_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        self.employee_file(employee_id, "usage-capture.v1.json")
    }

    pub fn rejected_usage_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        self.employee_file(employee_id, "rejected-usage.v1.json")
    }
}

/// Pure, monotonic stall watchdog for an active turn (`ADR-0030` / `LIVE-12`).
#[derive(Debug, Clone)]
pub struct StallWatchdog {
    timeout: Duration,
    last_activity: Instant,
    consecutive_stalls: u32,
    stall_locked: bool,
}

impl StallWatchdog {
    pub fn new(timeout: Duration, now: Instant) -> Self {
        Self {
            timeout,
            last_activity: now,
            consecutive_stalls: 0,
            stall_locked: false,
        }
    }

    pub fn from_persisted(
        timeout: Duration,
        now: Instant,
        consecutive_stalls: u32,
        stall_locked: bool,
    ) -> Self {
        Self {
            timeout,
            last_activity: now,
            consecutive_stalls,
            stall_locked,
        }
    }

    /// Records live activity (stdout frame, tool call, approval) and resets the current timer.
    pub fn record_activity(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// Resets consecutive stalls when a turn completes normally.
    pub fn record_turn_completed(&mut self, now: Instant) {
        self.last_activity = now;
        self.consecutive_stalls = 0;
    }

    /// Checks whether the current active turn has exceeded its stall timeout.
    pub fn is_stalled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_activity) >= self.timeout
    }

    /// Records a stall timeout reap. Locks automatic starts after [`MAX_CONSECUTIVE_STALLS`] (3).
    pub fn record_stall(&mut self, now: Instant) -> bool {
        self.last_activity = now;
        self.consecutive_stalls = self.consecutive_stalls.saturating_add(1);
        if self.consecutive_stalls >= MAX_CONSECUTIVE_STALLS {
            self.stall_locked = true;
        }
        self.stall_locked
    }

    /// Owner explicitly clears the stall lock (`MemberSessionClearStall`).
    pub fn clear_stall_lock(&mut self, now: Instant) {
        self.last_activity = now;
        self.consecutive_stalls = 0;
        self.stall_locked = false;
    }

    pub fn consecutive_stalls(&self) -> u32 {
        self.consecutive_stalls
    }

    pub fn is_locked(&self) -> bool {
        self.stall_locked
    }
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    let handle = fs::File::open(dir)?;
    org_platform::fs::full_sync(&handle)
}

/// Ensures every directory from `established_root` through `target` exists with owner-only
/// permissions and durably syncs the ENTIRE directory chain on every call so retries after an
/// interrupted directory creation never leave a visible directory entry unsynced (`Pass 8 #5`).
fn ensure_owner_dirs_synced(established_root: &Path, target: &Path) -> io::Result<()> {
    ensure_owner_dirs(established_root)?;
    if let Some(root_parent) = established_root.parent() {
        if !root_parent.as_os_str().is_empty() && root_parent.is_dir() {
            sync_dir(root_parent)?;
        }
    }
    let Ok(rel) = target.strip_prefix(established_root) else {
        ensure_owner_dirs(target)?;
        return sync_dir(target);
    };
    let mut current = established_root.to_path_buf();
    for component in rel.components() {
        let parent = current.clone();
        current.push(component);
        if !current.exists() {
            match org_platform::fs::create_owner_only_dir(&current) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        sync_dir(&parent)?;
    }
    sync_dir(target)
}

fn ensure_owner_dirs(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if !current.exists() {
            match org_platform::fs::create_owner_only_dir(&current) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

fn write_atomic_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    write_atomic_json_inner(path, value, false)
}

fn write_atomic_json_durable<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    write_atomic_json_inner(path, value, true)
}

fn write_atomic_json_inner<T: Serialize + ?Sized>(
    path: &Path,
    value: &T,
    durable: bool,
) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let mut suffix = [0u8; 8];
    SystemRandom::new()
        .fill(&mut suffix)
        .map_err(|_| io::Error::other("system entropy unavailable"))?;
    let hex: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    let tmp = parent.join(format!(".session.{hex}.tmp"));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)?;
    let write_res = file.write_all(&bytes).and_then(|()| {
        if durable {
            org_platform::fs::full_sync(&file)
        } else {
            file.sync_all()
        }
    });
    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    drop(file);
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    if durable {
        sync_dir(parent)?;
    }
    Ok(())
}

fn read_optional_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => {
            let parsed = serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(parsed))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
