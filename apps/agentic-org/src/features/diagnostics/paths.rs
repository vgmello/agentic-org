//! Instance path resolution, office discovery, and safe no-follow filesystem helpers shared by
//! `logs`, `doctor`, `support`, and `uninstall`.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::error::CliError;

const MAX_CONTROL_BYTES: u64 = 64 * 1024;
const DEV_ACCOUNT_BASE: &str = "ORG_DEV_ACCOUNT_BASE";

/// Non-secret paths and metadata discovered for one Agentic Org instance.
#[derive(Debug, Clone)]
pub(crate) struct InstancePaths {
    pub(crate) name: String,
    pub(crate) account_base: PathBuf,
    pub(crate) user_temp: Option<PathBuf>,
    pub(crate) setup_dir: PathBuf,
    pub(crate) setup_record: PathBuf,
    pub(crate) locator_dir: PathBuf,
    pub(crate) locator_file: PathBuf,
    pub(crate) locator_lock: PathBuf,
    pub(crate) instance_root: PathBuf,
    pub(crate) instance_config: PathBuf,
    pub(crate) registry_created: PathBuf,
    pub(crate) office_home: PathBuf,
    pub(crate) host_lock: PathBuf,
    pub(crate) host_registry_db: PathBuf,
    pub(crate) logs_dir: PathBuf,
    pub(crate) offices_dir: PathBuf,
    pub(crate) run_dir: PathBuf,
    pub(crate) host_socket: PathBuf,
    pub(crate) instance_id: Option<String>,
    pub(crate) service_mode: Option<String>,
    pub(crate) credentials_dir: Option<PathBuf>,
}

/// Non-secret metadata for a discovered office in an instance.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DiscoveredOffice {
    pub(crate) office_id: String,
    pub(crate) office_name: String,
    pub(crate) role: String,
    pub(crate) org_id: String,
    pub(crate) org_name: String,
    #[serde(skip)]
    pub(crate) durable_state: Option<PathBuf>,
    #[serde(skip)]
    pub(crate) office_db: Option<PathBuf>,
}

/// Non-blocking probe result for an advisory `flock` lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LockProbe {
    NotPresent,
    Free,
    Held,
    Unsafe(String),
}

/// Validates an instance name (1..=32 lowercase ASCII letters, digits, or hyphens).
pub(crate) fn validate_instance_name(name: &str) -> Result<(), CliError> {
    let valid = (1..=32).contains(&name.len())
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return Err(CliError::usage(format!(
            "invalid instance name `{name}`: must be 1–32 lowercase ASCII letters, digits, or hyphens"
        )));
    }
    Ok(())
}

/// Rejects NUL bytes or `..` parent traversal before any join or open.
pub(crate) fn validate_plain_path(path: &Path) -> Result<(), CliError> {
    if path.as_os_str().as_bytes().contains(&0) {
        return Err(CliError::usage("path must not contain NUL bytes"));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(CliError::usage(format!(
            "path `{}` must not contain `..` traversal components",
            path.display()
        )));
    }
    Ok(())
}

/// Validates a persisted or configured path: plain (no NUL/`..`), no `.` segments, and absolute.
pub(crate) fn validate_loaded_abs_path(path: &Path, label: &str) -> Result<(), CliError> {
    validate_plain_path(path)?;
    if !path.is_absolute() {
        return Err(CliError::failed(
            "diagnostics::unsafe_path",
            format!("{label} `{}` must be an absolute path", path.display()),
        ));
    }
    let has_dot = path
        .as_os_str()
        .as_bytes()
        .split(|&b| b == b'/')
        .any(|seg| seg == b"." || seg == b"..");
    if has_dot {
        return Err(CliError::failed(
            "diagnostics::unsafe_path",
            format!(
                "{label} `{}` must not contain `.` or `..` path components",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Strips trailing `/` and `/.` so `symlink_metadata` inspects a link itself rather than following it.
pub(crate) fn normalize_lstat_path(path: &Path) -> PathBuf {
    let bytes = path.as_os_str().as_bytes();
    let mut end = bytes.len();
    loop {
        if end > 1 && bytes[end - 1] == b'/' {
            end -= 1;
        } else if end > 2 && bytes[end - 2] == b'/' && bytes[end - 1] == b'.' {
            end -= 2;
        } else {
            break;
        }
    }
    PathBuf::from(OsStr::from_bytes(&bytes[..end]))
}

/// Reads a regular file read-only without following symlinks or blocking on FIFOs, bounded to `limit`.
pub(crate) fn read_regular_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let norm = normalize_lstat_path(path);
    let file = org_platform::fs::open_nofollow_ro(&norm)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let mut buf = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file exceeds {limit}-byte bound"),
        ));
    }
    Ok(buf)
}

/// Atomically writes `bytes` with `0600` permissions, refusing symlinks, `..`, trailing slashes,
/// and non-regular destinations.
pub(crate) fn write_atomic_owner_only(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    if path.as_os_str().as_bytes().contains(&0) {
        return Err("output path must not contain NUL bytes".to_owned());
    }
    if path.as_os_str().as_bytes().ends_with(b"/") {
        return Err("output path must name a file, not a directory".to_owned());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("output path must not contain `..` traversal components".to_owned());
    }
    let abs = std::path::absolute(path)
        .map_err(|e| format!("resolving output path `{}`: {e}", path.display()))?;
    let (Some(parent), Some(leaf)) = (abs.parent(), abs.file_name()) else {
        return Err(format!("output path `{}` must name a file", path.display()));
    };
    let parent_lstat = fs::symlink_metadata(normalize_lstat_path(parent))
        .map_err(|e| format!("inspecting output parent `{}`: {e}", parent.display()))?;
    if !parent_lstat.is_dir() {
        return Err(format!(
            "output parent `{}` must be a real directory (not a symlink)",
            parent.display()
        ));
    }
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("canonicalizing output parent `{}`: {e}", parent.display()))?;
    org_platform::fs::write_regular_beneath(&canonical_parent, Path::new(leaf), bytes)
        .map_err(|e| format!("writing `{}` atomically (0600): {e}", abs.display()))?;
    Ok(canonical_parent.join(leaf))
}

