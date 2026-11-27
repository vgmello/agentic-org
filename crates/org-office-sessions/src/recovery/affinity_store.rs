//! Bounded per-employee session affinity ledger (`session-affinity.v1.json`) for safe
//! two-stage warm/fresh session resume (`TASK-92.1`, ADR-0028).
//!
//! Records `(runtime_session_id -> runtime_id, workspace, spec_hash)` whenever a runtime
//! starts or confirms a session, so warm review rounds and explicit `--resume` requests only
//! resume when `(same_runtime && same_workspace && same_spec_hash)` still holds (guarding
//! runtimes with `resume_ignores_cwd: true` / `resume_applies_instructions: false`).

use std::fs;
use std::io;

use serde::{Deserialize, Serialize};

use crate::credentials::rfc3339_now;

use super::{
    ensure_owner_dirs_synced, read_optional_json, write_atomic_json_durable, SessionStore,
};

const MAX_AFFINITY_ENTRIES: usize = 64;

/// Recorded launch context of one runtime session ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAffinityEntry {
    pub runtime_session_id: String,
    pub runtime_id: String,
    pub workspace: String,
    pub spec_hash: String,
    pub recorded_at: String,
}

/// Durable per-employee bounded list of recorded runtime session affinities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAffinityRecord {
    pub version: u32,
    pub employee_id: String,
    #[serde(default)]
    pub entries: Vec<SessionAffinityEntry>,
    pub updated_at: String,
}

impl SessionAffinityRecord {
    fn empty(employee_id: &str) -> Self {
        Self {
            version: 1,
            employee_id: employee_id.to_owned(),
            entries: Vec::new(),
            updated_at: rfc3339_now(),
        }
    }

    fn upsert(&mut self, entry: SessionAffinityEntry) {
        self.entries
            .retain(|existing| existing.runtime_session_id != entry.runtime_session_id);
        self.entries.push(entry);
        if self.entries.len() > MAX_AFFINITY_ENTRIES {
            let drain = self.entries.len() - MAX_AFFINITY_ENTRIES;
            self.entries.drain(..drain);
        }
        self.updated_at = rfc3339_now();
    }
}

impl SessionStore {
    pub fn record_session_affinity(
        &self,
        employee_id: &str,
        entry: SessionAffinityEntry,
    ) -> io::Result<()> {
        if entry.runtime_session_id.trim().is_empty() {
            return Ok(());
        }
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.employee_dir(employee_id)?;
        ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
        let path = self.session_affinity_path(employee_id)?;
        let mut record = read_optional_json::<SessionAffinityRecord>(&path)?
            .unwrap_or_else(|| SessionAffinityRecord::empty(employee_id));
        record.upsert(entry);
        write_atomic_json_durable(&path, &record)
    }

    pub fn lookup_session_affinity(
        &self,
        employee_id: &str,
        runtime_session_id: &str,
    ) -> io::Result<Option<SessionAffinityEntry>> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.session_affinity_path(employee_id)?;
        let Some(record) = read_optional_json::<SessionAffinityRecord>(&path)? else {
            return Ok(None);
        };
        Ok(record
            .entries
            .into_iter()
            .rfind(|e| e.runtime_session_id == runtime_session_id))
    }

    pub fn clear_session_affinity(&self, employee_id: &str) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.session_affinity_path(employee_id)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}
