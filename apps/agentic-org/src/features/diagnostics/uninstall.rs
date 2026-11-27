//! `agentic-org uninstall`: cleanly removes an instance's runtime, socket, setup, locator, and
//! host registry metadata, clearing `host-registry.created` so a subsequent fresh setup is never
//! blocked by a stale marker (AC #1 / FC-022).
//!
//! Fails closed without explicit `--yes`, refuses while `registry.lock`, `host.lock`, any
//! `office.lock`, the instance root lock, or the registered service is active or unsafe, and
//! verifies strict canonical-anchor containment without following symlinks.

use std::fs::{self, File};
use std::io;
use std::path::Path;

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;
use org_cli_shell::output::Cell;
use org_host_setup::cli_context;
use org_platform::service::{ServiceManager, ServiceStatus};
use serde::Serialize;
use serde_json::Value;

use super::paths::{
    normalize_lstat_path, read_regular_bounded, resolve_instance, try_lock_dir_exclusive,
    try_lock_existing_exclusive, write_atomic_owner_only, InstancePaths, LockProbe,
};
use super::uninstall_guard::{
    remove_dir_if_empty_within, remove_entry_within, remove_tree_within, verify_instance_temp_dir,
    verify_parent_within_anchor, verify_uninstall_paths,
};
use super::UninstallArgs;
use crate::error::CliError;
use crate::selection;

const MAX_LOCATOR_BYTES: u64 = 64 * 1024;
const STOP_HELP: &str =
    "Stop the service first (`agentic-org service stop --yes`, or end foreground `agentic-org serve`), then re-run `agentic-org uninstall --yes`.";

#[derive(Debug, Serialize)]
struct UninstallReport {
    instance: String,
    unregistered_service: bool,
    registry_created_cleared: bool,
    locator_entry_removed: bool,
    context_cleared: bool,
    removed_paths: Vec<String>,
    preserved_paths: Vec<String>,
}

/// Held locator lock acquired before destructive actions and kept until locator update finishes.
struct LocatorLock(Option<File>);

impl Drop for LocatorLock {
    fn drop(&mut self) {
        if let Some(f) = &self.0 {
            let _ = f.unlock();
        }
    }
}

/// Held host, office, and instance-root locks guaranteeing nothing runs while uninstall deletes state.
struct InstanceLocks {
    _host_lock: Option<File>,
    _office_locks: Vec<File>,
    _root_lock: Option<File>,
}

impl Drop for InstanceLocks {
    fn drop(&mut self) {
        for f in &self._office_locks {
            let _ = f.unlock();
        }
        if let Some(f) = &self._host_lock {
            let _ = f.unlock();
        }
        if let Some(f) = &self._root_lock {
            let _ = f.unlock();
        }
    }
}

pub(crate) fn run(args: UninstallArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let paths = resolve_instance(target.instance())?;

    if ctx.modes().dry_run {
        return render_dry_run(&paths, ctx);
    }
    if !args.yes.yes {
        return Err(ShellError::ConfirmationRequired {
            action: format!(
                "Uninstall instance `{}` and clear host-registry.created",
                paths.name
            ),
        }
        .into());
    }

    // Pre-flight verify all anchors & intermediate directories before locking or unlinking.
    verify_uninstall_paths(&paths)?;

    // Acquire registry.lock BEFORE any destructive unlink and hold it until locator update completes.
    let locator_lock = acquire_locator_lock(&paths)?;
    let instance_locks = acquire_instance_locks(&paths)?;
    let unregistered_service = unregister_service_if_present(&paths)?;
    let mut removed = Vec::new();
    let mut preserved = Vec::new();

    let registry_created_cleared =
        remove_entry_within(&paths.instance_root, &paths.registry_created, &mut removed)?;
    remove_entry_within(&paths.instance_root, &paths.instance_config, &mut removed)?;
    remove_host_registry_files(&paths, &mut removed)?;
    remove_runtime_and_logs(&paths, &mut removed)?;

    // Release instance locks before removing host.lock and pruning empty instance directories,
    // while keeping locator_lock held through setup cleanup and locator rewrite.
    drop(instance_locks);
    remove_entry_within(&paths.instance_root, &paths.host_lock, &mut removed)?;
    prune_instance_dirs(&paths, &mut removed, &mut preserved)?;

    remove_setup_files(&paths, &mut removed)?;
    let locator_entry_removed = remove_locator_entry(&paths)?;
    drop(locator_lock);
    let context_cleared = clear_matching_context(&paths);

    let report = UninstallReport {
        instance: paths.name.clone(),
        unregistered_service,
        registry_created_cleared,
        locator_entry_removed,
        context_cleared,
        removed_paths: removed,
        preserved_paths: preserved,
    };
    render_report(&report, ctx)
}