/// Probes whether an advisory `flock` lock file is currently held without creating or modifying it.
pub(crate) fn probe_lock_held(path: &Path) -> LockProbe {
    let norm = normalize_lstat_path(path);
    let file = match org_platform::fs::open_nofollow_ro(&norm) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return LockProbe::NotPresent,
        Err(e) => return LockProbe::Unsafe(e.to_string()),
    };
    match file.metadata() {
        Ok(m) if !m.is_file() => {
            return LockProbe::Unsafe("lock path is not a regular file".into())
        }
        Err(e) => return LockProbe::Unsafe(e.to_string()),
        Ok(_) => {}
    }
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            LockProbe::Free
        }
        Err(TryLockError::WouldBlock) => LockProbe::Held,
        Err(TryLockError::Error(e)) => LockProbe::Unsafe(e.to_string()),
    }
}

/// Acquires an exclusive non-blocking lock on an existing regular file without following symlinks.
/// Returns `Ok(None)` when missing, `Ok(Some(File))` when acquired, or `Err` when busy/unsafe.
pub(crate) fn try_lock_existing_exclusive(path: &Path) -> Result<Option<File>, LockProbe> {
    let norm = normalize_lstat_path(path);
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(
            org_platform::fs::O_NOFOLLOW
                | org_platform::fs::O_NONBLOCK
                | org_platform::fs::O_CLOEXEC,
        )
        .open(&norm)
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(LockProbe::Unsafe(e.to_string())),
    };
    match file.metadata() {
        Ok(m) if !m.is_file() => {
            return Err(LockProbe::Unsafe("lock path is not a regular file".into()))
        }
        Err(e) => return Err(LockProbe::Unsafe(e.to_string())),
        Ok(_) => {}
    }
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Err(LockProbe::Held),
        Err(TryLockError::Error(e)) => Err(LockProbe::Unsafe(e.to_string())),
    }
}

/// Acquires an exclusive non-blocking lock on an existing directory without following symlinks.
pub(crate) fn try_lock_dir_exclusive(path: &Path) -> Result<Option<File>, LockProbe> {
    let norm = normalize_lstat_path(path);
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(
            org_platform::fs::O_NOFOLLOW
                | org_platform::fs::O_NONBLOCK
                | org_platform::fs::O_CLOEXEC,
        )
        .open(&norm)
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(LockProbe::Unsafe(e.to_string())),
    };
    match file.metadata() {
        Ok(m) if !m.is_dir() => return Err(LockProbe::Unsafe("not a directory".into())),
        Err(e) => return Err(LockProbe::Unsafe(e.to_string())),
        Ok(_) => {}
    }
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Err(LockProbe::Held),
        Err(TryLockError::Error(e)) => Err(LockProbe::Unsafe(e.to_string())),
    }
}

