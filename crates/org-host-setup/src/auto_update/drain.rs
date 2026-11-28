//! Idle-drain coordination before stopping services for a release swap (ADR-0009).
//!
//! Never violently kills an active agent turn unless explicitly forced (`--force`). Polls
//! [`DrainProbe::active_turns`] until zero active turns remain (`DrainPhase::Ready`) or the
//! drain timeout expires (`DrainPhase::BusyTimeout`).

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const MAX_TURN_FILE_BYTES: u64 = 64 * 1024;
const MAX_SCAN_ENTRIES: usize = 512;

/// Summary of one currently active agent turn blocking a clean swap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveTurnSnapshot {
    pub instance: String,
    pub office_id: String,
    pub employee_id: String,
    pub run_id: String,
}

/// Lifecycle phase of the pre-swap idle drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DrainPhase {
    /// Active turns are still in flight.
    Draining,
    /// Zero active turns remain (or `--force` bypassed drain); safe to swap.
    Ready,
    /// Active turns did not finish before the drain deadline; swap refused cleanly.
    BusyTimeout,
}

/// Structured receipt emitted for pre-swap idle draining.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrainReceipt {
    pub phase: DrainPhase,
    pub forced: bool,
    pub elapsed_ms: u64,
    pub remaining_turns: Vec<ActiveTurnSnapshot>,
}

/// Parameters controlling how long the updater waits for active turns to finish.
#[derive(Debug, Clone, Copy)]
pub struct DrainConfig {
    pub timeout: Duration,
    pub poll_interval: Duration,
    pub force: bool,
}

impl Default for DrainConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            poll_interval: Duration::from_millis(250),
            force: false,
        }
    }
}

/// Consumer-owned port reporting currently active managed agent turns across instances.
pub trait DrainProbe: Send + Sync {
    fn active_turns(&self) -> Result<Vec<ActiveTurnSnapshot>, String>;
}

/// Waits for all active agent turns to drain cleanly within `config.timeout`.
///
/// When `config.force` is true, records any observed active turns and immediately returns
/// `DrainPhase::Ready` with `forced: true`. Otherwise polls until `active_turns()` is empty
/// or the deadline passes (`DrainPhase::BusyTimeout`, never killing running work).
pub fn wait_for_idle_drain(
    probe: &dyn DrainProbe,
    config: DrainConfig,
) -> Result<DrainReceipt, String> {
    let started = Instant::now();
    let deadline = started + config.timeout;
    let poll = config.poll_interval.max(Duration::from_millis(5));
    loop {
        let turns = probe.active_turns()?;
        if turns.is_empty() || config.force {
            return Ok(DrainReceipt {
                phase: DrainPhase::Ready,
                forced: config.force && !turns.is_empty(),
                elapsed_ms: started.elapsed().as_millis() as u64,
                remaining_turns: turns,
            });
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(DrainReceipt {
                phase: DrainPhase::BusyTimeout,
                forced: false,
                elapsed_ms: started.elapsed().as_millis() as u64,
                remaining_turns: turns,
            });
        }
        let remaining = deadline.saturating_duration_since(now);
        std::thread::sleep(poll.min(remaining));
    }
}

/// Production [`DrainProbe`] inspecting durable `active-turn.v1.json` and `pending-start.v1.json`
/// records across a set of `(instance_name, instance_root)` targets using strict `O_NOFOLLOW`
/// bounded reads.
#[derive(Debug, Clone)]
pub struct DiskActiveTurnDrainProbe {
    instances: Vec<(String, PathBuf)>,
}

impl DiskActiveTurnDrainProbe {
    pub fn new(instances: Vec<(String, PathBuf)>) -> Self {
        Self { instances }
    }
}

impl DrainProbe for DiskActiveTurnDrainProbe {
    fn active_turns(&self) -> Result<Vec<ActiveTurnSnapshot>, String> {
        let mut active = Vec::new();
        for (instance, root) in &self.instances {
            scan_instance_active_turns(instance, root, &mut active)?;
        }
        active.sort_by(|a, b| {
            (&a.instance, &a.office_id, &a.employee_id, &a.run_id).cmp(&(
                &b.instance,
                &b.office_id,
                &b.employee_id,
                &b.run_id,
            ))
        });
        Ok(active)
    }
}

#[derive(Deserialize)]
struct OfficeJsonMini {
    office_id: String,
    org_id: String,
}

#[derive(Deserialize)]
struct ActiveTurnMini {
    employee_id: String,
    run_id: String,
    active: bool,
    #[serde(default)]
    stall_locked: bool,
    #[serde(default)]
    recovery_required: bool,
}

#[derive(Deserialize)]
struct PendingStartMini {
    employee_id: String,
    run_id: String,
}

