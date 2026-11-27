//! `agentic-org instance rebind [<name>]`: explicit confirmation-bound rebind of a stopped
//! instance root whose filesystem identity (device/inode) changed after a copy or restore
//! (`FC-148`).

use std::ffi::OsStr;
use std::fs::{self, File, TryLockError};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;
use org_cli_shell::output::Cell;
use org_cli_shell::prompt::require_confirmation;
use org_host::instance::registry::Registry;
use org_host::instance::{Error as InstanceError, RebindPreview};
use org_platform::service::{ServiceManager, ServiceStatus};
use serde::{Deserialize, Serialize};

use crate::args::{InstanceArgs, YesArgs};
use crate::error::CliError;
use crate::selection;

const MAX_SETUP_BYTES: u64 = 64 * 1024;
const STOP_HELP: &str = "Stop the running host and offices first (`agentic-org service stop --yes`), then re-run `agentic-org instance rebind --yes`.";

pub(super) const EXAMPLES: &str = "Examples:
  agentic-org instance rebind --dry-run
  agentic-org instance rebind lab --yes
  agentic-org instance rebind --name lab --yes --json";

#[derive(Args, Debug)]
pub(super) struct RebindArgs {
    /// Instance name to rebind (optional positional alternative to `--name` / `--instance`)
    #[arg(value_name = "NAME", conflicts_with = "name")]
    positional_name: Option<String>,
    #[command(flatten)]
    instance: InstanceArgs,
    #[command(flatten)]
    pub(super) yes: YesArgs,
}