/// Resolves the account application base and user temp base without creating anything on disk.
pub(crate) fn resolve_account_bases() -> Result<(PathBuf, Option<PathBuf>), CliError> {
    if cfg!(debug_assertions) {
        if let Some(dev) = std::env::var_os(DEV_ACCOUNT_BASE).filter(|v| !v.is_empty()) {
            let base = PathBuf::from(dev);
            if !base.is_absolute() {
                return Err(CliError::failed(
                    "cli::config::invalid",
                    format!("{DEV_ACCOUNT_BASE} must be an absolute path"),
                ));
            }
            let temp = base.join("temp");
            return Ok((base, Some(temp)));
        }
    }
    let home = std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            let uid = org_platform::posix::effective_uid();
            org_platform::posix::account_by_uid(uid)
                .ok()
                .flatten()
                .and_then(|acct| acct.home)
                .filter(|p| p.is_absolute())
        })
        .ok_or_else(|| {
            CliError::failed(
                "diagnostics::no_home",
                "account home directory is unavailable",
            )
        })?;
    let base = org_platform::dirs::application_base(&home).map_err(|e| {
        CliError::failed(
            "diagnostics::unsupported",
            format!("instance locations are unavailable: {e}"),
        )
    })?;
    let user_temp = org_platform::dirs::user_temp_base(&home).ok();
    Ok((base, user_temp))
}

#[derive(Deserialize)]
struct SetupRecordMini {
    input: SetupInputMini,
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    credentials: Option<PathBuf>,
}

#[derive(Deserialize)]
struct SetupInputMini {
    #[serde(default)]
    service: Option<String>,
    root: PathBuf,
}

#[derive(Deserialize)]
struct LocatorMini {
    #[serde(default)]
    entries: Vec<LocatorEntryMini>,
}

#[derive(Deserialize)]
struct LocatorEntryMini {
    binding: BindingMini,
}

#[derive(Deserialize)]
struct InstanceConfigMini {
    binding: BindingMini,
}

#[derive(Deserialize)]
struct BindingMini {
    name: String,
    id: String,
    root: PathBuf,
    temp_folder: PathBuf,
}

/// Resolves all non-secret paths of instance `instance_name` read-only (never creates folders).
pub(crate) fn resolve_instance(instance_name: Option<&str>) -> Result<InstancePaths, CliError> {
    let name = instance_name.unwrap_or("default").to_owned();
    validate_instance_name(&name)?;
    let (base, user_temp) = resolve_account_bases()?;
    validate_loaded_abs_path(&base, "account base")?;
    if let Some(ref temp) = user_temp {
        validate_loaded_abs_path(temp, "user temp base")?;
    }
    let setup_dir = base.join("setup").join(&name);
    let setup_record = setup_dir.join("setup.v1.json");
    let locator_dir = base.join("locator");
    let locator_file = locator_dir.join("instances.json");
    let locator_lock = locator_dir.join("registry.lock");

    let setup = read_regular_bounded(&setup_record, MAX_CONTROL_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SetupRecordMini>(&bytes).ok());
    let locator_binding = read_regular_bounded(&locator_file, MAX_CONTROL_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LocatorMini>(&bytes).ok())
        .and_then(|loc| {
            loc.entries
                .into_iter()
                .find(|entry| entry.binding.name == name)
                .map(|entry| entry.binding)
        });

    if let Some(ref s) = setup {
        validate_loaded_abs_path(&s.input.root, "setup instance root")?;
        if let Some(ref creds) = s.credentials {
            validate_loaded_abs_path(creds, "setup credentials directory")?;
        }
    }
    if let Some(ref b) = locator_binding {
        validate_loaded_abs_path(&b.root, "locator instance root")?;
        validate_loaded_abs_path(&b.temp_folder, "locator temp folder")?;
    }

    let instance_root = setup
        .as_ref()
        .map(|s| s.input.root.clone())
        .or_else(|| locator_binding.as_ref().map(|b| b.root.clone()))
        .unwrap_or_else(|| base.join("instances").join(&name));
    validate_loaded_abs_path(&instance_root, "instance root")?;
    let instance_config = instance_root.join("instance.json");
    let config_binding = read_regular_bounded(&instance_config, MAX_CONTROL_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<InstanceConfigMini>(&bytes).ok())
        .map(|cfg| cfg.binding);
    if let Some(ref b) = config_binding {
        validate_loaded_abs_path(&b.root, "instance config root")?;
        validate_loaded_abs_path(&b.temp_folder, "instance config temp folder")?;
    }

    let instance_id = setup
        .as_ref()
        .and_then(|s| s.instance_id.clone())
        .or_else(|| locator_binding.as_ref().map(|b| b.id.clone()))
        .or_else(|| config_binding.as_ref().map(|b| b.id.clone()));
    let service_mode = setup.as_ref().and_then(|s| s.input.service.clone());
    let credentials_dir = setup.as_ref().and_then(|s| s.credentials.clone());

    let office_home = instance_root.join("office-state");
    let run_dir = config_binding
        .as_ref()
        .map(|b| b.temp_folder.clone())
        .or_else(|| locator_binding.as_ref().map(|b| b.temp_folder.clone()))
        .unwrap_or_else(|| office_home.join("run"));
    validate_loaded_abs_path(&run_dir, "instance runtime directory")?;

    Ok(InstancePaths {
        name,
        account_base: base,
        user_temp,
        setup_dir,
        setup_record,
        locator_dir,
        locator_file,
        locator_lock,
        registry_created: instance_root.join("host-registry.created"),
        host_lock: office_home.join("host.lock"),
        host_registry_db: office_home.join("host-registry.db"),
        logs_dir: office_home.join("logs"),
        offices_dir: office_home.join("offices"),
        host_socket: run_dir.join("host.sock"),
        run_dir,
        office_home,
        instance_root,
        instance_config,
        instance_id,
        service_mode,
        credentials_dir,
    })
}

