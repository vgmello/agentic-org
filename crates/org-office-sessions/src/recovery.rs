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

    pub fn usage_capture_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .employee_dir(employee_id)?
            .join("usage-capture.v1.json"))
    }

    pub fn rejected_usage_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .employee_dir(employee_id)?
            .join("rejected-usage.v1.json"))
    }

    /// Atomically stages a raw usage observation (and any older overflow observations first, in
    /// strict FIFO order) into the single durable `usage-capture.v1.json` file that holds BOTH
    /// per-run cumulative watermarks AND normalized pending payloads (`TASK-97` / Final Gate
    /// Pass 13 #1/#2/#3).
    ///
    /// Because `watermarks` and `pending` are updated in ONE atomic rename + `full_sync` +
    /// ancestor-dir sync under `self.lock`, live capture and the background wake loop can never
    /// diverge, double-count on a partial rename/sync failure, or skip an older observation.
    pub fn stage_usage_observation(
        &self,
        employee_id: &str,
        entry: DeferredUsageEntry,
    ) -> io::Result<()> {
        let mut overflow = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let queue = overflow.entry(employee_id.to_owned()).or_default();
        if !queue
            .iter()
            .any(|existing| existing.run_id == entry.run_id && existing.seq == entry.seq)
        {
            queue.push_back(entry);
        }
        self.flush_overflow_locked(employee_id, &mut overflow)
    }

    /// Attempts to stage any in-memory overflow entries for `employee_id` in strict FIFO order
    /// into `usage-capture.v1.json`, stopping immediately on any persistence error.
    pub fn flush_overflow_usage(&self, employee_id: &str) -> io::Result<()> {
        let mut overflow = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        self.flush_overflow_locked(employee_id, &mut overflow)
    }

    /// Returns the number of unpersisted usage observations still buffered in memory for
    /// `employee_id` (non-zero only when disk staging failed and has not yet recovered).
    pub fn pending_usage_overflow_count(&self, employee_id: &str) -> usize {
        self.lock
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .map_or(0, std::collections::VecDeque::len)
    }

    fn flush_overflow_locked(
        &self,
        employee_id: &str,
        overflow: &mut std::collections::BTreeMap<
            String,
            std::collections::VecDeque<DeferredUsageEntry>,
        >,
    ) -> io::Result<()> {
        let Some(queue) = overflow.get_mut(employee_id) else {
            return Ok(());
        };
        if queue.is_empty() {
            overflow.remove(employee_id);
            return Ok(());
        }
        let path = self.usage_capture_path(employee_id)?;
        let dir = self.employee_dir(employee_id)?;
        let mut state: UsageCaptureState = read_optional_json(&path)?.unwrap_or_default();
        while let Some(front) = queue.front() {
            let existing_max = state.watermarks.get(&front.run_id).map_or(0, |w| w.max_seq);
            if front.seq <= existing_max {
                // Rename may have succeeded on a prior attempt before `sync_dir` failed: ensure
                // ancestor dirs and file are durably synced before popping from overflow.
                ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
                if path.is_file() {
                    let f = fs::File::open(&path)?;
                    org_platform::fs::full_sync(&f)?;
                }
                queue.pop_front();
                continue;
            }
            let next_order = state
                .watermarks
                .values()
                .map(|w| w.order)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let mut next_state = state.clone();
            let wm = next_state
                .watermarks
                .entry(front.run_id.clone())
                .or_default();
            let params = normalize_usage_observation(employee_id, front, wm);
            wm.max_seq = wm.max_seq.max(front.seq);
            wm.order = next_order;
            if !next_state.pending.iter().any(|p| {
                p.producer_id == params.producer_id && p.source_event_id == params.source_event_id
            }) {
                next_state.pending.push(params);
            }
            ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
            write_atomic_json_durable(&path, &next_state)?;
            state = next_state;
            queue.pop_front();
        }
        if queue.is_empty() {
            overflow.remove(employee_id);
        }
        Ok(())
    }

    pub fn read_pending_usage(
        &self,
        employee_id: &str,
    ) -> io::Result<Vec<org_proto::costs::CostUsageRecordParams>> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let state: UsageCaptureState =
            read_optional_json(&self.usage_capture_path(employee_id)?)?.unwrap_or_default();
        Ok(state.pending)
    }

    pub fn remove_pending_usage_by_keys(
        &self,
        employee_id: &str,
        acked: &[(String, String)],
    ) -> io::Result<()> {
        if acked.is_empty() {
            return Ok(());
        }
        let overflow = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.usage_capture_path(employee_id)?;
        let Some(mut state): Option<UsageCaptureState> = read_optional_json(&path)? else {
            return Ok(());
        };
        state.pending.retain(|item| {
            !acked
                .iter()
                .any(|(p, e)| p == &item.producer_id && e == &item.source_event_id)
        });
        let mut protected_runs = self.live_and_queued_runs_locked(employee_id, &state.pending)?;
        if let Some(q) = overflow.get(employee_id) {
            for item in q {
                protected_runs.insert(item.run_id.clone());
            }
        }
        prune_watermarks(&mut state.watermarks, &protected_runs);
        let dir = self.employee_dir(employee_id)?;
        ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
        write_atomic_json_durable(&path, &state)
    }

    fn live_and_queued_runs_locked(
        &self,
        employee_id: &str,
        pending_remaining: &[org_proto::costs::CostUsageRecordParams],
    ) -> io::Result<std::collections::BTreeSet<String>> {
        let mut protected = std::collections::BTreeSet::new();
        if let Ok(Some(active)) =
            read_optional_json::<ActiveTurnRecord>(&self.active_turn_path(employee_id)?)
        {
            protected.insert(active.run_id);
        }
        if let Ok(Some(pending_start)) =
            read_optional_json::<PendingStartRecord>(&self.pending_start_path(employee_id)?)
        {
            protected.insert(pending_start.run_id);
        }
        for item in pending_remaining {
            if let Some(run_id) = item.run_id.as_deref() {
                protected.insert(run_id.to_owned());
            }
        }
        Ok(protected)
    }

    pub fn append_rejected_usage(
        &self,
        employee_id: &str,
        entry: &RejectedUsageEntry,
    ) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.rejected_usage_path(employee_id)?;
        let mut items: Vec<RejectedUsageEntry> = read_optional_json(&path)?.unwrap_or_default();
        if !items.iter().any(|existing| {
            existing.params.producer_id == entry.params.producer_id
                && existing.params.source_event_id == entry.params.source_event_id
        }) {
            items.push(entry.clone());
        }
        let dir = self.employee_dir(employee_id)?;
        ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
        write_atomic_json_durable(&path, &items)
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

    /// Lists all employee IDs that currently have a session folder on disk or buffered usage.
    pub fn list_employee_ids(&self) -> io::Result<Vec<String>> {
        let mut ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        if self.sessions_root.exists() {
            for entry in fs::read_dir(&self.sessions_root)? {
                let Ok(entry) = entry else { continue };
                if !entry.path().is_dir() {
                    continue;
                }
                let file_name = entry.file_name();
                let Some(name) = file_name.to_str() else {
                    continue;
                };
                if is_safe_id(name) {
                    ids.insert(name.to_owned());
                }
            }
        }
        let overflow = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        ids.extend(overflow.keys().filter(|id| is_safe_id(id)).cloned());
        Ok(ids.into_iter().collect())
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

    /// Atomically upserts `active-turn.v1.json` with `active = false` and `recovery_required = true`
    /// under the store lock so an unverified teardown cannot disappear across restarts even when
    /// no active-turn record was previously written (`AE-07`, `TASK-166`).
    pub fn persist_recovery_required(
        &self,
        employee_id: &str,
        run_id: Option<&str>,
        session_id: Option<&str>,
        runtime_session_id: Option<&str>,
    ) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.employee_dir(employee_id)?;
        ensure_owner_dirs(&dir)?;
        let path = self.active_turn_path(employee_id)?;
        let record = match read_optional_json::<ActiveTurnRecord>(&path)? {
            Some(mut rec) => {
                rec.active = false;
                rec.recovery_required = true;
                rec.updated_at = rfc3339_now();
                rec
            }
            None => ActiveTurnRecord {
                version: 1,
                employee_id: employee_id.to_owned(),
                run_id: run_id.unwrap_or_default().to_owned(),
                session_id: session_id.unwrap_or_default().to_owned(),
                runtime_session_id: runtime_session_id.map(str::to_owned),
                mark: String::new(),
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
        write_atomic_json(&path, &record)
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

/// Atomic single-file durable state for session usage capture (`usage-capture.v1.json`) pairing
/// per-run cumulative watermarks with the normalized pending submission queue (`Pass 13`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageCaptureState {
    #[serde(default)]
    pub watermarks: std::collections::BTreeMap<String, UsageWatermarkRecord>,
    #[serde(default)]
    pub pending: Vec<org_proto::costs::CostUsageRecordParams>,
}

/// Persisted per-run cumulative watermark stored atomically alongside pending usage records.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageWatermarkRecord {
    #[serde(default)]
    pub tokens: [u64; 5],
    #[serde(default)]
    pub cost_micro_usd: u64,
    #[serde(default)]
    pub max_seq: u64,
    #[serde(default)]
    pub order: u64,
}

/// Raw usage observation with receipt timestamp (`occurred_at`) captured once before staging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredUsageEntry {
    pub run_id: String,
    pub seq: u64,
    pub occurred_at: String,
    pub usage: org_agent_runtime::Usage,
}

