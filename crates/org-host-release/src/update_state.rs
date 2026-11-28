//! Durable atomic auto-update state: post-restart health-check checkpoint and failed-release
//! quarantine tombstones (ADR-0009).
//!
//! Stored in a dedicated owner-only (`0700`) state directory outside `ReleaseStore`'s root so
//! `ReleaseStore::verify_root` stays strictly closed and backward-compatible.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{Error, ReleasePin};

const STATE_FILE_NAME: &str = "update-state.v1.json";
const MAX_STATE_BYTES: u64 = 64 * 1024;
const MAX_QUARANTINE_ENTRIES: usize = 64;
const MAX_EXPECTED_INSTANCES: usize = 64;
const MAX_INSTANCE_NAME_BYTES: usize = 128;
const MAX_REASON_BYTES: usize = 512;

/// Atomic checkpoint recorded BEFORE swapping the active release pin and restarting services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingHealthCheck {
    pub previous_pin: Option<ReleasePin>,
    pub target_pin: ReleasePin,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_url: Option<String>,
    /// Managed instance names that were running before the swap began. `None` indicates a
    /// checkpoint created without an explicit list (recovery treats all managed instances in
    /// the store as required when restarting).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_running_instances: Option<Vec<String>>,
}

/// Tombstone preventing the auto-updater from infinite-looping on a release that failed health checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuarantinedRelease {
    pub release_id: String,
    pub sequence: u64,
    pub manifest_sha256: String,
    pub reason: String,
    pub quarantined_at_ms: u64,
}

/// Persisted auto-update ledger (`update-state.v1.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateState {
    pub version: u32,
    #[serde(default)]
    pub confirmed: Option<ReleasePin>,
    #[serde(default)]
    pub pending_health_check: Option<PendingHealthCheck>,
    #[serde(default)]
    pub quarantined: Vec<QuarantinedRelease>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            version: 1,
            confirmed: None,
            pending_health_check: None,
            quarantined: Vec::new(),
        }
    }
}

impl UpdateState {
    pub fn quarantined_entry(&self, release_id: &str) -> Option<&QuarantinedRelease> {
        self.quarantined
            .iter()
            .find(|entry| entry.release_id == release_id)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::InvalidRelease);
        }
        if let Some(pin) = &self.confirmed {
            pin.validate()?;
        }
        if let Some(pending) = &self.pending_health_check {
            validate_pending(pending)?;
        }
        validate_quarantined(&self.quarantined)
    }
}

fn validate_pending(pending: &PendingHealthCheck) -> Result<(), Error> {
    pending.target_pin.validate()?;
    if let Some(prev) = &pending.previous_pin {
        prev.validate()?;
    }
    if let Some(instances) = &pending.expected_running_instances {
        if instances.len() > MAX_EXPECTED_INSTANCES {
            return Err(Error::InvalidRelease);
        }
        for name in instances {
            if name.is_empty()
                || name.len() > MAX_INSTANCE_NAME_BYTES
                || name.chars().any(char::is_control)
            {
                return Err(Error::InvalidRelease);
            }
        }
    }
    Ok(())
}

fn validate_quarantined(entries: &[QuarantinedRelease]) -> Result<(), Error> {
    if entries.len() > MAX_QUARANTINE_ENTRIES {
        return Err(Error::InvalidRelease);
    }
    for entry in entries {
        if !org_release::valid_release_id(&entry.release_id) || entry.sequence == 0 {
            return Err(Error::InvalidRelease);
        }
    }
    Ok(())
}

/// Computes the default dedicated `0700` update-state directory sibling to `store_dir`.
pub fn default_update_state_dir(store_dir: &Path, explicit_store: bool) -> Result<PathBuf, Error> {
    if !store_dir.is_absolute() {
        return Err(Error::UnsafeInput);
    }
    let parent = store_dir.parent().ok_or(Error::UnsafeInput)?;
    if !explicit_store {
        return Ok(parent.join("release-updater"));
    }
    let name = store_dir
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or(Error::UnsafeInput)?;
    Ok(parent.join(format!("{name}.updater")))
}