#[derive(Deserialize)]
struct OfficeJsonMini {
    office_id: String,
    office_name: String,
    role: String,
    org_id: String,
    org_name: String,
}

/// Discovers offices recorded under `paths.offices_dir`, filtered optionally by `office_filter`.
pub(crate) fn discover_offices(
    paths: &InstancePaths,
    office_filter: Option<&str>,
) -> Vec<DiscoveredOffice> {
    let Ok(entries) = fs::read_dir(&paths.offices_dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    dirs.sort();
    let mut offices = Vec::new();
    for dir in dirs {
        let Ok(meta) = fs::symlink_metadata(&dir) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let Some(info) = read_regular_bounded(&dir.join("office.json"), MAX_CONTROL_BYTES)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OfficeJsonMini>(&bytes).ok())
        else {
            continue;
        };
        if office_filter.is_some_and(|want| want != info.office_id) {
            continue;
        }
        let (durable_state, office_db) =
            resolve_office_data_roots(paths, &dir, &info.org_id, &info.office_id);
        offices.push(DiscoveredOffice {
            office_id: info.office_id,
            office_name: info.office_name,
            role: info.role,
            org_id: info.org_id,
            org_name: info.org_name,
            durable_state,
            office_db,
        });
    }
    offices
}

fn resolve_office_data_roots(
    paths: &InstancePaths,
    office_dir: &Path,
    org_id: &str,
    office_id: &str,
) -> (Option<PathBuf>, Option<PathBuf>) {
    if let Ok(target) = org_backup::locate(&paths.office_home, org_id, Some(office_id)) {
        let state = target.durable.join("state");
        let durable = state.is_dir().then_some(state);
        let db = target.runtime.join(office_id).join("office.db");
        let office_db = db.is_file().then_some(db);
        return (durable, office_db);
    }
    let local_state = office_dir.join("durable").join("state");
    if local_state.is_dir() {
        let db = find_nearby_office_db(office_dir, office_id);
        return (Some(local_state), db);
    }
    let org_base = paths.instance_root.join(org_id);
    let hq_state = org_base.join("durable").join("state");
    if hq_state.is_dir() {
        let db = find_nearby_office_db(&org_base, office_id);
        return (Some(hq_state), db);
    }
    let sat_base = paths
        .instance_root
        .join("satellites")
        .join(org_id)
        .join(office_id);
    let sat_state = sat_base.join("durable").join("state");
    if sat_state.is_dir() {
        let db = find_nearby_office_db(&sat_base, office_id);
        return (Some(sat_state), db);
    }
    (None, None)
}

/// Locates `office.db` beside an office/org root without mutating anything.
pub(crate) fn find_nearby_office_db(base: &Path, office_id: &str) -> Option<PathBuf> {
    let candidates = [
        base.join("office.db"),
        base.join("runtime").join("office.db"),
        base.join("runtime").join(office_id).join("office.db"),
    ];
    candidates
        .into_iter()
        .find(|cand| fs::symlink_metadata(cand).is_ok_and(|m| m.is_file()))
}

/// Returns true when `path` is owned by the current effective UID with mode `expected_mode`.
pub(crate) fn check_owner_and_mode(path: &Path, expected_mode: u32) -> io::Result<Option<String>> {
    let meta = fs::symlink_metadata(normalize_lstat_path(path))?;
    if meta.file_type().is_symlink() {
        return Ok(Some(format!(
            "{} is a symlink (must be a real entry)",
            path.display()
        )));
    }
    let uid = org_platform::posix::effective_uid();
    if meta.uid() != uid {
        return Ok(Some(format!(
            "{} is owned by uid {}, expected {uid}",
            path.display(),
            meta.uid()
        )));
    }
    let mode = meta.mode() & 0o7777;
    if mode != expected_mode {
        return Ok(Some(format!(
            "{} has mode {:04o}, expected {:04o}",
            path.display(),
            mode,
            expected_mode
        )));
    }
    Ok(None)
}
