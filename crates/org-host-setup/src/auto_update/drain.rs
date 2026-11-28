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
/// records across a set of `(instance_name, instance_root)` targets using component-wise
/// `O_NOFOLLOW` traversal and bounded regular-file reads (`org_platform::fs::open_regular_beneath`).
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
        active.dedup();
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
    let norm_root = normalize_lstat_path(instance_root);
    let Some(office_names) = list_real_subdirs_beneath(&norm_root, &["office-state", "offices"])?
    else {
        return Ok(());
    };
    for office_dir_name in office_names.into_iter().take(MAX_SCAN_ENTRIES) {
        let office_json_rel =
            rel_path(&["office-state", "offices", &office_dir_name, "office.json"]);
        let Some(bytes) = read_regular_beneath_bounded(&norm_root, &office_json_rel)? else {
            continue;
        };
        let info: OfficeJsonMini = serde_json::from_slice(&bytes).map_err(|_| {
            format!(
                "corrupt office metadata `{}` during idle drain",
                norm_root.join(&office_json_rel).display()
            )
        })?;
        if !is_safe_single_segment(&info.org_id) || !is_safe_single_segment(&info.office_id) {
            return Err(format!(
                "refusing unsafe org_id/office_id metadata in `{}` during idle drain",
                norm_root.join(&office_json_rel).display()
            ));
        }
        scan_office_durable_roots(instance, &norm_root, &office_dir_name, &info, out)?;
    }
    Ok(())
}

fn scan_office_durable_roots(
    instance: &str,
    norm_root: &Path,
    office_dir_name: &str,
    info: &OfficeJsonMini,
    out: &mut Vec<ActiveTurnSnapshot>,
) -> Result<(), String> {
    for durable_segments in
        candidate_durable_root_segments(office_dir_name, &info.org_id, &info.office_id)
    {
        let mut session_segments = durable_segments;
        session_segments.extend([
            "local".to_owned(),
            "offices".to_owned(),
            info.office_id.clone(),
            "sessions".to_owned(),
        ]);
        scan_sessions_dir(instance, &info.office_id, norm_root, &session_segments, out)?;
    }
    Ok(())
}

/// Returns candidate durable root paths relative to `instance_root`:
/// 1. Legacy/office-local layout: `office-state/offices/<office_dir>/durable`
/// 2. HQ layout: `<org_id>/durable`
/// 3. Satellite office layout (matching `org-host::writer::satellite`): `<org_id>/offices/<office_id>/durable`
fn candidate_durable_root_segments(
    office_dir_name: &str,
    org_id: &str,
    office_id: &str,
) -> [Vec<String>; 3] {
    [
        vec![
            "office-state".to_owned(),
            "offices".to_owned(),
            office_dir_name.to_owned(),
            "durable".to_owned(),
        ],
        vec![org_id.to_owned(), "durable".to_owned()],
        vec![
            org_id.to_owned(),
            "offices".to_owned(),
            office_id.to_owned(),
            "durable".to_owned(),
        ],
    ]
}

fn scan_sessions_dir(
    instance: &str,
    office_id: &str,
    norm_root: &Path,
    session_segments: &[String],
    out: &mut Vec<ActiveTurnSnapshot>,
) -> Result<(), String> {
    let seg_refs: Vec<&str> = session_segments.iter().map(String::as_str).collect();
    let Some(emp_names) = list_real_subdirs_beneath(norm_root, &seg_refs)? else {
        return Ok(());
    };
    let sessions_rel = rel_path(&seg_refs);
    for emp_name in emp_names.into_iter().take(MAX_SCAN_ENTRIES) {
        let emp_rel = sessions_rel.join(&emp_name);
        if let Some(snapshot) = inspect_employee_session(instance, office_id, norm_root, &emp_rel)?
        {
            out.push(snapshot);
        }
    }
    Ok(())
}

fn inspect_employee_session(
    instance: &str,
    office_id: &str,
    norm_root: &Path,
    emp_rel: &Path,
) -> Result<Option<ActiveTurnSnapshot>, String> {
    let active_rel = emp_rel.join("active-turn.v1.json");
    if let Some(bytes) = read_regular_beneath_bounded(norm_root, &active_rel)? {
        let rec: ActiveTurnMini = serde_json::from_slice(&bytes).map_err(|_| {
            format!(
                "corrupt active-turn state `{}` during idle drain",
                norm_root.join(&active_rel).display()
            )
        })?;
        if rec.active && !rec.stall_locked && !rec.recovery_required {
            return Ok(Some(ActiveTurnSnapshot {
                instance: instance.to_owned(),
                office_id: office_id.to_owned(),
                employee_id: rec.employee_id,
                run_id: rec.run_id,
            }));
        }
    }
    let pending_rel = emp_rel.join("pending-start.v1.json");
    if let Some(bytes) = read_regular_beneath_bounded(norm_root, &pending_rel)? {
        let rec: PendingStartMini = serde_json::from_slice(&bytes).map_err(|_| {
            format!(
                "corrupt pending-start state `{}` during idle drain",
                norm_root.join(&pending_rel).display()
            )
        })?;
        return Ok(Some(ActiveTurnSnapshot {
            instance: instance.to_owned(),
            office_id: office_id.to_owned(),
            employee_id: rec.employee_id,
            run_id: rec.run_id,
        }));
    }
    Ok(None)
}