fn render_dry_run(paths: &InstancePaths, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let plan = serde_json::json!({
        "action": "uninstall",
        "instance": paths.name,
        "registry_created": paths.registry_created.display().to_string(),
        "instance_root": paths.instance_root.display().to_string(),
        "setup_dir": paths.setup_dir.display().to_string(),
        "run_dir": paths.run_dir.display().to_string(),
    });
    let summary = format!(
        "would uninstall instance `{}` (clearing host-registry.created and runtime/setup metadata)",
        paths.name
    );
    Ok(ctx.render_dry_run(&summary, &plan)?)
}

fn acquire_locator_lock(paths: &InstancePaths) -> Result<LocatorLock, CliError> {
    match try_lock_existing_exclusive(&paths.locator_lock) {
        Ok(lock) => Ok(LocatorLock(lock)),
        Err(LockProbe::Held) => Err(CliError::failed(
            "uninstall::locator_busy",
            "instance locator is busy; retry once concurrent setup completes",
        )),
        Err(LockProbe::Unsafe(why)) => Err(CliError::failed(
            "uninstall::unsafe_lock",
            format!(
                "cannot lock `{}` safely: {why}",
                paths.locator_lock.display()
            ),
        )),
        Err(_) => Err(CliError::failed(
            "uninstall::unsafe_lock",
            format!("cannot lock `{}` safely", paths.locator_lock.display()),
        )),
    }
}

fn acquire_instance_locks(paths: &InstancePaths) -> Result<InstanceLocks, CliError> {
    let host_lock = match try_lock_existing_exclusive(&paths.host_lock) {
        Ok(lock) => lock,
        Err(LockProbe::Held) => {
            return Err(CliError::failed(
                "uninstall::host_running",
                format!(
                    "instance `{}` is currently running (host.lock is held)",
                    paths.name
                ),
            )
            .with_help(STOP_HELP));
        }
        Err(LockProbe::Unsafe(why)) => {
            return Err(CliError::failed(
                "uninstall::unsafe_lock",
                format!("cannot lock `{}` safely: {why}", paths.host_lock.display()),
            ));
        }
        Err(_) => {
            return Err(CliError::failed(
                "uninstall::unsafe_lock",
                format!("cannot lock `{}` safely", paths.host_lock.display()),
            ));
        }
    };

    let office_locks = acquire_office_locks(&paths.offices_dir)?;

    let root_lock = match try_lock_dir_exclusive(&paths.instance_root) {
        Ok(lock) => lock,
        Err(LockProbe::Held) => {
            return Err(CliError::failed(
                "uninstall::host_running",
                format!(
                    "instance `{}` root is currently locked by another agentic-org process",
                    paths.name
                ),
            )
            .with_help(STOP_HELP));
        }
        Err(LockProbe::Unsafe(why)) => {
            return Err(CliError::failed(
                "uninstall::unsafe_root",
                format!(
                    "cannot lock instance root `{}` safely: {why}",
                    paths.instance_root.display()
                ),
            ));
        }
        Err(_) => {
            return Err(CliError::failed(
                "uninstall::unsafe_root",
                format!(
                    "cannot lock instance root `{}` safely",
                    paths.instance_root.display()
                ),
            ));
        }
    };

    Ok(InstanceLocks {
        _host_lock: host_lock,
        _office_locks: office_locks,
        _root_lock: root_lock,
    })
}

