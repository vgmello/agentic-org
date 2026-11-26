//! Pending-start and active-turn records, history, and startup recovery scanning
//! (`LIVE-9..13`, `ADR-0030`).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;

use crate::redact::redact_json;

use super::{
    ensure_owner_dirs, is_safe_id, read_optional_json, rfc3339_now, write_atomic_json,
    ActiveTurnRecord, PendingStartRecord, ProcessExitOracle, RecoveryAction, RecoveryReport,
    SessionStore, MAX_RESUME_ATTEMPTS,
};

impl SessionStore {
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
        // Redact structurally, then serialize: every string (and every value under a secret
        // key) goes through the choke-point and the line always stays valid JSON.
        let redacted =
            serde_json::to_string(&redact_json(entry, active_secrets)).map_err(io::Error::other)?;
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

    pub(super) fn recover_employee(
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

    pub(super) fn mark_recovery_required(
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
