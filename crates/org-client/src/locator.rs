//! Automatic local office socket discovery from `locator/instances.json` and saved CLI context
//! (`TASK-248`), using `org-platform` contracts with zero platform conditionals.
//!
//! Reads the per-user instance locator (`<application_base>/locator/instances.json`, or
//! `<ORG_DEV_ACCOUNT_BASE>/locator/instances.json` in development builds), verifies every
//! runtime directory and Unix socket without following symbolic links, and resolves active
//! `<temp_folder>/o/<handle>/s.sock` sockets on both macOS and GNU/Linux.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Read as _;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};

use org_office_launch::{OfficeFile, MAX_OFFICE_FILE_BYTES, OFFICES_DIR_NAME, OFFICE_FILE_NAME};
use org_proto::location::{
    is_office_socket_in, office_socket_dir_in, office_sockets_dir, selected_office_socket_dir_in,
    OFFICE_HOME_ENV, OFFICE_SOCKET_FILE_NAME,
};
use serde_json::Value;

use crate::paths::check_socket_path;

/// Development account-base override shared with `org-host` and `agent-os` (debug builds only).
const DEV_ACCOUNT_BASE_ENV: &str = "ORG_DEV_ACCOUNT_BASE";
/// Operator CLI config directory override shared with `org-host::setup_flow::cli_context`.
const CONFIG_DIR_ENV: &str = "AGENTIC_ORG_CONFIG_DIR";
/// Same cap as `org-host/src/instance/registry.rs` (`MAX_BYTES`).
const MAX_LOCATOR_BYTES: u64 = 64 * 1024;
const MAX_CONTEXT_BYTES: u64 = 64 * 1024;

/// One actively listening local office socket discovered on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredOffice {
    pub socket: PathBuf,
    pub runtime_dir: PathBuf,
    pub socket_handle: String,
    pub instance_name: String,
    pub instance_id: Option<String>,
    pub office_id: Option<String>,
    pub office_name: Option<String>,
}

impl DiscoveredOffice {
    /// Human-readable one-line description for disambiguation errors.
    pub fn display_label(&self) -> String {
        let id = self.office_id.as_deref().unwrap_or(&self.socket_handle);
        match self.office_name.as_deref() {
            Some(name) if !name.is_empty() => {
                format!("{id} ({name}, instance {})", self.instance_name)
            }
            _ => format!("{id} (instance {})", self.instance_name),
        }
    }
}

/// Non-secret saved selection read from `cli-context.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedContext {
    pub instance_name: Option<String>,
    pub office_id: String,
}

/// Why auto-discovery could not pick a single active local office.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOfficeError {
    NotFound {
        selector: Option<String>,
    },
    Ambiguous {
        selector: Option<String>,
        candidates: Vec<DiscoveredOffice>,
    },
}

impl fmt::Display for ResolveOfficeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound {
                selector: Some(sel),
            } => {
                write!(f, "no active local office matched `{sel}`")
            }
            Self::NotFound { selector: None } => write!(f, "no active local office was found"),
            Self::Ambiguous {
                selector,
                candidates,
            } => {
                let labels: Vec<String> = candidates
                    .iter()
                    .map(DiscoveredOffice::display_label)
                    .collect();
                match selector {
                    Some(sel) => write!(
                        f,
                        "`{sel}` matched {} active offices ({}); pass a full --office <uuid> or --socket <path>",
                        candidates.len(),
                        labels.join(", ")
                    ),
                    None => write!(
                        f,
                        "{} active local offices are running ({}); pass --office <uuid> or --socket <path>",
                        candidates.len(),
                        labels.join(", ")
                    ),
                }
            }
        }
    }
}

impl std::error::Error for ResolveOfficeError {}

