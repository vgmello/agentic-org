//! External lease files inside an agent's existing session folder (`TASK-137`):
//! `external-lease.v1.json` (0600, atomic, fully synced) and `external-history.jsonl` (0600,
//! append-only, secret-redacted like `history.jsonl`). No new office-level tree: the folder is
//! already excluded from store rebuilds and sandbox-denied to managed agents.

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use org_proto::external_auth::ExternalHistoryEntry;

use crate::external::ExternalLeaseRecord;
use crate::redact::redact_json;

use super::{ensure_owner_dirs_synced, write_atomic_json_durable, SessionStore};

/// Largest lease file read (it holds a handful of short fields).
const MAX_LEASE_BYTES: u64 = 16 * 1024;
/// Most bytes read from the end of `external-history.jsonl` for `agent sessions`.
const MAX_HISTORY_TAIL_BYTES: u64 = 256 * 1024;

impl SessionStore {
    pub fn external_lease_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .employee_dir(employee_id)?
            .join("external-lease.v1.json"))
    }

    pub fn external_history_path(&self, employee_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .employee_dir(employee_id)?
            .join("external-history.jsonl"))
    }

    /// Reads `employee_id`'s lease, refusing a symbolic link, a non-file and an oversized file.
    pub fn read_external_lease(
        &self,
        employee_id: &str,
    ) -> io::Result<Option<ExternalLeaseRecord>> {
        let path = self.external_lease_path(employee_id)?;
        let file = match org_platform::fs::open_nofollow_ro(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut bytes = Vec::new();
        file.take(MAX_LEASE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_LEASE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "external lease file is too large",
            ));
        }
        let record: ExternalLeaseRecord = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if record.employee_id != employee_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "external lease names another employee",
            ));
        }
        Ok(Some(record))
    }

    /// Atomically and durably replaces `record.employee_id`'s lease (0600, fsync file and folder).
    pub fn write_external_lease(&self, record: &ExternalLeaseRecord) -> io::Result<()> {
        let dir = self.employee_dir(&record.employee_id)?;
        ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
        write_atomic_json_durable(&self.external_lease_path(&record.employee_id)?, record)
    }

    /// Appends one redacted JSON line to `external-history.jsonl` (0600, never through a link).
    pub fn append_external_history(
        &self,
        employee_id: &str,
        entry: &serde_json::Value,
        active_secrets: &[&str],
    ) -> io::Result<()> {
        let dir = self.employee_dir(employee_id)?;
        ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
        let redacted =
            serde_json::to_string(&redact_json(entry, active_secrets)).map_err(io::Error::other)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(
                org_platform::fs::O_NOFOLLOW
                    | org_platform::fs::O_NONBLOCK
                    | org_platform::fs::O_CLOEXEC,
            )
            .open(self.external_history_path(employee_id)?)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "external history is not a regular file",
            ));
        }
        writeln!(file, "{redacted}")?;
        file.flush()
    }

    /// The last `limit` well-formed history entries, oldest first (bounded tail read).
    pub fn read_external_history(
        &self,
        employee_id: &str,
        limit: usize,
    ) -> io::Result<Vec<ExternalHistoryEntry>> {
        let path = self.external_history_path(employee_id)?;
        let mut file = match org_platform::fs::open_nofollow_ro(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let len = file.metadata()?.len();
        let start = len.saturating_sub(MAX_HISTORY_TAIL_BYTES);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(MAX_HISTORY_TAIL_BYTES).read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        let mut lines: Vec<&str> = text.lines().collect();
        if start > 0 && !lines.is_empty() {
            // The first line may be cut in the middle.
            lines.remove(0);
        }
        let mut entries: Vec<ExternalHistoryEntry> = lines
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        if entries.len() > limit {
            entries.drain(..entries.len() - limit);
        }
        Ok(entries)
    }
}