#[derive(Debug, Serialize)]
struct RebindPlan {
    action: &'static str,
    instance: String,
    instance_id: String,
    root: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct RebindReceipt {
    rebound: bool,
    instance: String,
    instance_id: String,
    root: String,
    description: String,
}

#[derive(Deserialize)]
struct SetupServiceMini {
    input: SetupInputServiceMini,
}

#[derive(Deserialize)]
struct SetupInputServiceMini {
    #[serde(default)]
    service: Option<String>,
}

/// Held exclusive `host.lock` and `office.lock` guards kept alive across `Registry::rebind`
/// mutation so no host or office can start between confirmation and registry publication.
#[derive(Default)]
struct RebindLocks {
    host_lock: Option<File>,
    office_locks: Vec<File>,
}

impl Drop for RebindLocks {
    fn drop(&mut self) {
        for f in &self.office_locks {
            let _ = f.unlock();
        }
        if let Some(f) = &self.host_lock {
            let _ = f.unlock();
        }
    }
}

pub(super) fn run(args: RebindArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let explicit_name = args
        .positional_name
        .as_deref()
        .or(args.instance.name.as_deref());
    let target = selection::select(explicit_name, None)?;
    let paths = org_host::human::paths(target.instance())
        .map_err(|e| CliError::failed("instance::paths", e))?;
    let registry = Registry::open(&paths.locator, &paths.user_temp).map_err(map_instance_error)?;

    let mut observed: Option<RebindPlan> = None;
    let mut confirm_error: Option<CliError> = None;
    let mut held_locks: Option<RebindLocks> = None;
    let dry_run = ctx.modes().dry_run;
    let _ = args.yes;

    let result = registry.rebind(target.instance(), |preview: &RebindPreview| {
        let plan = build_plan(preview);
        if dry_run {
            observed = Some(plan);
            return Err(InstanceError::RebindUnconfirmed);
        }
        if let Err(err) = require_confirmation(ctx, &plan.description) {
            confirm_error = Some(CliError::from(err));
            return Err(InstanceError::RebindUnconfirmed);
        }
        if let Err(err) = ensure_service_stopped(&paths.base, preview.name(), preview.instance_id())
        {
            confirm_error = Some(err);
            return Err(InstanceError::RebindUnconfirmed);
        }
        match acquire_instance_locks(preview.root()) {
            Ok(locks) => held_locks = Some(locks),
            Err(err) => {
                confirm_error = Some(err);
                return Err(InstanceError::RebindUnconfirmed);
            }
        }
        observed = Some(plan);
        Ok(())
    });
    drop(held_locks);

    if dry_run {
        if let Some(plan) = observed {
            let summary = format!("would {}", plan.description);
            return Ok(ctx.render_dry_run(&summary, &plan)?);
        }
    }
    if let Some(err) = confirm_error {
        return Err(err);
    }
    result.map_err(map_instance_error)?;
    let plan = observed.ok_or_else(|| {
        CliError::failed(
            "instance::rebind",
            "rebind completed without a confirmation preview",
        )
    })?;
    let receipt = RebindReceipt {
        rebound: true,
        instance: plan.instance,
        instance_id: plan.instance_id,
        root: plan.root,
        description: plan.description,
    };
    let fields = [
        ("instance", Cell::id(&receipt.instance)),
        ("instance_id", Cell::id(&receipt.instance_id)),
        ("root", Cell::plain(&receipt.root)),
        ("rebound", Cell::plain("true")),
    ];
    let json = serde_json::to_value(&receipt)
        .map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    ctx.render_detail(&receipt.instance, &fields, &json)?;
    ctx.success(&format!(
        "Rebound instance `{}` ({}) at {}.",
        receipt.instance, receipt.instance_id, receipt.root
    ));
    Ok(())
}

fn build_plan(preview: &RebindPreview) -> RebindPlan {
    RebindPlan {
        action: "instance_rebind",
        instance: preview.name().to_owned(),
        instance_id: preview.instance_id().to_owned(),
        root: preview.root().display().to_string(),
        description: preview.description(),
    }
}

/// Strips trailing `/` and `/.` so `symlink_metadata` inspects a symlink itself rather than
/// following it.
fn normalize_lstat_path(path: &Path) -> PathBuf {
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

fn is_foreground_service(account_base: &Path, instance_name: &str) -> bool {
    let record_path = normalize_lstat_path(
        &account_base
            .join("setup")
            .join(instance_name)
            .join("setup.v1.json"),
    );
    let Ok(file) = org_platform::fs::open_nofollow_ro(&record_path) else {
        return false;
    };
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return false;
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_SETUP_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_SETUP_BYTES
    {
        return false;
    }
    serde_json::from_slice::<SetupServiceMini>(&bytes)
        .ok()
        .and_then(|rec| rec.input.service)
        .as_deref()
        == Some("foreground")
}

/// Verifies the instance's registered OS service is not running before rebind mutates state.
fn ensure_service_stopped(
    account_base: &Path,
    instance_name: &str,
    instance_id: &str,
) -> Result<(), CliError> {
    if is_foreground_service(account_base, instance_name) {
        return Ok(());
    }
    let manager = crate::host::service_manager().map_err(|e| {
        CliError::failed(
            "instance::service",
            format!(
                "cannot access service manager for `{instance_name}`: {e}; nothing was changed"
            ),
        )
    })?;
    ensure_service_stopped_with(instance_name, instance_id, &*manager)
}

fn ensure_service_stopped_with(
    instance_name: &str,
    instance_id: &str,
    manager: &dyn ServiceManager,
) -> Result<(), CliError> {
    let service_id = instance_id.to_ascii_lowercase();
    match manager.status(&service_id) {
        Ok(ServiceStatus::Running { pid }) => Err(CliError::failed(
            "instance::busy",
            format!(
                "registered service for `{instance_name}` is currently running (pid {pid}); nothing was changed"
            ),
        )
        .with_help(STOP_HELP)),
        Ok(ServiceStatus::Stopped | ServiceStatus::Unloaded | ServiceStatus::NotRegistered) => {
            Ok(())
        }
        Err(e) => Err(CliError::failed(
            "instance::service",
            format!("cannot verify service status for `{instance_name}`: {e}; nothing was changed"),
        )),
    }
}

fn verify_real_dir_or_absent(path: &Path, label: &str) -> Result<bool, CliError> {
    let norm = normalize_lstat_path(path);
    match fs::symlink_metadata(&norm) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(CliError::failed(
            "instance::unsafe_lock",
            format!(
                "cannot inspect {label} `{}`: {e}; nothing was changed",
                norm.display()
            ),
        )),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => Err(CliError::failed(
            "instance::unsafe_lock",
            format!(
                "{label} `{}` must be a real directory (symlinks are refused); nothing was changed",
                norm.display()
            ),
        )),
        Ok(_) => Ok(true),
    }
}

/// Validates `instance_root`, `office-state`, `offices`, and every office entry without following
/// symlinks (failing closed on all non-`NotFound` errors), and acquires exclusive `host.lock` and
/// `office.lock` guards held across `Registry::rebind`.
fn acquire_instance_locks(instance_root: &Path) -> Result<RebindLocks, CliError> {
    let root_norm = normalize_lstat_path(instance_root);
    match fs::symlink_metadata(&root_norm) {
        Ok(meta) if !meta.file_type().is_symlink() && meta.is_dir() => {}
        Ok(_) => {
            return Err(CliError::failed(
                "instance::unsafe_lock",
                format!(
                    "instance root `{}` must be a real directory (symlinks are refused); nothing was changed",
                    root_norm.display()
                ),
            ));
        }
        Err(e) => {
            return Err(CliError::failed(
                "instance::unsafe_lock",
                format!(
                    "cannot inspect instance root `{}`: {e}; nothing was changed",
                    root_norm.display()
                ),
            ));
        }
    }

    let office_home = root_norm.join("office-state");
    if !verify_real_dir_or_absent(&office_home, "office-state directory")? {
        return Ok(RebindLocks::default());
    }

    let mut locks = RebindLocks {
        host_lock: try_lock_existing_exclusive(&office_home.join("host.lock"), "host")?,
        office_locks: Vec::new(),
    };

    let offices_dir = normalize_lstat_path(&office_home.join("offices"));
    if !verify_real_dir_or_absent(&offices_dir, "offices directory")? {
        return Ok(locks);
    }

    let entries = fs::read_dir(&offices_dir).map_err(|e| {
        CliError::failed(
            "instance::unsafe_lock",
            format!(
                "cannot list offices directory `{}`: {e}; nothing was changed",
                offices_dir.display()
            ),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            CliError::failed(
                "instance::unsafe_lock",
                format!(
                    "cannot read entry in offices directory `{}`: {e}; nothing was changed",
                    offices_dir.display()
                ),
            )
        })?;
        let office_path = normalize_lstat_path(&entry.path());
        let meta = fs::symlink_metadata(&office_path).map_err(|e| {
            CliError::failed(
                "instance::unsafe_lock",
                format!(
                    "cannot inspect office entry `{}`: {e}; nothing was changed",
                    office_path.display()
                ),
            )
        })?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(CliError::failed(
                "instance::unsafe_lock",
                format!(
                    "office entry `{}` must be a real directory (symlinks and non-directories are refused); nothing was changed",
                    office_path.display()
                ),
            ));
        }
        if let Some(lock) = try_lock_existing_exclusive(&office_path.join("office.lock"), "office")?
        {
            locks.office_locks.push(lock);
        }
    }
    Ok(locks)
}

