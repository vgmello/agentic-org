//! Durable per-employee pending-wake queue, suspended-ask state, and wake quota records.

use std::fs;
use std::io;

use crate::redact::redact_text;

use super::{ensure_owner_dirs, is_safe_id, read_optional_json, write_atomic_json, SessionStore};

impl SessionStore {
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
            item.summary = redact_text(&item.summary, active_secrets);
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
            ask.question_summary = redact_text(&ask.question_summary, active_secrets);
        }
        for ans in &mut clean.pending_resume_answers {
            ans.question_summary = redact_text(&ans.question_summary, active_secrets);
            ans.answer_summary = redact_text(&ans.answer_summary, active_secrets);
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
}
