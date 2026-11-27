//! Host, runtime, permissions, registry consistency, release pin, and credential checks for
//! `agentic-org doctor`.

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::doctor_types::DoctorCheck;
use super::paths::{
    check_owner_and_mode, normalize_lstat_path, probe_lock_held, read_regular_bounded,
    InstancePaths, LockProbe,
};

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const MAX_RELEASE_PIN_BYTES: u64 = 16 * 1024;

pub(super) fn check_instance_layout(paths: &InstancePaths, checks: &mut Vec<DoctorCheck>) {
    let has_root =
        fs::symlink_metadata(normalize_lstat_path(&paths.instance_root)).is_ok_and(|m| m.is_dir());
    let has_setup =
        fs::symlink_metadata(normalize_lstat_path(&paths.setup_record)).is_ok_and(|m| m.is_file());
    let has_office_home =
        fs::symlink_metadata(normalize_lstat_path(&paths.office_home)).is_ok_and(|m| m.is_dir());

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
    collect_permission_issues(&dir_targets, 0o700, &mut violations);
    collect_permission_issues(&file_targets, 0o600, &mut violations);
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

fn collect_permission_issues(targets: &[&PathBuf], expected_mode: u32, out: &mut Vec<String>) {
    for path in targets {
        match check_owner_and_mode(path, expected_mode) {
            Ok(Some(msg)) => out.push(msg),
            Ok(None) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => out.push(format!("cannot inspect `{}`: {e}", path.display())),
        }
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

fn registry_fail(msg: impl Into<String>, fix: &str, path: &Path) -> DoctorCheck {
    DoctorCheck::fail(
        "instance.registry_consistency",
        "registry",
        msg,
        Some(fix.to_owned()),
        Some(path.display().to_string()),
    )
}

pub(super) fn validate_marker_content(paths: &InstancePaths) -> Option<DoctorCheck> {
    let bytes = match read_regular_bounded(&paths.registry_created, 256) {
        Ok(b) => b,
        Err(e) => {
            return Some(registry_fail(
                format!("cannot read host-registry.created safely: {e}"),
                "Inspect or clear the marker with `agentic-org uninstall --yes`.",
                &paths.registry_created,
            ));
        }
    };
    if let Some(expected) = paths.instance_id.as_deref() {
        if bytes.as_slice() != expected.as_bytes() {
            return Some(registry_fail(
                "host-registry.created names a different instance ID than instance metadata",
                "Run `agentic-org uninstall --yes` and re-initialize with `agentic-org setup`.",
                &paths.registry_created,
            ));
        }
    }
    None
}

fn validate_sqlite_header(db_path: &Path) -> Option<DoctorCheck> {
    let mut file = match org_platform::fs::open_nofollow_ro(&normalize_lstat_path(db_path)) {
        Ok(f) => f,
        Err(e) => {
            return Some(registry_fail(
                format!("host-registry.db cannot be opened safely: {e}"),
                "Verify host-registry.db is a regular owner-only file, not a symlink.",
                db_path,
            ));
        }
    };
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Some(registry_fail(
            "host-registry.db is not a regular file",
            "Restore a valid host-registry.db or run `agentic-org uninstall --yes`.",
            db_path,
        ));
    }
    let mut hdr = [0u8; 16];
    if file.read_exact(&mut hdr).is_err() || &hdr != SQLITE_MAGIC {
        return Some(registry_fail(
            "host-registry.db header is corrupt or not a SQLite 3 database",
            "Restore host-registry.db from backup or uninstall and re-run setup.",
            db_path,
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
    let norm_root = normalize_lstat_path(&sockets_root);
    match fs::symlink_metadata(&norm_root) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            checks.push(DoctorCheck::fail(
                "runtime.office_sockets",
                "runtime",
                "office sockets directory is not a real directory (symlinks are refused)",
                Some(
                    "Inspect and remove the invalid `o` entry under the runtime directory."
                        .to_owned(),
                ),
                Some(sockets_root.display().to_string()),
            ));
            return;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => return,
        Err(e) => {
            checks.push(DoctorCheck::fail(
                "runtime.office_sockets",
                "runtime",
                format!("cannot inspect office sockets directory: {e}"),
                Some("Verify permissions on the runtime directory.".to_owned()),
                Some(sockets_root.display().to_string()),
            ));
            return;
        }
    }
    let entries = match fs::read_dir(&norm_root) {
        Ok(entries) => entries,
        Err(e) => {
            checks.push(DoctorCheck::fail(
                "runtime.office_sockets",
                "runtime",
                format!("cannot list office sockets directory: {e}"),
                Some("Verify permissions on the runtime `o` directory.".to_owned()),
                Some(sockets_root.display().to_string()),
            ));
            return;
        }
    };
    let host_held = probe_lock_held(&paths.host_lock) == LockProbe::Held;
    let (mut stale, mut active) = (0usize, 0usize);
    for entry in entries.flatten() {
        let dir = normalize_lstat_path(&entry.path());
        if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let sock = normalize_lstat_path(&dir.join("s.sock"));
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

#[derive(Deserialize)]
struct ReleaseCommitMini {
    version: u32,
    ordinal: u64,
    pin: ReleasePinMini,
}

#[derive(Deserialize)]
struct ReleasePinMini {
    release_id: String,
}

fn release_fail(msg: impl Into<String>, fix: &str, path: &Path) -> DoctorCheck {
    DoctorCheck::fail(
        "release.status",
        "release",
        msg,
        Some(fix.to_owned()),
        Some(path.display().to_string()),
    )
}

pub(super) fn check_release_pin(checks: &mut Vec<DoctorCheck>) {
    let Ok(store_dir) = org_host::default_release_store_dir() else {
        return;
    };
    check_release_pin_at(&store_dir, checks);
}

pub(super) fn check_release_pin_at(store_dir: &Path, checks: &mut Vec<DoctorCheck>) {
    let norm_store = normalize_lstat_path(store_dir);
    match fs::symlink_metadata(&norm_store) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let path_s = Some(store_dir.display().to_string());
            let check = if cfg!(debug_assertions) {
                DoctorCheck::pass(
                    "release.status",
                    "release",
                    "no pinned release bundle in release store",
                    path_s,
                )
            } else {
                DoctorCheck::warn(
                    "release.status",
                    "release",
                    "no pinned release bundle in release store",
                    Some(
                        "Pin a signed release with `agentic-org release install <bundle>`."
                            .to_owned(),
                    ),
                    path_s,
                )
            };
            checks.push(check);
            return;
        }
        Err(e) => {
            checks.push(release_fail(
                format!("cannot inspect release store: {e}"),
                "Verify permissions on the release store directory.",
                store_dir,
            ));
            return;
        }
        Ok(meta) if !meta.is_dir() => {
            checks.push(release_fail(
                "release store path is not a real directory (symlinks are refused)",
                "Ensure the release store is an owner-only real directory.",
                store_dir,
            ));
            return;
        }
        Ok(_) => {}
    }
    let pins_dir = norm_store.join("pins");
    let latest = match latest_release_pin_file(&pins_dir) {
        Ok(Some(found)) => found,
        Ok(None) => {
            checks.push(DoctorCheck::warn(
                "release.status",
                "release",
                "release store exists without an active release pin",
                Some(
                    "Install a signed release bundle with `agentic-org release install <bundle>`."
                        .to_owned(),
                ),
                Some(store_dir.display().to_string()),
            ));
            return;
        }
        Err(msg) => {
            checks.push(release_fail(
                msg,
                "Inspect the release store pins directory for corruption or symlinks.",
                &pins_dir,
            ));
            return;
        }
    };
    checks.push(verify_release_pin_record(latest.0, &latest.1));
}

fn latest_release_pin_file(pins_dir: &Path) -> Result<Option<(u64, PathBuf)>, String> {
    let norm_pins = normalize_lstat_path(pins_dir);
    match fs::symlink_metadata(&norm_pins) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot inspect release pins directory: {e}")),
        Ok(meta) if !meta.is_dir() => {
            return Err(
                "release store `pins` entry is not a real directory (symlinks are refused)"
                    .to_owned(),
            );
        }
        Ok(_) => {}
    }
    let entries =
        fs::read_dir(&norm_pins).map_err(|e| format!("cannot list release pins directory: {e}"))?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries {
        let entry = entry.map_err(|e| format!("reading release pins entry: {e}"))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with(".pending-") {
            continue;
        }
        let Some(ordinal) = parse_pin_ordinal(&name) else {
            continue;
        };
        if best.as_ref().is_none_or(|(cur, _)| ordinal > *cur) {
            best = Some((ordinal, entry.path()));
        }
    }
    Ok(best)
}

fn parse_pin_ordinal(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".json")?;
    if stem.is_empty() || stem.len() > 20 || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    stem.parse::<u64>().ok().filter(|&n| n > 0)
}

fn verify_release_pin_record(expected_ordinal: u64, pin_path: &Path) -> DoctorCheck {
    const REINSTALL: &str =
        "Re-install a signed release with `agentic-org release install <bundle>`.";
    let bytes = match read_regular_bounded(pin_path, MAX_RELEASE_PIN_BYTES) {
        Ok(b) => b,
        Err(e) => {
            return release_fail(
                format!("cannot read release pin safely: {e}"),
                "Verify the release pin file is a regular file (not a symlink).",
                pin_path,
            );
        }
    };
    let commit: ReleaseCommitMini = match serde_json::from_slice(&bytes) {
        Ok(c) => c,
        Err(e) => {
            return release_fail(
                format!("release pin record is malformed: {e}"),
                REINSTALL,
                pin_path,
            )
        }
    };
    if commit.version != 1 || commit.ordinal != expected_ordinal || commit.pin.release_id.is_empty()
    {
        return release_fail(
            "release pin record failed version/ordinal validation",
            REINSTALL,
            pin_path,
        );
    }
    DoctorCheck::pass(
        "release.status",
        "release",
        format!(
            "pinned release `{}` present (ordinal {})",
            commit.pin.release_id, commit.ordinal
        ),
        Some(pin_path.display().to_string()),
    )
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