/// Resolves an active local office socket from this account's `locator/instances.json`,
/// optional `ORG_OFFICE_HOME`, and saved `cli-context.json` fallback.
pub fn resolve_local_office(
    selector: Option<&str>,
) -> Result<DiscoveredOffice, ResolveOfficeError> {
    let env = |name: &str| std::env::var_os(name);
    let locator = default_locator_path_from(&env, cfg!(debug_assertions));
    let saved = default_context_path_from(&env, cfg!(debug_assertions))
        .as_deref()
        .and_then(load_saved_context);
    let extra_home = env(OFFICE_HOME_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    resolve_local_office_from(
        locator.as_deref(),
        extra_home.as_deref(),
        saved.as_ref(),
        selector,
    )
}

/// Pure path-parameterized resolver used by [`resolve_local_office`] and unit tests.
pub fn resolve_local_office_from(
    locator_path: Option<&Path>,
    extra_office_home: Option<&Path>,
    saved_context: Option<&SavedContext>,
    selector: Option<&str>,
) -> Result<DiscoveredOffice, ResolveOfficeError> {
    let mut by_socket: BTreeMap<PathBuf, DiscoveredOffice> = BTreeMap::new();
    if let Some(locator) = locator_path {
        for office in discover_from_locator(locator) {
            by_socket.entry(office.socket.clone()).or_insert(office);
        }
    }
    if let Some(home) = extra_office_home {
        for office in discover_in_runtime(
            &home.join("run"),
            Some(&home.join(OFFICES_DIR_NAME)),
            "office-home",
            None,
        ) {
            by_socket.entry(office.socket.clone()).or_insert(office);
        }
    }
    let mut offices: Vec<DiscoveredOffice> = by_socket.into_values().collect();
    offices.sort_by(|a, b| {
        (
            a.instance_name.as_str(),
            a.office_id.as_deref().unwrap_or(""),
            a.socket_handle.as_str(),
        )
            .cmp(&(
                b.instance_name.as_str(),
                b.office_id.as_deref().unwrap_or(""),
                b.socket_handle.as_str(),
            ))
    });
    select_office(offices, saved_context, selector)
}

/// Discovers every actively listening local office listed in `locator_path` (`instances.json`).
pub fn discover_from_locator(locator_path: &Path) -> Vec<DiscoveredOffice> {
    let Some(bytes) = read_owned_regular_file(locator_path, MAX_LOCATOR_BYTES) else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_slice::<Value>(&bytes) else {
        return Vec::new();
    };
    if doc.get("version").and_then(Value::as_u64) != Some(1) {
        return Vec::new();
    }
    let Some(entries) = doc.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries {
        if entry.get("phase").and_then(Value::as_str) != Some("ready") {
            continue;
        }
        let Some(binding) = entry.get("binding") else {
            continue;
        };
        let Some(name) = binding.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(temp_str) = binding.get("temp_folder").and_then(Value::as_str) else {
            continue;
        };
        let Some(runtime_dir) = safe_abs_path(temp_str) else {
            continue;
        };
        let instance_id = binding.get("id").and_then(Value::as_str).map(str::to_owned);
        let offices_dir = binding
            .get("root")
            .and_then(Value::as_str)
            .and_then(safe_abs_path)
            .map(|root| root.join("office-state").join(OFFICES_DIR_NAME));
        found.extend(discover_in_runtime(
            &runtime_dir,
            offices_dir.as_deref(),
            name,
            instance_id.as_deref(),
        ));
    }
    found
}

/// Default `locator/instances.json` path from environment variables (same rule as `org-host`).
pub fn default_locator_path_from(
    env: &dyn Fn(&str) -> Option<OsString>,
    development: bool,
) -> Option<PathBuf> {
    if development {
        if let Some(base) = env(DEV_ACCOUNT_BASE_ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            return Some(base.join("locator").join("instances.json"));
        }
    }
    let home = env("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())?;
    org_platform::dirs::application_base(&home)
        .ok()
        .map(|base| base.join("locator").join("instances.json"))
}

/// Default `cli-context.json` path from environment variables (same rule as `cli_context.rs`).
pub fn default_context_path_from(
    env: &dyn Fn(&str) -> Option<OsString>,
    development: bool,
) -> Option<PathBuf> {
    let abs = |name: &str| {
        env(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if let Some(dir) = abs(CONFIG_DIR_ENV) {
        return Some(dir.join("cli-context.json"));
    }
    if development {
        if let Some(base) = abs(DEV_ACCOUNT_BASE_ENV) {
            return Some(base.join("cli-context.json"));
        }
    }
    abs("HOME").map(|home| home.join("cli-context.json"))
}

/// Reads non-secret saved selection from `cli-context.json` without following symlinks.
pub fn load_saved_context(path: &Path) -> Option<SavedContext> {
    let bytes = read_owned_regular_file(path, MAX_CONTEXT_BYTES)?;
    let doc: Value = serde_json::from_slice(&bytes).ok()?;
    let office_id = doc.get("office_id").and_then(Value::as_str)?;
    if !org_proto::is_uuid_v7(office_id) {
        return None;
    }
    let instance_name = doc
        .get("instance_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    Some(SavedContext {
        instance_name,
        office_id: office_id.to_owned(),
    })
}

fn select_office(
    offices: Vec<DiscoveredOffice>,
    saved_context: Option<&SavedContext>,
    selector: Option<&str>,
) -> Result<DiscoveredOffice, ResolveOfficeError> {
    if let Some(raw) = selector {
        let sel = raw.trim();
        return match_selector(offices, sel);
    }
    if offices.is_empty() {
        return Err(ResolveOfficeError::NotFound { selector: None });
    }
    if offices.len() == 1 {
        return Ok(offices.into_iter().next().expect("one office"));
    }
    if let Some(saved) = saved_context {
        let matched: Vec<DiscoveredOffice> = offices
            .iter()
            .filter(|o| matches_saved_context(o, saved))
            .cloned()
            .collect();
        if matched.len() == 1 {
            return Ok(matched.into_iter().next().expect("one saved office"));
        }
    }
    Err(ResolveOfficeError::Ambiguous {
        selector: None,
        candidates: offices,
    })
}

fn matches_saved_context(office: &DiscoveredOffice, saved: &SavedContext) -> bool {
    if let Some(inst) = saved.instance_name.as_deref() {
        if office.instance_name != inst {
            return false;
        }
    }
    if office.office_id.as_deref() == Some(saved.office_id.as_str()) {
        return true;
    }
    office.office_id.is_none()
        && org_proto::is_uuid_v7(&saved.office_id)
        && office
            .socket_handle
            .eq_ignore_ascii_case(&saved.office_id[saved.office_id.len() - 12..])
}

fn match_selector(
    offices: Vec<DiscoveredOffice>,
    selector: &str,
) -> Result<DiscoveredOffice, ResolveOfficeError> {
    if selector.is_empty() {
        return Err(ResolveOfficeError::NotFound {
            selector: Some(selector.to_owned()),
        });
    }
    let strategies: [fn(&DiscoveredOffice, &str) -> bool; 4] = [
        matches_exact_id,
        matches_suffix,
        matches_office_name,
        matches_instance_name,
    ];
    for predicate in strategies {
        let matched: Vec<DiscoveredOffice> = offices
            .iter()
            .filter(|o| predicate(o, selector))
            .cloned()
            .collect();
        if matched.len() == 1 {
            return Ok(matched.into_iter().next().expect("one match"));
        }
        if matched.len() > 1 {
            return Err(ResolveOfficeError::Ambiguous {
                selector: Some(selector.to_owned()),
                candidates: matched,
            });
        }
    }
    Err(ResolveOfficeError::NotFound {
        selector: Some(selector.to_owned()),
    })
}

fn matches_exact_id(office: &DiscoveredOffice, selector: &str) -> bool {
    if office.office_id.as_deref() == Some(selector) {
        return true;
    }
    office.office_id.is_none()
        && org_proto::is_uuid_v7(selector)
        && office
            .socket_handle
            .eq_ignore_ascii_case(&selector[selector.len() - 12..])
}

fn matches_suffix(office: &DiscoveredOffice, selector: &str) -> bool {
    if office.socket_handle.eq_ignore_ascii_case(selector) {
        return true;
    }
    selector.len() >= 4
        && office.office_id.as_deref().is_some_and(|id| {
            id.to_ascii_lowercase()
                .ends_with(&selector.to_ascii_lowercase())
        })
}

fn matches_office_name(office: &DiscoveredOffice, selector: &str) -> bool {
    office
        .office_name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(selector))
}

fn matches_instance_name(office: &DiscoveredOffice, selector: &str) -> bool {
    office.instance_name.eq_ignore_ascii_case(selector)
}

fn discover_in_runtime(
    runtime_dir: &Path,
    offices_dir: Option<&Path>,
    instance_name: &str,
    instance_id: Option<&str>,
) -> Vec<DiscoveredOffice> {
    let uid = org_platform::posix::effective_uid();
    let sockets_root = office_sockets_dir(runtime_dir);
    if !is_owned_real_dir(runtime_dir, uid) || !is_owned_real_dir(&sockets_root, uid) {
        return Vec::new();
    }
    let meta_by_handle = offices_dir
        .map(|dir| load_office_metadata_map(dir, runtime_dir, uid))
        .unwrap_or_default();
    let Ok(entries) = fs::read_dir(&sockets_root) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Some(handle) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let handle_dir = sockets_root.join(&handle);
        if !is_owned_real_dir(&handle_dir, uid) {
            continue;
        }
        let socket = handle_dir.join(OFFICE_SOCKET_FILE_NAME);
        if !is_live_owned_office_socket(runtime_dir, &socket, uid) {
            continue;
        }
        let meta = meta_by_handle.get(&handle.to_ascii_lowercase());
        found.push(DiscoveredOffice {
            socket,
            runtime_dir: runtime_dir.to_path_buf(),
            socket_handle: handle,
            instance_name: instance_name.to_owned(),
            instance_id: instance_id.map(str::to_owned),
            office_id: meta.map(|(id, _)| id.clone()),
            office_name: meta.map(|(_, name)| name.clone()),
        });
    }
    found
}

fn load_office_metadata_map(
    offices_dir: &Path,
    runtime_dir: &Path,
    uid: u32,
) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    if !is_owned_real_dir(offices_dir, uid) {
        return map;
    }
    let Ok(entries) = fs::read_dir(offices_dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !is_owned_real_dir(&dir, uid) {
            continue;
        }
        let Some(bytes) =
            read_owned_regular_file(&dir.join(OFFICE_FILE_NAME), MAX_OFFICE_FILE_BYTES as u64)
        else {
            continue;
        };
        let Ok(office) = OfficeFile::from_slice(&bytes) else {
            continue;
        };
        let folder_matches = dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| office.names_folder(name));
        if !folder_matches {
            continue;
        }
        let sock_dir = selected_office_socket_dir_in(runtime_dir, &office.office_id)
            .or_else(|| office_socket_dir_in(runtime_dir, &office.office_id));
        if let Some(handle) = sock_dir
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str())
        {
            map.insert(
                handle.to_ascii_lowercase(),
                (office.office_id, office.office_name),
            );
        }
    }
    map
}

