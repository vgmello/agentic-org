//! `agentic-org uninstall`: cleanly removes an instance's runtime, socket, setup, locator, and
//! host registry metadata, clearing `host-registry.created` so a subsequent fresh setup is never
//! blocked by a stale marker (AC #1 / FC-022).
//!
//! Fails closed without explicit `--yes`, refuses while `host.lock`, any `office.lock`, the
//! instance root lock, or the registered service is active, and never follows symlinks.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;
use org_cli_shell::output::Cell;
use org_host::setup_flow::cli_context;
use org_platform::service::ServiceStatus;
use serde::Serialize;
use serde_json::Value;

use super::paths::{
    normalize_lstat_path, read_regular_bounded, resolve_instance, try_lock_dir_exclusive,
    try_lock_existing_exclusive, write_atomic_owner_only, InstancePaths, LockProbe,
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

/// Held locks guaranteeing no host or office runs while uninstall removes state.
struct UninstallLocks {
    _host_lock: Option<File>,
    _office_locks: Vec<File>,
    _root_lock: Option<File>,
}

impl Drop for UninstallLocks {
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

    let locks = acquire_uninstall_locks(&paths)?;
    let unregistered_service = unregister_service_if_present(&paths)?;
    let mut removed = Vec::new();
    let mut preserved = Vec::new();

    let registry_created_cleared = remove_entry_nofollow(&paths.registry_created, &mut removed)?;
    remove_entry_nofollow(&paths.instance_config, &mut removed)?;
    remove_host_registry_files(&paths, &mut removed)?;
    remove_runtime_and_logs(&paths, &mut removed)?;

    // Release file/directory locks before removing host.lock and pruning empty directories.
    drop(locks);
    remove_entry_nofollow(&paths.host_lock, &mut removed)?;
    prune_instance_dirs(&paths, &mut removed, &mut preserved)?;

    remove_setup_files(&paths, &mut removed)?;
    let locator_entry_removed = remove_locator_entry(&paths)?;
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

fn acquire_uninstall_locks(paths: &InstancePaths) -> Result<UninstallLocks, CliError> {
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

    Ok(UninstallLocks {
        _host_lock: host_lock,
        _office_locks: office_locks,
        _root_lock: root_lock,
    })
}

fn acquire_office_locks(offices_dir: &Path) -> Result<Vec<File>, CliError> {
    let Ok(entries) = fs::read_dir(offices_dir) else {
        return Ok(Vec::new());
    };
    let mut locks = Vec::new();
    for entry in entries.flatten() {
        let lock_path = entry.path().join("office.lock");
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
    let Ok(manager) = crate::host::service_manager() else {
        return Ok(false);
    };
    let service_id = instance_id.to_ascii_lowercase();
    match manager.status(&service_id) {
        Ok(ServiceStatus::Running { pid }) => Err(CliError::failed(
            "uninstall::service_running",
            format!(
                "registered service for `{}` is still running (pid {pid})",
                paths.name
            ),
        )
        .with_help(STOP_HELP)),
        Ok(ServiceStatus::Stopped | ServiceStatus::Unloaded) => {
            manager.unregister(&service_id).map_err(|e| {
                CliError::failed(
                    "uninstall::service",
                    format!("unregistering service for `{}`: {e}", paths.name),
                )
            })?;
            Ok(true)
        }
        Ok(ServiceStatus::NotRegistered) | Err(_) => Ok(false),
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
        remove_entry_nofollow(&paths.office_home.join(name), removed)?;
    }
    Ok(())
}

fn remove_runtime_and_logs(
    paths: &InstancePaths,
    removed: &mut Vec<String>,
) -> Result<(), CliError> {
    remove_entry_nofollow(&paths.host_socket, removed)?;
    remove_entry_nofollow(&paths.office_home.join("host.sock"), removed)?;
    remove_tree_nofollow(&paths.office_home.join("run"), removed)?;
    remove_tree_nofollow(&paths.logs_dir, removed)?;
    if paths.run_dir != paths.office_home.join("run") && is_instance_temp_dir(paths) {
        remove_tree_nofollow(&paths.run_dir, removed)?;
    }
    Ok(())
}

fn is_instance_temp_dir(paths: &InstancePaths) -> bool {
    let Some(user_temp) = &paths.user_temp else {
        return false;
    };
    let ao_dir = user_temp.join("ao");
    paths.run_dir.starts_with(&ao_dir) && paths.run_dir != ao_dir
}

fn prune_instance_dirs(
    paths: &InstancePaths,
    removed: &mut Vec<String>,
    preserved: &mut Vec<String>,
) -> Result<(), CliError> {
    remove_dir_if_empty(&paths.offices_dir, removed);
    remove_dir_if_empty(&paths.office_home, removed);
    remove_dir_if_empty(&paths.instance_root.join("agents"), removed);
    if !remove_dir_if_empty(&paths.instance_root, removed)
        && fs::symlink_metadata(&paths.instance_root).is_ok()
    {
        preserved.push(paths.instance_root.display().to_string());
    }
    Ok(())
}

fn remove_setup_files(paths: &InstancePaths, removed: &mut Vec<String>) -> Result<(), CliError> {
    remove_entry_nofollow(&paths.setup_record, removed)?;
    remove_entry_nofollow(&paths.setup_dir.join("setup.lock"), removed)?;
    remove_dir_if_empty(&paths.setup_dir, removed);
    Ok(())
}

fn remove_locator_entry(paths: &InstancePaths) -> Result<bool, CliError> {
    if fs::symlink_metadata(&paths.locator_file).is_err() {
        return Ok(false);
    }
    let _lock = match try_lock_existing_exclusive(&paths.locator_lock) {
        Ok(lock) => lock,
        Err(LockProbe::Held) => {
            return Err(CliError::failed(
                "uninstall::locator_busy",
                "instance locator is busy; retry once concurrent setup completes",
            ));
        }
        Err(_) => None,
    };
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

fn remove_entry_nofollow(path: &Path, removed: &mut Vec<String>) -> Result<bool, CliError> {
    let norm = normalize_lstat_path(path);
    match fs::symlink_metadata(&norm) {
        Ok(meta) if meta.is_dir() => Ok(false),
        Ok(_) => {
            fs::remove_file(&norm).map_err(|e| {
                CliError::failed(
                    "uninstall::io",
                    format!("removing `{}`: {e}", norm.display()),
                )
            })?;
            removed.push(norm.display().to_string());
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(CliError::failed(
            "uninstall::io",
            format!("inspecting `{}`: {e}", norm.display()),
        )),
    }
}

fn remove_tree_nofollow(path: &Path, removed: &mut Vec<String>) -> Result<(), CliError> {
    let norm = normalize_lstat_path(path);
    let meta = match fs::symlink_metadata(&norm) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(CliError::failed(
                "uninstall::io",
                format!("inspecting `{}`: {e}", norm.display()),
            ))
        }
    };
    if meta.file_type().is_symlink() {
        fs::remove_file(&norm).map_err(|e| {
            CliError::failed(
                "uninstall::io",
                format!("removing symlink `{}`: {e}", norm.display()),
            )
        })?;
        removed.push(norm.display().to_string());
        return Ok(());
    }
    if meta.is_dir() {
        let entries: Vec<PathBuf> = fs::read_dir(&norm)
            .map_err(|e| {
                CliError::failed(
                    "uninstall::io",
                    format!("listing `{}`: {e}", norm.display()),
                )
            })?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        for child in entries {
            remove_tree_nofollow(&child, removed)?;
        }
        fs::remove_dir(&norm).map_err(|e| {
            CliError::failed(
                "uninstall::io",
                format!("removing `{}`: {e}", norm.display()),
            )
        })?;
        removed.push(norm.display().to_string());
    } else {
        fs::remove_file(&norm).map_err(|e| {
            CliError::failed(
                "uninstall::io",
                format!("removing `{}`: {e}", norm.display()),
            )
        })?;
        removed.push(norm.display().to_string());
    }
    Ok(())
}

fn remove_dir_if_empty(path: &Path, removed: &mut Vec<String>) -> bool {
    let norm = normalize_lstat_path(path);
    let Ok(meta) = fs::symlink_metadata(&norm) else {
        return false;
    };
    if !meta.is_dir() {
        return false;
    }
    if fs::remove_dir(&norm).is_ok() {
        removed.push(norm.display().to_string());
        true
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