/// Computes normalized incremental usage deltas from `wm`, advances `wm` in place, and builds
/// the immutable `CostUsageRecordParams` carrying the original receipt `occurred_at`.
fn normalize_usage_observation(
    emp_id: &str,
    obs: &DeferredUsageEntry,
    wm: &mut UsageWatermarkRecord,
) -> org_proto::costs::CostUsageRecordParams {
    use org_agent_runtime::{CostBasis, UsageScope};
    use org_proto::costs::{
        AttributionBasis, CostUsageRecordParams, UsageActorKind, UsageScopeKind,
    };
    let usage = &obs.usage;
    let (in_tok, out_tok, cr_tok, cw_tok, reas_tok) = match usage.token_scope {
        UsageScope::Cumulative => {
            let d = (
                usage.input.saturating_sub(wm.tokens[0]),
                usage.output.saturating_sub(wm.tokens[1]),
                usage.cache_read.saturating_sub(wm.tokens[2]),
                usage.cache_write.saturating_sub(wm.tokens[3]),
                usage.reasoning.saturating_sub(wm.tokens[4]),
            );
            wm.tokens[0] = wm.tokens[0].max(usage.input);
            wm.tokens[1] = wm.tokens[1].max(usage.output);
            wm.tokens[2] = wm.tokens[2].max(usage.cache_read);
            wm.tokens[3] = wm.tokens[3].max(usage.cache_write);
            wm.tokens[4] = wm.tokens[4].max(usage.reasoning);
            d
        }
        _ => (
            usage.input,
            usage.output,
            usage.cache_read,
            usage.cache_write,
            usage.reasoning,
        ),
    };
    let raw_cost = match usage.cost_basis {
        CostBasis::ListPriceEstimate => usage
            .cost_usd
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| (v * 1_000_000.0).round() as u64),
        _ => None,
    };
    let runtime_cost_micro_usd = match (raw_cost, usage.cost_scope) {
        (Some(total), UsageScope::Cumulative) => {
            let d = total.saturating_sub(wm.cost_micro_usd);
            wm.cost_micro_usd = wm.cost_micro_usd.max(total);
            Some(d)
        }
        (Some(delta), _) => Some(delta),
        (None, _) => None,
    };
    let meta = serde_json::json!({
        "token_scope": format!("{:?}", usage.token_scope),
        "cost_scope": format!("{:?}", usage.cost_scope),
        "cost_basis": format!("{:?}", usage.cost_basis),
    })
    .to_string();
    CostUsageRecordParams {
        id: None,
        producer_id: format!("session-{emp_id}"),
        source_event_id: format!("{}:{}", obs.run_id, obs.seq),
        run_id: Some(obs.run_id.clone()),
        actor_kind: UsageActorKind::Agent,
        employee_id: Some(emp_id.to_owned()),
        runtime: "agent_session".into(),
        provider: None,
        model: usage.model.clone(),
        occurred_at: obs.occurred_at.clone(),
        source_metadata_json: Some(meta),
        input_tokens: Some(in_tok),
        output_tokens: Some(out_tok),
        cache_read_tokens: Some(cr_tok),
        cache_write_tokens: Some(cw_tok),
        reasoning_tokens: Some(reas_tok),
        token_issue: None,
        runtime_cost_micro_usd,
        is_subscription: false,
        // `doc-87` / `doc-51` COST-3: unattributed session usage is NEVER defaulted to home team.
        scope_kind: UsageScopeKind::Organization,
        task_id: None,
        project_id: None,
        team_id: None,
        feature_id: None,
        milestone_id: None,
        release_id: None,
        task_type_snapshot: None,
        work_type: None,
        attribution_basis: AttributionBasis::Unattributed,
    }
}

fn prune_watermarks(
    watermarks: &mut std::collections::BTreeMap<String, UsageWatermarkRecord>,
    protected_runs: &std::collections::BTreeSet<String>,
) {
    while watermarks.len() > 64 {
        let oldest = watermarks
            .iter()
            .filter(|(k, _)| !protected_runs.contains(*k))
            .min_by_key(|(_, v)| v.order)
            .map(|(k, _)| k.clone());
        if let Some(evict) = oldest {
            watermarks.remove(&evict);
        } else {
            break;
        }
    }
}

/// Dead-letter record for a usage event permanently rejected by accounting validation so later
/// valid events continue draining without loss of diagnostic evidence (`TASK-97` / Pass 8 #2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedUsageEntry {
    pub params: org_proto::costs::CostUsageRecordParams,
    pub error_code: String,
    pub error_message: String,
    pub rejected_at: String,
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

fn write_atomic_json_durable<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
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
    if let Err(err) = file
        .write_all(&bytes)
        .and_then(|()| org_platform::fs::full_sync(&file))
    {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    drop(file);
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    sync_dir(parent)
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