fn try_lock_existing_exclusive(lock_path: &Path, label: &str) -> Result<Option<File>, CliError> {
    let norm = normalize_lstat_path(lock_path);
    let file = match org_platform::fs::open_nofollow_ro(&norm) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(CliError::failed(
                "instance::unsafe_lock",
                format!(
                    "cannot inspect {label} lock `{}`: {e}; nothing was changed",
                    norm.display()
                ),
            ))
        }
    };
    let meta = file.metadata().map_err(|e| {
        CliError::failed(
            "instance::unsafe_lock",
            format!(
                "cannot read {label} lock metadata `{}`: {e}; nothing was changed",
                norm.display()
            ),
        )
    })?;
    if !meta.is_file() {
        return Err(CliError::failed(
            "instance::unsafe_lock",
            format!(
                "{label} lock `{}` is not a regular file; nothing was changed",
                norm.display()
            ),
        ));
    }
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Err(CliError::failed(
            "instance::busy",
            format!(
                "instance {label} is currently running (`{}` is locked); nothing was changed",
                norm.display()
            ),
        )
        .with_help(STOP_HELP)),
        Err(TryLockError::Error(e)) => Err(CliError::failed(
            "instance::unsafe_lock",
            format!(
                "cannot lock {label} lock `{}`: {e}; nothing was changed",
                norm.display()
            ),
        )),
    }
}