pub(super) fn acquire_office_locks(offices_dir: &Path) -> Result<Vec<File>, CliError> {
    let norm = normalize_lstat_path(offices_dir);
    match fs::symlink_metadata(&norm) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(CliError::failed(
                "uninstall::unsafe_lock",
                format!("inspecting offices directory `{}`: {e}", norm.display()),
            ));
        }
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(CliError::failed(
                "uninstall::unsafe_lock",
                format!(
                    "offices directory `{}` must be a real directory (symlinks are refused)",
                    norm.display()
                ),
            ));
        }
        Ok(_) => {}
    }
    let entries = fs::read_dir(&norm).map_err(|e| {
        CliError::failed(
            "uninstall::unsafe_lock",
            format!("listing offices directory `{}`: {e}", norm.display()),
        )
    })?;
    let mut locks = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            CliError::failed(
                "uninstall::unsafe_lock",
                format!("reading entry in `{}`: {e}", norm.display()),
            )
        })?;
        let office_path = normalize_lstat_path(&entry.path());
        let meta = fs::symlink_metadata(&office_path).map_err(|e| {
            CliError::failed(
                "uninstall::unsafe_lock",
                format!("inspecting office entry `{}`: {e}", office_path.display()),
            )
        })?;
        if meta.file_type().is_symlink() {
            return Err(CliError::failed(
                "uninstall::unsafe_lock",
                format!(
                    "refusing symlinked office directory `{}`",
                    office_path.display()
                ),
            ));
        }
        if !meta.is_dir() {
            continue;
        }
        let lock_path = office_path.join("office.lock");
        match try_lock_existing_exclusive(&lock_path) {
            Ok(Some(f)) => locks.push(f),
            Ok(None) => {}
            Err(LockProbe::Held) => {
                return Err(CliError::failed(
                    "uninstall::office_running",
                    format!(
                        "an office server is still running ({})",
                        lock_path.display()
                    ),
                )
                .with_help(STOP_HELP));
            }
            Err(LockProbe::Unsafe(why)) => {
                return Err(CliError::failed(
                    "uninstall::unsafe_lock",
                    format!("cannot lock `{}` safely: {why}", lock_path.display()),
                ));
            }
            Err(_) => {
                return Err(CliError::failed(
                    "uninstall::unsafe_lock",
                    format!("cannot lock `{}` safely", lock_path.display()),
                ));
            }
        }
    }
    Ok(locks)
}

fn unregister_service_if_present(paths: &InstancePaths) -> Result<bool, CliError> {
    let Some(instance_id) = paths.instance_id.as_deref() else {
        return Ok(false);
    };
    if paths.service_mode.as_deref() == Some("foreground") {
        return Ok(false);
    }
    let manager = crate::host::service_manager().map_err(|e| {
        CliError::failed(
            "uninstall::service",
            format!("accessing service manager for `{}`: {e}", paths.name),
        )
    })?;
    unregister_service_with(&paths.name, instance_id, &*manager)
}

pub(super) fn unregister_service_with(
    instance_name: &str,
    instance_id: &str,
    manager: &dyn ServiceManager,
) -> Result<bool, CliError> {
    let service_id = instance_id.to_ascii_lowercase();
    match manager.status(&service_id) {
        Ok(ServiceStatus::Running { pid }) => Err(CliError::failed(
            "uninstall::service_running",
            format!("registered service for `{instance_name}` is still running (pid {pid})"),
        )
        .with_help(STOP_HELP)),
        Ok(ServiceStatus::Stopped | ServiceStatus::Unloaded) => {
            manager.unregister(&service_id).map_err(|e| {
                CliError::failed(
                    "uninstall::service",
                    format!("unregistering service for `{instance_name}`: {e}"),
                )
            })?;
            Ok(true)
        }
        Ok(ServiceStatus::NotRegistered) => Ok(false),
        Err(e) => Err(CliError::failed(
            "uninstall::service",
            format!("checking service status for `{instance_name}`: {e}"),
        )),
    }
}

fn remove_host_registry_files(
    paths: &InstancePaths,
    removed: &mut Vec<String>,
) -> Result<(), CliError> {
    for suffix in [
        "",
        "-wal",
        "-shm",
        "-journal",
        ".tmp",
        ".tmp-wal",
        ".tmp-shm",
        ".tmp-journal",
    ] {
        let name = format!("host-registry.db{suffix}");
        remove_entry_within(&paths.instance_root, &paths.office_home.join(name), removed)?;
    }
    Ok(())
}

fn remove_runtime_and_logs(
    paths: &InstancePaths,
    removed: &mut Vec<String>,
) -> Result<(), CliError> {
    let fallback_run = paths.office_home.join("run");
    if paths.run_dir == fallback_run {
        remove_entry_within(&paths.instance_root, &paths.host_socket, removed)?;
    } else if let Some((canon_ao, canon_run)) = verify_instance_temp_dir(paths)? {
        if let Some(sock_name) = paths.host_socket.file_name() {
            remove_entry_within(&canon_ao, &canon_run.join(sock_name), removed)?;
        }
        remove_tree_within(&canon_ao, &canon_run, removed)?;
    }
    remove_entry_within(
        &paths.instance_root,
        &paths.office_home.join("host.sock"),
        removed,
    )?;
    remove_tree_within(&paths.instance_root, &fallback_run, removed)?;
    remove_tree_within(&paths.instance_root, &paths.logs_dir, removed)?;
    Ok(())
}

