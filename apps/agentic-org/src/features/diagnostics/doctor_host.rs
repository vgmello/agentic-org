//! Host, runtime, permissions, registry consistency, release pin, and credential checks for
//! `agentic-org doctor`.

use std::fs;
use std::io::Read;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;

use super::doctor::{CheckStatus, DoctorCheck};
use super::paths::{
    check_owner_and_mode, normalize_lstat_path, probe_lock_held, read_regular_bounded,
    InstancePaths, LockProbe,
};

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

pub(super) fn check_instance_layout(paths: &InstancePaths, checks: &mut Vec<DoctorCheck>) {
    let has_root = fs::symlink_metadata(&paths.instance_root).is_ok_and(|m| m.is_dir());
    let has_setup = fs::symlink_metadata(&paths.setup_record).is_ok_and(|m| m.is_file());
    let has_office_home = fs::symlink_metadata(&paths.office_home).is_ok_and(|m| m.is_dir());

    if !has_root && !has_setup {
        checks.push(DoctorCheck::fail(
            "instance.layout",
            "instance",
            format!("instance `{}` is not set up", paths.name),
            Some("Run `agentic-org setup` to initialize this instance.".to_owned()),
            Some(paths.instance_root.display().to_string()),
        ));
        return;
    }
    if has_root && has_office_home {
        checks.push(DoctorCheck::pass(
            "instance.layout",
            "instance",
            format!(
                "instance `{}` layout present{}",
                paths.name,
                paths
                    .instance_id
                    .as_deref()
                    .map(|id| format!(" (id {id})"))
                    .unwrap_or_default()
            ),
            Some(paths.instance_root.display().to_string()),
        ));
    } else {
        checks.push(DoctorCheck::fail(
            "instance.layout",
            "instance",
            format!("instance `{}` layout is incomplete", paths.name),
            Some(
                "Re-run `agentic-org setup` or cleanly reset with `agentic-org uninstall --yes`."
                    .to_owned(),
            ),
            Some(paths.instance_root.display().to_string()),
        ));
    }
}

pub(super) fn check_permissions(paths: &InstancePaths, checks: &mut Vec<DoctorCheck>) {
    let dir_targets = [
        &paths.instance_root,
        &paths.office_home,
        &paths.logs_dir,
        &paths.offices_dir,
        &paths.setup_dir,
        &paths.locator_dir,
    ];
    let file_targets = [
        &paths.setup_record,
        &paths.instance_config,
        &paths.registry_created,
        &paths.host_registry_db,
        &paths.host_lock,
    ];
    let mut violations = Vec::new();
    for dir in dir_targets {
        if let Ok(Some(msg)) = check_owner_and_mode(dir, 0o700) {
            violations.push(msg);
        }
    }
    for file in file_targets {
        if let Ok(Some(msg)) = check_owner_and_mode(file, 0o600) {
            violations.push(msg);
        }
    }
    if violations.is_empty() {
        checks.push(DoctorCheck::pass(
            "instance.permissions",
            "security",
            "instance directories (0700) and sensitive files (0600) have owner-only permissions",
            Some(paths.instance_root.display().to_string()),
        ));
    } else {
        checks.push(DoctorCheck::fail(
            "instance.permissions",
            "security",
            format!(
                "{} permission violation(s): {}",
                violations.len(),
                violations.join("; ")
            ),
            Some("Tighten permissions with `chmod 700` on instance directories and `chmod 600` on sensitive files.".to_owned()),
            Some(paths.instance_root.display().to_string()),
        ));
    }
}

pub(super) fn check_registry_consistency(paths: &InstancePaths, checks: &mut Vec<DoctorCheck>) {
    let marker_exists = fs::symlink_metadata(normalize_lstat_path(&paths.registry_created)).is_ok();
    let db_exists = fs::symlink_metadata(normalize_lstat_path(&paths.host_registry_db)).is_ok();

    if marker_exists && !db_exists {
        checks.push(DoctorCheck::fail(
            "instance.registry_consistency",
            "registry",
            "host-registry.created marker exists but host-registry.db is missing",
            Some("Restore host-registry.db from backup, or run `agentic-org uninstall --yes` before a fresh `agentic-org setup`.".to_owned()),
            Some(paths.registry_created.display().to_string()),
        ));
        return;
    }
    if marker_exists {
        if let Some(issue) = validate_marker_content(paths) {
            checks.push(issue);
            return;
        }
    }
    if db_exists {
        if let Some(issue) = validate_sqlite_header(&paths.host_registry_db) {
            checks.push(issue);
            return;
        }
        if !marker_exists {
            checks.push(DoctorCheck::warn(
                "instance.registry_consistency",
                "registry",
                "host-registry.db exists without host-registry.created marker",
                Some(
                    "Start the host service once so it repairs the host-registry.created marker."
                        .to_owned(),
                ),
                Some(paths.host_registry_db.display().to_string()),
            ));
            return;
        }
        checks.push(DoctorCheck::pass(
            "instance.registry_consistency",
            "registry",
            "host-registry.db and host-registry.created marker are consistent",
            Some(paths.host_registry_db.display().to_string()),
        ));
        return;
    }
    checks.push(DoctorCheck::warn(
        "instance.registry_consistency",
        "registry",
        "host-registry.db has not been created yet",
        Some(
            "Start the instance service (`agentic-org service start`) to initialize the host registry."
                .to_owned(),
        ),
        Some(paths.host_registry_db.display().to_string()),
    ));
}