fn map_instance_error(error: InstanceError) -> CliError {
    let code = match &error {
        InstanceError::Invalid(_) => return CliError::usage(error.to_string()),
        InstanceError::RebindUnconfirmed => return CliError::from(ShellError::Cancelled),
        InstanceError::RebindRecoveryRequired { .. } => "instance::rebind_recovery_required",
        InstanceError::RebindChanged { .. } => "instance::rebind_changed",
        InstanceError::IdentityClash { .. } => "instance::identity_clash",
        InstanceError::RootUnavailable { .. } => "instance::root_unavailable",
        InstanceError::ReservedRoot { .. } => "instance::reserved_root",
        InstanceError::Busy(_) => "instance::busy",
        InstanceError::Unsafe(_) => "instance::unsafe",
        InstanceError::Conflict(_) => "instance::conflict",
        InstanceError::CaseInsensitive(_) | InstanceError::UnsupportedFilesystem { .. } => {
            "instance::unsupported_filesystem"
        }
        InstanceError::FilesystemProbe { .. } | InstanceError::Io(_) => "instance::io",
    };
    CliError::failed(code, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::os::unix::fs::{symlink, PermissionsExt};

    use org_cli_shell::error::CliDiagnostic;
    use org_platform::service::{Registration, ServiceError, Unit};

    use super::*;

    struct StubServiceManager(fn(&str) -> Result<ServiceStatus, ServiceError>);

    impl ServiceManager for StubServiceManager {
        fn register(&self, _: &Unit) -> Result<Registration, ServiceError> {
            unreachable!()
        }
        fn start(&self, _: &str) -> Result<(), ServiceError> {
            unreachable!()
        }
        fn stop(&self, _: &str) -> Result<(), ServiceError> {
            unreachable!()
        }
        fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
            (self.0)(id)
        }
        fn unregister(&self, _: &str) -> Result<(), ServiceError> {
            unreachable!()
        }
    }

    #[test]
    fn ensure_service_stopped_refuses_running_or_errored_service() {
        let running = StubServiceManager(|_| Ok(ServiceStatus::Running { pid: 4242 }));
        let err =
            ensure_service_stopped_with("lab", "0193a5c0-0000-7000-8000-000000000001", &running)
                .unwrap_err();
        assert_eq!(err.code(), "instance::busy");

        let failed = StubServiceManager(|_| Err(ServiceError::System("dbus unavailable".into())));
        let err =
            ensure_service_stopped_with("lab", "0193a5c0-0000-7000-8000-000000000001", &failed)
                .unwrap_err();
        assert_eq!(err.code(), "instance::service");

        for ok_fn in [
            (|_| Ok(ServiceStatus::Stopped)) as fn(&str) -> Result<ServiceStatus, ServiceError>,
            |_| Ok(ServiceStatus::Unloaded),
            |_| Ok(ServiceStatus::NotRegistered),
        ] {
            let mgr = StubServiceManager(ok_fn);
            assert!(ensure_service_stopped_with(
                "lab",
                "0193a5c0-0000-7000-8000-000000000001",
                &mgr
            )
            .is_ok());
        }
    }

    #[test]
    fn acquire_instance_locks_rejects_symlinks_non_dirs_unreadable_and_holds_locks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        fs::create_dir(&root).unwrap();

        // 1. Symlinked office-state fails closed.
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("office-state")).unwrap();
        let err = acquire_instance_locks(&root).err().unwrap();
        assert_eq!(err.code(), "instance::unsafe_lock");
        fs::remove_file(root.join("office-state")).unwrap();

        // 2. Symlinked offices fails closed.
        let office_home = root.join("office-state");
        fs::create_dir(&office_home).unwrap();
        symlink(&outside, office_home.join("offices")).unwrap();
        let err = acquire_instance_locks(&root).err().unwrap();
        assert_eq!(err.code(), "instance::unsafe_lock");
        fs::remove_file(office_home.join("offices")).unwrap();

        // 3. Regular file or symlink entry inside offices fails closed.
        let offices = office_home.join("offices");
        fs::create_dir(&offices).unwrap();
        fs::write(offices.join("stray-file"), b"nope").unwrap();
        assert_eq!(
            acquire_instance_locks(&root).err().unwrap().code(),
            "instance::unsafe_lock"
        );
        fs::remove_file(offices.join("stray-file")).unwrap();

        symlink(&outside, offices.join("office-link")).unwrap();
        assert_eq!(
            acquire_instance_locks(&root).err().unwrap().code(),
            "instance::unsafe_lock"
        );
        fs::remove_file(offices.join("office-link")).unwrap();

        // 4. Unreadable offices directory fails closed (never fails open).
        fs::set_permissions(&offices, fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = acquire_instance_locks(&root).err().unwrap();
        fs::set_permissions(&offices, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(unreadable.code(), "instance::unsafe_lock");

        // 5. Acquiring locks holds both host.lock and office.lock until dropped.
        let office_dir = offices.join("off-1");
        fs::create_dir(&office_dir).unwrap();
        fs::write(office_home.join("host.lock"), b"").unwrap();
        fs::write(office_dir.join("office.lock"), b"").unwrap();
        let guard = acquire_instance_locks(&root).unwrap();

        // While guard is live, contending exclusive locks fail with WouldBlock.
        let probe_host = OpenOptions::new()
            .read(true)
            .write(true)
            .open(office_home.join("host.lock"))
            .unwrap();
        assert!(matches!(
            probe_host.try_lock(),
            Err(TryLockError::WouldBlock)
        ));
        let probe_office = OpenOptions::new()
            .read(true)
            .write(true)
            .open(office_dir.join("office.lock"))
            .unwrap();
        assert!(matches!(
            probe_office.try_lock(),
            Err(TryLockError::WouldBlock)
        ));

        drop(guard);
        probe_host.try_lock().unwrap();
        probe_host.unlock().unwrap();
        probe_office.try_lock().unwrap();
        probe_office.unlock().unwrap();
    }
}