fn prune_instance_dirs(
    paths: &InstancePaths,
    removed: &mut Vec<String>,
    preserved: &mut Vec<String>,
) -> Result<(), CliError> {
    remove_dir_if_empty_within(&paths.instance_root, &paths.offices_dir, removed)?;
    remove_dir_if_empty_within(&paths.instance_root, &paths.office_home, removed)?;
    remove_dir_if_empty_within(
        &paths.instance_root,
        &paths.instance_root.join("agents"),
        removed,
    )?;
    if !remove_dir_if_empty_within(&paths.instance_root, &paths.instance_root, removed)?
        && fs::symlink_metadata(normalize_lstat_path(&paths.instance_root)).is_ok()
    {
        preserved.push(paths.instance_root.display().to_string());
    }
    Ok(())
}

fn remove_setup_files(paths: &InstancePaths, removed: &mut Vec<String>) -> Result<(), CliError> {
    remove_entry_within(&paths.account_base, &paths.setup_record, removed)?;
    remove_entry_within(
        &paths.account_base,
        &paths.setup_dir.join("setup.lock"),
        removed,
    )?;
    remove_dir_if_empty_within(&paths.account_base, &paths.setup_dir, removed)?;
    Ok(())
}

fn remove_locator_entry(paths: &InstancePaths) -> Result<bool, CliError> {
    if !verify_parent_within_anchor(&paths.account_base, &paths.locator_file)? {
        return Ok(false);
    }
    if fs::symlink_metadata(normalize_lstat_path(&paths.locator_file)).is_err() {
        return Ok(false);
    }
    let bytes = read_regular_bounded(&paths.locator_file, MAX_LOCATOR_BYTES).map_err(|e| {
        CliError::failed(
            "uninstall::locator",
            format!("reading `{}`: {e}", paths.locator_file.display()),
        )
    })?;
    let mut doc: Value = serde_json::from_slice(&bytes).map_err(|e| {
        CliError::failed(
            "uninstall::locator",
            format!("parsing `{}`: {e}", paths.locator_file.display()),
        )
    })?;
    let Some(entries) = doc.get_mut("entries").and_then(Value::as_array_mut) else {
        return Ok(false);
    };
    let before = entries.len();
    entries.retain(|entry| {
        entry
            .get("binding")
            .and_then(|b| b.get("name"))
            .and_then(Value::as_str)
            != Some(paths.name.as_str())
    });
    if entries.len() == before {
        return Ok(false);
    }
    let updated =
        serde_json::to_vec(&doc).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    write_atomic_owner_only(&paths.locator_file, &updated)
        .map_err(|e| CliError::failed("uninstall::locator", e))?;
    Ok(true)
}

fn clear_matching_context(paths: &InstancePaths) -> bool {
    let Ok(ctx_path) = selection::context_path() else {
        return false;
    };
    let Ok(Some(saved)) = cli_context::load_from(&ctx_path) else {
        return false;
    };
    let name_matches = saved.instance_name.as_deref().unwrap_or("default") == paths.name;
    let id_matches = paths
        .instance_id
        .as_deref()
        .is_some_and(|id| id == saved.instance_id);
    if name_matches || id_matches {
        cli_context::clear_at(&ctx_path).unwrap_or(false)
    } else {
        false
    }
}

fn render_report(report: &UninstallReport, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let json =
        serde_json::to_value(report).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let fields = [
        ("instance", Cell::name(&report.instance)),
        (
            "registry_created_cleared",
            Cell::plain(report.registry_created_cleared.to_string()),
        ),
        (
            "unregistered_service",
            Cell::plain(report.unregistered_service.to_string()),
        ),
        (
            "removed_entries",
            Cell::plain(report.removed_paths.len().to_string()),
        ),
    ];
    ctx.render_detail(&report.instance, &fields, &json)?;
    if !report.preserved_paths.is_empty() {
        ctx.warn(&format!(
            "Preserved non-empty directory(ies) containing durable data: {}",
            report.preserved_paths.join(", ")
        ));
    }
    ctx.success(&format!(
        "Uninstalled instance `{}` cleanly (host-registry.created cleared: {}).",
        report.instance, report.registry_created_cleared
    ));
    Ok(())
}