fn validate_marker_content(paths: &InstancePaths) -> Option<DoctorCheck> {
    let bytes = match read_regular_bounded(&paths.registry_created, 256) {
        Ok(b) => b,
        Err(e) => {
            return Some(DoctorCheck::fail(
                "instance.registry_consistency",
                "registry",
                format!("cannot read host-registry.created safely: {e}"),
                Some("Inspect or clear the marker with `agentic-org uninstall --yes`.".to_owned()),
                Some(paths.registry_created.display().to_string()),
            ));
        }
    };
    let text = String::from_utf8_lossy(&bytes).trim().to_owned();
    if let Some(expected) = paths.instance_id.as_deref() {
        if text != expected {
            return Some(DoctorCheck::fail(
                "instance.registry_consistency",
                "registry",
                "host-registry.created names a different instance ID than instance metadata",
                Some(
                    "Run `agentic-org uninstall --yes` and re-initialize with `agentic-org setup`."
                        .to_owned(),
                ),
                Some(paths.registry_created.display().to_string()),
            ));
        }
    }
    None
}

fn validate_sqlite_header(db_path: &Path) -> Option<DoctorCheck> {
    let mut file = match org_platform::fs::open_nofollow_ro(&normalize_lstat_path(db_path)) {
        Ok(f) => f,
        Err(e) => {
            return Some(DoctorCheck::fail(
                "instance.registry_consistency",
                "registry",
                format!("host-registry.db cannot be opened safely: {e}"),
                Some(
                    "Verify host-registry.db is a regular owner-only file, not a symlink."
                        .to_owned(),
                ),
                Some(db_path.display().to_string()),
            ));
        }
    };
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Some(DoctorCheck::fail(
            "instance.registry_consistency",
            "registry",
            "host-registry.db is not a regular file",
            Some(
                "Restore a valid host-registry.db or run `agentic-org uninstall --yes`.".to_owned(),
            ),
            Some(db_path.display().to_string()),
        ));
    }
    let mut hdr = [0u8; 16];
    if file.read_exact(&mut hdr).is_err() || &hdr != SQLITE_MAGIC {
        return Some(DoctorCheck::fail(
            "instance.registry_consistency",
            "registry",
            "host-registry.db header is corrupt or not a SQLite 3 database",
            Some("Restore host-registry.db from backup or uninstall and re-run setup.".to_owned()),
            Some(db_path.display().to_string()),
        ));
    }
    None
}

pub(super) fn check_sockets(paths: &InstancePaths, checks: &mut Vec<DoctorCheck>) {
    let lock_state = probe_lock_held(&paths.host_lock);
    let sock_meta = fs::symlink_metadata(normalize_lstat_path(&paths.host_socket));
    let sock_is_socket = sock_meta.as_ref().is_ok_and(|m| m.file_type().is_socket());
    let sock_exists = sock_meta.is_ok();

    match (lock_state, sock_exists, sock_is_socket) {
        (LockProbe::Unsafe(why), _, _) => checks.push(DoctorCheck::fail(
            "runtime.host_socket",
            "runtime",
            format!("host.lock is unsafe or unreadable: {why}"),
            Some("Inspect permissions and symlinks on host.lock.".to_owned()),
            Some(paths.host_lock.display().to_string()),
        )),
        (LockProbe::Held, true, true) => checks.push(DoctorCheck::pass(
            "runtime.host_socket",
            "runtime",
            "host.lock is active and host.sock is present",
            Some(paths.host_socket.display().to_string()),
        )),
        (LockProbe::Held, false, _) => checks.push(DoctorCheck::fail(
            "runtime.host_socket",
            "runtime",
            "host.lock is held but host.sock is missing",
            Some("Restart the service (`agentic-org service stop --yes && agentic-org service start`).".to_owned()),
            Some(paths.host_socket.display().to_string()),
        )),
        (_, true, false) => checks.push(DoctorCheck::fail(
            "runtime.host_socket",
            "runtime",
            "host.sock exists but is not a Unix domain socket",
            Some("Stop the host and remove the invalid host.sock entry.".to_owned()),
            Some(paths.host_socket.display().to_string()),
        )),
        (LockProbe::Free | LockProbe::NotPresent, true, true) => checks.push(DoctorCheck::warn(
            "runtime.host_socket",
            "runtime",
            "stale host.sock exists while host.lock is not held",
            Some("Start the host service (`agentic-org service start`) or clean up with `agentic-org uninstall --yes`.".to_owned()),
            Some(paths.host_socket.display().to_string()),
        )),
        (LockProbe::Free | LockProbe::NotPresent, false, _) => {
            let service_note = crate::host::service_view(Some(&paths.name))
                .map(|v| format!(" (service: {})", v.summary))
                .unwrap_or_default();
            checks.push(DoctorCheck::warn(
                "runtime.host_socket",
                "runtime",
                format!("host is not running{service_note}"),
                Some(
                    "Start the instance with `agentic-org service start` or `agentic-org serve`."
                        .to_owned(),
                ),
                Some(paths.host_socket.display().to_string()),
            ));
        }
    }
    check_office_sockets(paths, checks);
}