/// Owner-only (`0700`) persistent store for [`UpdateState`].
#[derive(Debug, Clone)]
pub struct UpdateStateStore {
    dir: PathBuf,
}

impl UpdateStateStore {
    /// Opens or creates `dir` with owner-only `0700` permissions, refusing symlinks.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        let normalized = normalize_lstat_path(dir);
        if !normalized.is_absolute() {
            return Err(Error::UnsafeInput);
        }
        if let Ok(meta) = fs::symlink_metadata(&normalized) {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(Error::UnsafeInput);
            }
        }
        org_platform::fs::create_owner_only_dir_all(&normalized).map_err(|_| Error::UnsafeInput)?;
        org_platform::fs::set_owner_only_dir(&normalized).map_err(|_| Error::UnsafeInput)?;
        let uid = org_platform::fs::effective_uid().map_err(|_| Error::UnsafeInput)?;
        let _verified = org_platform::fs::OwnerOnlyDir::open(&normalized, uid)
            .map_err(|_| Error::UnsafeInput)?;
        let canonical = normalized.canonicalize().map_err(|_| Error::UnsafeInput)?;
        Ok(Self { dir: canonical })
    }

    /// Inspect an existing state directory read-only (returns default state if directory does not exist).
    pub fn read_readonly(dir: &Path) -> Result<UpdateState, Error> {
        let normalized = normalize_lstat_path(dir);
        if !normalized.is_absolute() {
            return Err(Error::UnsafeInput);
        }
        match fs::symlink_metadata(&normalized) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(UpdateState::default()),
            Err(_) => return Err(Error::UnsafeInput),
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err(Error::UnsafeInput);
            }
            Ok(_) => {}
        }
        let canonical = normalized.canonicalize().map_err(|_| Error::UnsafeInput)?;
        read_state_beneath(&canonical)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn load(&self) -> Result<UpdateState, Error> {
        let uid = org_platform::fs::effective_uid().map_err(|_| Error::UnsafeInput)?;
        let _verified =
            org_platform::fs::OwnerOnlyDir::open(&self.dir, uid).map_err(|_| Error::UnsafeInput)?;
        read_state_beneath(&self.dir)
    }

    pub fn save(&self, state: &UpdateState) -> Result<(), Error> {
        state.validate()?;
        let uid = org_platform::fs::effective_uid().map_err(|_| Error::UnsafeInput)?;
        let _verified =
            org_platform::fs::OwnerOnlyDir::open(&self.dir, uid).map_err(|_| Error::UnsafeInput)?;
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| Error::InvalidRelease)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(Error::UnsafeInput);
        }
        org_platform::fs::write_regular_beneath(&self.dir, Path::new(STATE_FILE_NAME), &bytes)
            .map_err(|_| Error::UnsafeInput)?;
        Ok(())
    }

    /// Persists the pre-swap rollback checkpoint atomically before touching any service or pin.
    pub fn record_pending(
        &self,
        previous_pin: Option<ReleasePin>,
        target_pin: ReleasePin,
        pointer_url: Option<String>,
    ) -> Result<UpdateState, Error> {
        self.record_pending_inner(previous_pin, target_pin, pointer_url, None)
    }

    /// Persists the pre-swap rollback checkpoint alongside the exact managed instances that were
    /// running before the swap began, so crash recovery verifies and restores every one of them.
    pub fn record_pending_with_services(
        &self,
        previous_pin: Option<ReleasePin>,
        target_pin: ReleasePin,
        pointer_url: Option<String>,
        expected_running_instances: Vec<String>,
    ) -> Result<UpdateState, Error> {
        self.record_pending_inner(
            previous_pin,
            target_pin,
            pointer_url,
            Some(expected_running_instances),
        )
    }

    fn record_pending_inner(
        &self,
        previous_pin: Option<ReleasePin>,
        target_pin: ReleasePin,
        pointer_url: Option<String>,
        expected_running_instances: Option<Vec<String>>,
    ) -> Result<UpdateState, Error> {
        let mut state = self.load()?;
        state.pending_health_check = Some(PendingHealthCheck {
            previous_pin,
            target_pin,
            started_at_ms: now_ms(),
            pointer_url: pointer_url.map(|u| org_redact::redact_text(&u, &[])),
            expected_running_instances,
        });
        self.save(&state)?;
        Ok(state)
    }

    /// Clears a pending checkpoint without altering the confirmed release (e.g. pre-commit abort).
    pub fn clear_pending(&self) -> Result<UpdateState, Error> {
        let mut state = self.load()?;
        state.pending_health_check = None;
        self.save(&state)?;
        Ok(state)
    }

    /// Marks `pin` as confirmed healthy and clears any pending health-check checkpoint.
    pub fn confirm_healthy(&self, pin: &ReleasePin) -> Result<UpdateState, Error> {
        let mut state = self.load()?;
        state.confirmed = Some(pin.clone());
        state.pending_health_check = None;
        self.save(&state)?;
        Ok(state)
    }

    /// Records a failed release in the quarantine tombstone, clears pending state, and restores
    /// `confirmed` to `rolled_back_to` when rollback succeeded.
    pub fn record_rollback_and_quarantine(
        &self,
        failed: &ReleasePin,
        rolled_back_to: Option<&ReleasePin>,
        reason: &str,
    ) -> Result<UpdateState, Error> {
        let mut state = self.load()?;
        state.pending_health_check = None;
        if let Some(prev) = rolled_back_to {
            state.confirmed = Some(prev.clone());
        }
        let clean_reason = sanitize_reason(reason);
        state
            .quarantined
            .retain(|entry| entry.release_id != failed.release_id);
        state.quarantined.push(QuarantinedRelease {
            release_id: failed.release_id.clone(),
            sequence: failed.sequence,
            manifest_sha256: failed.manifest_sha256.clone(),
            reason: clean_reason,
            quarantined_at_ms: now_ms(),
        });
        if state.quarantined.len() > MAX_QUARANTINE_ENTRIES {
            let overflow = state.quarantined.len() - MAX_QUARANTINE_ENTRIES;
            state.quarantined.drain(0..overflow);
        }
        self.save(&state)?;
        Ok(state)
    }

    /// Removes one release (or all releases when `release_id` is `None`) from quarantine.
    pub fn clear_quarantine(&self, release_id: Option<&str>) -> Result<UpdateState, Error> {
        let mut state = self.load()?;
        match release_id {
            Some(id) => state.quarantined.retain(|entry| entry.release_id != id),
            None => state.quarantined.clear(),
        }
        self.save(&state)?;
        Ok(state)
    }
}

fn read_state_beneath(canonical_dir: &Path) -> Result<UpdateState, Error> {
    let file =
        match org_platform::fs::open_regular_beneath(canonical_dir, Path::new(STATE_FILE_NAME)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(UpdateState::default()),
            Err(_) => return Err(Error::UnsafeInput),
        };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_STATE_BYTES {
        return Err(Error::UnsafeInput);
    }
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(Error::UnsafeInput);
    }
    let state: UpdateState = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidRelease)?;
    state.validate()?;
    Ok(state)
}

fn sanitize_reason(raw: &str) -> String {
    let redacted = org_redact::redact_text(raw.trim(), &[]);
    let single_line: String = redacted
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let compact = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= MAX_REASON_BYTES {
        compact
    } else {
        let suffix = " [truncated]";
        let cut = compact.floor_char_boundary(MAX_REASON_BYTES - suffix.len());
        format!("{}{suffix}", &compact[..cut])
    }
}

fn normalize_lstat_path(path: &Path) -> PathBuf {
    path.components().collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
#[path = "update_state_tests.rs"]
mod tests;