/// Verifies `norm_root` and every single intermediate segment of `segments` is a real directory
/// (never a symbolic link or non-directory), returning `Ok(None)` only when a directory segment
/// does not exist (`NotFound`), and failing closed on any symlink, non-directory, or I/O error.
fn verify_real_dir_chain(norm_root: &Path, segments: &[&str]) -> Result<Option<PathBuf>, String> {
    let mut current = norm_root.to_path_buf();
    if !check_real_dir_component(&current)? {
        return Ok(None);
    }
    for seg in segments {
        if !is_safe_single_segment(seg) {
            return Err(format!(
                "refusing unsafe path segment `{seg}` under `{}` during idle drain",
                norm_root.display()
            ));
        }
        current.push(seg);
        if !check_real_dir_component(&current)? {
            return Ok(None);
        }
    }
    Ok(Some(current))
}

fn check_real_dir_component(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => Err(format!(
            "refusing symlink or non-directory path `{}` during idle drain",
            path.display()
        )),
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!(
            "inspecting `{}` during idle drain: {}",
            path.display(),
            e.kind()
        )),
    }
}

/// Lists direct child subdirectory names beneath `norm_root/<segments>`, refusing symlinks at
/// `norm_root`, every intermediate directory, and every direct child entry.
fn list_real_subdirs_beneath(
    norm_root: &Path,
    segments: &[&str],
) -> Result<Option<Vec<String>>, String> {
    let Some(dir) = verify_real_dir_chain(norm_root, segments)? else {
        return Ok(None);
    };
    let mut names = Vec::new();
    let iter = fs::read_dir(&dir).map_err(|e| {
        format!(
            "reading `{}` during idle drain: {}",
            dir.display(),
            e.kind()
        )
    })?;
    for entry_res in iter {
        let entry = entry_res.map_err(|e| {
            format!(
                "iterating `{}` during idle drain: {}",
                dir.display(),
                e.kind()
            )
        })?;
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(format!(
                    "inspecting `{}` during idle drain: {}",
                    path.display(),
                    e.kind()
                ));
            }
        };
        if meta.file_type().is_symlink() {
            return Err(format!(
                "refusing symlink entry `{}` during idle drain",
                path.display()
            ));
        }
        if !meta.is_dir() {
            continue;
        }
        let name = entry.file_name().into_string().map_err(|_| {
            format!(
                "refusing non-UTF-8 directory entry under `{}` during idle drain",
                dir.display()
            )
        })?;
        if !is_safe_single_segment(&name) {
            return Err(format!(
                "refusing unsafe directory name `{name}` under `{}` during idle drain",
                dir.display()
            ));
        }
        names.push(name);
    }
    names.sort();
    Ok(Some(names))
}

fn read_regular_beneath_bounded(
    norm_root: &Path,
    relative: &Path,
) -> Result<Option<Vec<u8>>, String> {
    let file = match org_platform::fs::open_regular_beneath(norm_root, relative) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "refusing unsafe session state file `{}` ({:?})",
                norm_root.join(relative).display(),
                e.kind()
            ));
        }
    };
    let meta = file.metadata().map_err(|e| {
        format!(
            "reading metadata of `{}`: {}",
            norm_root.join(relative).display(),
            e.kind()
        )
    })?;
    if !meta.is_file() {
        return Err(format!(
            "session state path `{}` is not a regular file",
            norm_root.join(relative).display()
        ));
    }
    let mut buf = Vec::new();
    file.take(MAX_TURN_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| {
            format!(
                "reading `{}`: {}",
                norm_root.join(relative).display(),
                e.kind()
            )
        })?;
    if buf.len() as u64 > MAX_TURN_FILE_BYTES {
        return Err(format!(
            "session state file `{}` exceeds size bound",
            norm_root.join(relative).display()
        ));
    }
    Ok(Some(buf))
}

fn rel_path(segments: &[&str]) -> PathBuf {
    let mut path = PathBuf::new();
    for seg in segments {
        path.push(seg);
    }
    path
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