fn scan_instance_active_turns(
    instance: &str,
    instance_root: &Path,
    out: &mut Vec<ActiveTurnSnapshot>,
) -> Result<(), String> {
    let offices_dir = instance_root.join("office-state").join("offices");
    let entries = match read_real_dir(&offices_dir)? {
        Some(entries) => entries,
        None => return Ok(()),
    };
    for office_dir in entries.into_iter().take(MAX_SCAN_ENTRIES) {
        let office_meta = match read_regular_bounded(&office_dir.join("office.json")) {
            Ok(Some(bytes)) => serde_json::from_slice::<OfficeJsonMini>(&bytes).ok(),
            _ => None,
        };
        let Some(info) = office_meta else {
            continue;
        };
        if !is_safe_single_segment(&info.org_id) || !is_safe_single_segment(&info.office_id) {
            return Err(format!(
                "refusing unsafe org_id/office_id metadata in `{}` during idle drain",
                office_dir.display()
            ));
        }
        for durable in
            candidate_durable_roots(instance_root, &office_dir, &info.org_id, &info.office_id)
        {
            let sessions_dir = durable
                .join("local")
                .join("offices")
                .join(&info.office_id)
                .join("sessions");
            scan_sessions_dir(instance, &info.office_id, &sessions_dir, out)?;
        }
    }
    Ok(())
}

fn candidate_durable_roots(
    instance_root: &Path,
    office_dir: &Path,
    org_id: &str,
    office_id: &str,
) -> [PathBuf; 3] {
    [
        office_dir.join("durable"),
        instance_root.join(org_id).join("durable"),
        instance_root
            .join("satellites")
            .join(org_id)
            .join(office_id)
            .join("durable"),
    ]
}

fn scan_sessions_dir(
    instance: &str,
    office_id: &str,
    sessions_dir: &Path,
    out: &mut Vec<ActiveTurnSnapshot>,
) -> Result<(), String> {
    let entries = match read_real_dir(sessions_dir)? {
        Some(entries) => entries,
        None => return Ok(()),
    };
    for emp_dir in entries.into_iter().take(MAX_SCAN_ENTRIES) {
        if let Some(bytes) = read_regular_bounded(&emp_dir.join("active-turn.v1.json"))? {
            if let Ok(rec) = serde_json::from_slice::<ActiveTurnMini>(&bytes) {
                if rec.active && !rec.stall_locked && !rec.recovery_required {
                    out.push(ActiveTurnSnapshot {
                        instance: instance.to_owned(),
                        office_id: office_id.to_owned(),
                        employee_id: rec.employee_id,
                        run_id: rec.run_id,
                    });
                    continue;
                }
            }
        }
        if let Some(bytes) = read_regular_bounded(&emp_dir.join("pending-start.v1.json"))? {
            if let Ok(rec) = serde_json::from_slice::<PendingStartMini>(&bytes) {
                out.push(ActiveTurnSnapshot {
                    instance: instance.to_owned(),
                    office_id: office_id.to_owned(),
                    employee_id: rec.employee_id,
                    run_id: rec.run_id,
                });
            }
        }
    }
    Ok(())
}

fn read_real_dir(dir: &Path) -> Result<Option<Vec<PathBuf>>, String> {
    let norm = normalize_lstat_path(dir);
    let meta = match fs::symlink_metadata(&norm) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("inspecting `{}`: {}", norm.display(), e.kind())),
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "refusing non-directory or symlink path `{}` during idle drain",
            norm.display()
        ));
    }
    let mut dirs = Vec::new();
    let iter =
        fs::read_dir(&norm).map_err(|e| format!("reading `{}`: {}", norm.display(), e.kind()))?;
    for entry in iter.flatten() {
        let path = entry.path();
        if fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(Some(dirs))
}

fn read_regular_bounded(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let norm = normalize_lstat_path(path);
    let file = match org_platform::fs::open_nofollow_ro(&norm) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "refusing unsafe session state file `{}` ({:?})",
                norm.display(),
                e.kind()
            ));
        }
    };
    let meta = file
        .metadata()
        .map_err(|e| format!("reading metadata of `{}`: {}", norm.display(), e.kind()))?;
    if !meta.is_file() {
        return Err(format!(
            "session state path `{}` is not a regular file",
            norm.display()
        ));
    }
    let mut buf = Vec::new();
    file.take(MAX_TURN_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("reading `{}`: {}", norm.display(), e.kind()))?;
    if buf.len() as u64 > MAX_TURN_FILE_BYTES {
        return Err(format!(
            "session state file `{}` exceeds size bound",
            norm.display()
        ));
    }
    Ok(Some(buf))
}

fn is_safe_single_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains('\0')
}

fn normalize_lstat_path(path: &Path) -> PathBuf {
    path.components().collect()
}