fn check_office_sockets(paths: &InstancePaths, checks: &mut Vec<DoctorCheck>) {
    let sockets_root = paths.run_dir.join("o");
    let Ok(entries) = fs::read_dir(&sockets_root) else {
        return;
    };
    let host_held = probe_lock_held(&paths.host_lock) == LockProbe::Held;
    let mut stale = 0usize;
    let mut active = 0usize;
    for entry in entries.flatten() {
        let sock = entry.path().join("s.sock");
        if fs::symlink_metadata(&sock).is_ok_and(|m| m.file_type().is_socket()) {
            if host_held {
                active += 1;
            } else {
                stale += 1;
            }
        }
    }
    if stale > 0 {
        checks.push(DoctorCheck::warn(
            "runtime.office_sockets",
            "runtime",
            format!("{stale} stale office socket(s) present while host is stopped"),
            Some(
                "Start the service to reclaim sockets, or run `agentic-org uninstall --yes`."
                    .to_owned(),
            ),
            Some(sockets_root.display().to_string()),
        ));
    } else if active > 0 {
        checks.push(DoctorCheck::pass(
            "runtime.office_sockets",
            "runtime",
            format!("{active} active office socket(s) present"),
            Some(sockets_root.display().to_string()),
        ));
    }
}

pub(super) fn check_release_pin(checks: &mut Vec<DoctorCheck>) {
    let Ok(store_dir) = org_host::default_release_store_dir() else {
        return;
    };
    if !store_dir.exists() {
        let status = if cfg!(debug_assertions) {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        };
        checks.push(DoctorCheck {
            id: "release.status".to_owned(),
            category: "release".to_owned(),
            status,
            message: "no pinned release bundle in release store".to_owned(),
            remediation: (!cfg!(debug_assertions)).then(|| {
                "Pin a signed release with `agentic-org release install <bundle>`.".to_owned()
            }),
            path: Some(store_dir.display().to_string()),
        });
        return;
    }
    let pin_path = store_dir.join("active.v1.json");
    match read_regular_bounded(&pin_path, 64 * 1024)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    {
        Some(v) => {
            let release_id = v
                .get("release_id")
                .and_then(|r| r.as_str())
                .unwrap_or("unknown");
            checks.push(DoctorCheck::pass(
                "release.status",
                "release",
                format!("pinned release `{release_id}` present"),
                Some(pin_path.display().to_string()),
            ));
        }
        None => checks.push(DoctorCheck::warn(
            "release.status",
            "release",
            "release store exists without an active release pin",
            Some(
                "Install a signed release bundle with `agentic-org release install <bundle>`."
                    .to_owned(),
            ),
            Some(store_dir.display().to_string()),
        )),
    }
}

pub(super) fn check_credentials_dir(paths: &InstancePaths, checks: &mut Vec<DoctorCheck>) {
    let Some(cred_dir) = &paths.credentials_dir else {
        return;
    };
    match check_owner_and_mode(cred_dir, 0o700) {
        Ok(None) => checks.push(DoctorCheck::pass(
            "credentials.store",
            "security",
            "recorded credential directory exists with owner-only (0700) permissions",
            Some(cred_dir.display().to_string()),
        )),
        Ok(Some(msg)) => checks.push(DoctorCheck::fail(
            "credentials.store",
            "security",
            msg,
            Some(
                "Tighten the credential directory to mode 0700 owned by the service user."
                    .to_owned(),
            ),
            Some(cred_dir.display().to_string()),
        )),
        Err(e) => checks.push(DoctorCheck::fail(
            "credentials.store",
            "security",
            format!("credential directory is unavailable: {e}"),
            Some("Verify the recorded credential directory is mounted and accessible.".to_owned()),
            Some(cred_dir.display().to_string()),
        )),
    }
}
