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

use crate::credentials::{is_safe_id, redact_secrets, rfc3339_now};

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
    lock: Arc<Mutex<()>>,
}

impl SessionStore {
    pub fn new(durable_root: &Path, office_id: &str) -> Self {
        Self {
            sessions_root: durable_root
                .join("local")
                .join("offices")
                .join(office_id)
                .join("sessions"),
            lock: Arc::new(Mutex::new(())),
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

    pub fn pending_start_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .employee_dir(employee_id)?
            .join("pending-start.v1.json"))
    }

    pub fn active_turn_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self.employee_dir(employee_id)?.join("active-turn.v1.json"))
    }

    pub fn history_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self.employee_dir(employee_id)?.join("history.jsonl"))
    }

    pub fn pending_wakes_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .employee_dir(employee_id)?
            .join("pending-wakes.v1.json"))
    }

    pub fn suspended_state_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .employee_dir(employee_id)?
            .join("suspended-asks.v1.json"))
    }

    pub fn wake_quota_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self.employee_dir(employee_id)?.join("wake-quota.v1.json"))
    }

    pub fn read_pending_wakes(
        &self,
        employee_id: &str,
    ) -> io::Result<Option<crate::wake::PendingWakeQueueRecord>> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        read_optional_json(&self.pending_wakes_path(employee_id)?)
    }

    pub fn write_pending_wakes(
        &self,
        record: &crate::wake::PendingWakeQueueRecord,
    ) -> io::Result<()> {
        self.write_pending_wakes_redacted(record, &[])
    }

    pub fn write_pending_wakes_redacted(
        &self,
        record: &crate::wake::PendingWakeQueueRecord,
        active_secrets: &[&str],
    ) -> io::Result<()> {
        let mut clean = record.clone();
        for item in &mut clean.items {
            item.summary = redact_secrets(&item.summary, active_secrets);
        }
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.employee_dir(&clean.employee_id)?;
        ensure_owner_dirs(&dir)?;
        write_atomic_json(&self.pending_wakes_path(&clean.employee_id)?, &clean)
    }

    pub fn clear_pending_wakes(&self, employee_id: &str) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.pending_wakes_path(employee_id)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn read_suspended_state(
        &self,
        employee_id: &str,
    ) -> io::Result<Option<crate::wake::SuspendedStateRecord>> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        read_optional_json(&self.suspended_state_path(employee_id)?)
    }

    pub fn write_suspended_state(
        &self,
        record: &crate::wake::SuspendedStateRecord,
    ) -> io::Result<()> {
        self.write_suspended_state_redacted(record, &[])
    }

    pub fn write_suspended_state_redacted(
        &self,
        record: &crate::wake::SuspendedStateRecord,
        active_secrets: &[&str],
    ) -> io::Result<()> {
        let mut clean = record.clone();
        for ask in &mut clean.asks {
            ask.question_summary = redact_secrets(&ask.question_summary, active_secrets);
        }
        for ans in &mut clean.pending_resume_answers {
            ans.question_summary = redact_secrets(&ans.question_summary, active_secrets);
            ans.answer_summary = redact_secrets(&ans.answer_summary, active_secrets);
        }
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.employee_dir(&clean.employee_id)?;
        ensure_owner_dirs(&dir)?;
        write_atomic_json(&self.suspended_state_path(&clean.employee_id)?, &clean)
    }

    pub fn clear_suspended_state(&self, employee_id: &str) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.suspended_state_path(employee_id)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn read_wake_quota(
        &self,
        employee_id: &str,
    ) -> io::Result<Option<crate::wake::WakeQuotaRecord>> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        read_optional_json(&self.wake_quota_path(employee_id)?)
    }

    pub fn write_wake_quota(&self, record: &crate::wake::WakeQuotaRecord) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.employee_dir(&record.employee_id)?;
        ensure_owner_dirs(&dir)?;
        write_atomic_json(&self.wake_quota_path(&record.employee_id)?, record)
    }

    /// Lists all employee IDs that currently have a session folder on disk.
    pub fn list_employee_ids(&self) -> io::Result<Vec<String>> {
        if !self.sessions_root.exists() {
            return Ok(Vec::new());
        }
        let mut dirs: Vec<String> = fs::read_dir(&self.sessions_root)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .filter(|name| is_safe_id(name))
            .collect();
        dirs.sort();
        Ok(dirs)
    }

    pub fn write_pending_start(&self, record: &PendingStartRecord) -> io::Result<()> {
        let dir = self.employee_dir(&record.employee_id)?;
        ensure_owner_dirs(&dir)?;
        write_atomic_json(&self.pending_start_path(&record.employee_id)?, record)
    }

    pub fn clear_pending_start(&self, employee_id: &str) -> io::Result<()> {
        let path = self.pending_start_path(employee_id)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn read_pending_start(&self, employee_id: &str) -> io::Result<Option<PendingStartRecord>> {
        read_optional_json(&self.pending_start_path(employee_id)?)
    }

    pub fn write_active_turn(&self, record: &ActiveTurnRecord) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.employee_dir(&record.employee_id)?;
        ensure_owner_dirs(&dir)?;
        write_atomic_json(&self.active_turn_path(&record.employee_id)?, record)
    }

    /// Atomically reads, mutates, and rewrites `active-turn.v1.json` under the store lock so
    /// concurrent socket/RPC/pump updates cannot clobber one another.
    pub fn update_active_turn(
        &self,
        employee_id: &str,
        mutate: impl FnOnce(&mut ActiveTurnRecord),
    ) -> io::Result<bool> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.active_turn_path(employee_id)?;
        let Some(mut record) = read_optional_json::<ActiveTurnRecord>(&path)? else {
            return Ok(false);
        };
        mutate(&mut record);
        write_atomic_json(&path, &record)?;
        Ok(true)
    }

    pub fn read_active_turn(&self, employee_id: &str) -> io::Result<Option<ActiveTurnRecord>> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        read_optional_json(&self.active_turn_path(employee_id)?)
    }

    /// Appends one redacted JSON line to `history.jsonl` with mode `0600` (`ENROLL-3`).
    pub fn append_history(
        &self,
        employee_id: &str,
        entry: &serde_json::Value,
        active_secrets: &[&str],
    ) -> io::Result<()> {
        let dir = self.employee_dir(employee_id)?;
        ensure_owner_dirs(&dir)?;
        let raw = serde_json::to_string(entry).map_err(io::Error::other)?;
        let redacted = redact_secrets(&raw, active_secrets);
        let path = self.history_path(employee_id)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)?;
        writeln!(file, "{redacted}")?;
        file.flush()
    }

    /// Scans all employee session folders on startup and decides per-employee recovery actions
    /// (`ADR-0030`, `LIVE-9..11`). Fails closed into `RecoveryRequired` whenever prior process exit
    /// cannot be proven or resume attempts reach [`MAX_RESUME_ATTEMPTS`].
    pub fn recover_on_startup(&self, oracle: &dyn ProcessExitOracle) -> io::Result<RecoveryReport> {
        let mut report = RecoveryReport::default();
        if !self.sessions_root.exists() {
            return Ok(report);
        }
        let mut dirs: Vec<String> = fs::read_dir(&self.sessions_root)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .filter(|name| is_safe_id(name))
            .collect();
        dirs.sort();
        for employee_id in dirs {
            let action = self.recover_employee(&employee_id, oracle)?;
            if action != RecoveryAction::Clean {
                report.actions.push(action);
            }
        }
        Ok(report)
    }

    fn recover_employee(
        &self,
        employee_id: &str,
        oracle: &dyn ProcessExitOracle,
    ) -> io::Result<RecoveryAction> {
        let pending = self.read_pending_start(employee_id)?;
        let active = self.read_active_turn(employee_id)?;

        if let Some(pending) = pending {
            let gone = oracle
                .prove_exited(&pending.mark, None, None, None)
                .unwrap_or(false);
            if !gone {
                return self.mark_recovery_required(
                    employee_id,
                    active,
                    Some(&pending),
                    "prior pending-start process could not be proven exited",
                );
            }
            self.clear_pending_start(employee_id)?;
        }

        let Some(mut turn) = active else {
            return Ok(RecoveryAction::Clean);
        };
        if turn.stall_locked || turn.recovery_required {
            return Ok(RecoveryAction::RecoveryRequired {
                employee_id: employee_id.to_owned(),
                reason: "session requires manual recovery or stall clearance".to_owned(),
            });
        }
        if !turn.active {
            return Ok(RecoveryAction::Clean);
        }
        let gone = oracle
            .prove_exited(
                &turn.mark,
                turn.pid,
                turn.started_identity.as_deref(),
                turn.boot_identity.as_deref(),
            )
            .unwrap_or(false);
        if !gone {
            return self.mark_recovery_required(
                employee_id,
                Some(turn),
                None,
                "prior active-turn process is still running or could not be verified exited",
            );
        }
        if turn.resume_attempts >= MAX_RESUME_ATTEMPTS {
            return self.mark_recovery_required(
                employee_id,
                Some(turn),
                None,
                "maximum automatic restart resume attempts (3) exhausted",
            );
        }
        turn.resume_attempts += 1;
        let attempt = turn.resume_attempts;
        self.write_active_turn(&turn)?;
        Ok(RecoveryAction::Resume {
            employee_id: employee_id.to_owned(),
            runtime_session_id: turn.runtime_session_id,
            previous_objective: turn.objective,
            accepted_commands: turn.accepted_commands,
            attempt,
        })
    }

    fn mark_recovery_required(
        &self,
        employee_id: &str,
        existing: Option<ActiveTurnRecord>,
        pending: Option<&PendingStartRecord>,
        reason: &str,
    ) -> io::Result<RecoveryAction> {
        let record = match existing {
            Some(mut turn) => {
                turn.active = false;
                turn.recovery_required = true;
                turn.updated_at = rfc3339_now();
                turn
            }
            None => ActiveTurnRecord {
                version: 1,
                employee_id: employee_id.to_owned(),
                run_id: pending.map_or_else(String::new, |p| p.run_id.clone()),
                session_id: pending.map_or_else(String::new, |p| p.session_id.clone()),
                runtime_session_id: None,
                mark: pending.map_or_else(String::new, |p| p.mark.clone()),
                pid: None,
                pgid: None,
                started_identity: None,
                boot_identity: None,
                objective: String::new(),
                accepted_commands: Vec::new(),
                resume_attempts: 0,
                consecutive_stalls: 0,
                stall_locked: false,
                recovery_required: true,
                active: false,
                updated_at: rfc3339_now(),
            },
        };
        self.write_active_turn(&record)?;
        Ok(RecoveryAction::RecoveryRequired {
            employee_id: employee_id.to_owned(),
            reason: reason.to_owned(),
        })
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

fn ensure_owner_dirs(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
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

fn write_atomic_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
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
    if let Err(err) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    drop(file);
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
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