fn is_live_owned_office_socket(runtime_dir: &Path, socket: &Path, uid: u32) -> bool {
    let normalized: PathBuf = socket.components().collect();
    if !is_office_socket_in(&normalized, runtime_dir) || check_socket_path(&normalized).is_err() {
        return false;
    }
    let Ok(meta) = fs::symlink_metadata(&normalized) else {
        return false;
    };
    if !meta.file_type().is_socket() || meta.uid() != uid {
        return false;
    }
    UnixStream::connect(&normalized).is_ok()
}

fn is_owned_real_dir(path: &Path, uid: u32) -> bool {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return false;
    }
    let norm: PathBuf = path.components().collect();
    fs::symlink_metadata(&norm).is_ok_and(|meta| meta.file_type().is_dir() && meta.uid() == uid)
}

fn safe_abs_path(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.contains('\0') {
        return None;
    }
    let path = Path::new(raw);
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    Some(path.components().collect())
}

fn read_owned_regular_file(path: &Path, max_bytes: u64) -> Option<Vec<u8>> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let norm: PathBuf = path.components().collect();
    let file = org_platform::fs::open_nofollow_ro(&norm).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.uid() != org_platform::posix::effective_uid() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(max_bytes + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= max_bytes).then_some(bytes)
}

#[cfg(test)]
mod tests;
