//! `release install`: verify a signed bundle and pin it atomically into the release store,
//! then move the instances' registered services to it (`org_host::setup_flow::ServiceRefresh`).

use std::fs;
use std::path::{Path, PathBuf};

use org_cli_shell::context::ExecutionContext;
use org_host::release::{ArchiveBundle, DirectoryBundle, ReleasePin, ReleaseStore};
use org_host::setup_flow::{InstallAttempt, RefreshOutcome, ServiceRefresh, ServiceRefreshReport};
use org_platform::service::{ServiceManager, ServiceMode};
use serde::Serialize;

use super::{store_dir, PinView};
use crate::error::CliError;
use crate::host;

pub(super) fn run(
    source_raw: &str,
    explicit_store: Option<&str>,
    restart: bool,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let store_dir = store_dir(explicit_store)?;
    let (source, meta) = resolve_bundle_source(source_raw)?;
    let kind = if meta.is_dir() {
        "directory"
    } else if meta.is_file() {
        "archive"
    } else {
        return Err(CliError::usage(format!(
            "bundle path `{}` must be a `.tar.gz` archive or release directory",
            source.display()
        )));
    };
    let mut found = registered_services(&store_dir)?;
    if let Some(warning) = &found.warning {
        // Under `--json` it is the output's `services_warning`: standard error holds only
        // an error's envelope.
        if !ctx.modes().json {
            ctx.warn(warning);
        }
    }
    if ctx.modes().dry_run {
        return preview(ctx, &store_dir, &source, kind, &found, restart);
    }
    // Only auto-create and restrict mode 0700 on our own default app directory; never chmod
    // an arbitrary user-supplied `--store` parent (such as `$HOME`). Either step failing
    // stops the install: the store is never opened under a folder left more open.
    if explicit_store.is_none() {
        if let Some(parent) = store_dir.parent() {
            org_platform::fs::create_owner_only_dir_all(parent)
                .and_then(|()| org_platform::fs::set_owner_only_dir(parent))
                .map_err(|e| {
                    format!(
                        "creating release store parent owner-only ({}): {e}",
                        parent.display()
                    )
                })?;
        }
    }
    // A running release host holds the store: the services that run from it are stopped
    // only when the store is busy, and started again if the install fails.
    let manager = &*found.manager;
    let pin = found
        .services
        .install(manager, || install_once(&store_dir, &source, kind))?;
    let program = store_dir
        .join("releases")
        .join(&pin.release_id)
        .join("agentic-org");
    let program = fs::canonicalize(&program)
        .map_err(|e| format!("locating the installed {}: {e}", program.display()))?;
    let reports = std::mem::take(&mut found.services).refresh(manager, &program, restart);
    render(ctx, &store_dir, &pin, reports, found.warning)
}

/// The service manager setup registers with, and the account's registered services as it
/// holds them.
struct Registered {
    manager: Box<dyn ServiceManager>,
    services: ServiceRefresh,
    /// Why they could not be read, when they could not: the install goes on without them.
    warning: Option<String>,
}

/// Only the service manager is composed: a disk-encryption probe or credential folder that
/// cannot be determined has nothing to do with moving services.
fn registered_services(store_dir: &Path) -> Result<Registered, CliError> {
    let inspected = host::service_manager().and_then(|manager| {
        let services = ServiceRefresh::inspect(&*manager, store_dir)?;
        Ok((manager, services))
    });
    match inspected {
        Ok((manager, services)) => Ok(Registered {
            manager,
            services,
            warning: None,
        }),
        Err(why) => Ok(Registered {
            manager: org_platform::service::service(ServiceMode::Foreground, Path::new("/"))
                .map_err(|e| e.to_string())?,
            services: ServiceRefresh::default(),
            warning: Some(format!(
                "The instances' registered services were not checked ({why}); after the \
                 install, run `agentic-org setup` for each instance whose service should run \
                 the new release."
            )),
        }),
    }
}

/// One attempt: open the store (busy while a running host holds it) and install into it.
fn install_once(store_dir: &Path, source: &Path, kind: &str) -> Result<ReleasePin, InstallAttempt> {
    let mut store = ReleaseStore::open(store_dir).map_err(|e| {
        let busy = matches!(e, org_host::release::Error::Busy);
        let why = format!(
            "opening release store ({}): {e} (ensure parent directories are not group/world-writable, e.g. `chmod go-w ~/.local ~/.local/share`, and stop any running service first with `agentic-org service stop`)",
            store_dir.display()
        );
        if busy {
            InstallAttempt::Busy(why)
        } else {
            InstallAttempt::Failed(why)
        }
    })?;
    let installed = if kind == "directory" {
        DirectoryBundle::open(source)
            .map_err(|e| format!("verifying release directory `{}`: {e}", source.display()))
            .and_then(|bundle| {
                store.install_directory(&bundle).map_err(|e| {
                    format!("installing release directory `{}`: {e}", source.display())
                })
            })
    } else {
        install_archive_into_store(&mut store, store_dir, source)
    };
    installed.map_err(InstallAttempt::Failed)
}

/// The pinned release, as `release status` shows it, with what the install did to each
/// registered service (`services` in JSON; a line each for a person).
fn render(
    ctx: &mut ExecutionContext,
    store_dir: &Path,
    pin: &ReleasePin,
    services: Vec<ServiceRefreshReport>,
    services_warning: Option<String>,
) -> Result<(), CliError> {
    #[derive(Serialize)]
    struct Installed {
        #[serde(flatten)]
        pin: PinView,
        services: Vec<ServiceRefreshReport>,
        #[serde(skip_serializing_if = "Option::is_none")]
        services_warning: Option<String>,
    }
    let view = Installed {
        pin: PinView::new(store_dir, pin, "installed"),
        services,
        services_warning,
    };
    if ctx.modes().json {
        return Ok(ctx.write_json(&view)?);
    }
    view.pin.render(ctx)?;
    for service in &view.services {
        match service.outcome {
            RefreshOutcome::Refused | RefreshOutcome::Failed => ctx.warn(&service.note),
            RefreshOutcome::Refreshed | RefreshOutcome::Unchanged if service.running => {
                ctx.success(&service.note)
            }
            _ => ctx.note(&service.note),
        }
    }
    Ok(())
}

/// What `--dry-run` shows: the bundle that would be verified, the store it would be pinned
/// into, whether services stopped for the install would be started again (`restart`, off
/// with `--no-restart`), and what would happen to each registered service. Nothing is
/// opened, extracted, locked, stopped or written.
#[derive(Serialize)]
struct InstallPlan {
    action: &'static str,
    bundle: String,
    kind: &'static str,
    store: String,
    restart: bool,
    services: Vec<PlannedService>,
    #[serde(skip_serializing_if = "Option::is_none")]
    services_warning: Option<String>,
}

#[derive(Serialize)]
struct PlannedService {
    instance: String,
    action: &'static str,
    running: bool,
    /// Whether the install would start it again on the new release.
    restart: bool,
}

fn preview(
    ctx: &mut ExecutionContext,
    store_dir: &Path,
    source: &Path,
    kind: &'static str,
    found: &Registered,
    restart: bool,
) -> Result<(), CliError> {
    let plan = InstallPlan {
        action: "release_install",
        bundle: source.display().to_string(),
        kind,
        store: store_dir.display().to_string(),
        restart,
        services: found
            .services
            .planned()
            .into_iter()
            .map(|(instance, action, running)| PlannedService {
                restart: restart && running && action == "refresh",
                instance,
                action,
                running,
            })
            .collect(),
        services_warning: found.warning.clone(),
    };
    let mut summary = format!(
        "would verify the release {kind} {} and pin it into {}",
        plan.bundle, plan.store
    );
    for service in &plan.services {
        let instance = &service.instance;
        summary.push_str(&match (service.action, service.running, service.restart) {
            ("refresh", true, true) => format!(
                "; stop the service of instance \"{instance}\", move it to the new release and start it again"
            ),
            ("refresh", true, false) => format!(
                "; stop the service of instance \"{instance}\" and move it to the new release without starting it again (--no-restart)"
            ),
            ("refresh", false, _) => format!(
                "; move the stopped service of instance \"{instance}\" to the new release"
            ),
            (left, _, _) => {
                format!("; leave the service of instance \"{instance}\" as it is ({left})")
            }
        });
    }
    Ok(ctx.render_dry_run(&summary, &plan)?)
}

pub(super) fn resolve_bundle_source(source_raw: &str) -> Result<(PathBuf, fs::Metadata), String> {
    let abs = std::path::absolute(Path::new(source_raw))
        .map_err(|e| format!("resolving bundle path `{source_raw}`: {e}"))?;
    let sym = fs::symlink_metadata(&abs)
        .map_err(|e| format!("reading bundle path `{}`: {e}", abs.display()))?;
    if sym.file_type().is_symlink() {
        return Err(format!(
            "bundle path `{}` must not be a symlink",
            abs.display()
        ));
    }
    // Canonicalize ancestor OS symlinks (e.g. macOS `/var -> /private/var`, `/tmp -> /private/tmp`)
    // so `ArchiveBundle::open` / `DirectoryBundle::open`'s `O_NOFOLLOW` walk succeeds while
    // still rejecting a symlinked leaf above.
    let canonical = match (abs.parent(), abs.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map_err(|e| format!("canonicalizing parent of `{}`: {e}", abs.display()))?
            .join(name),
        _ => abs,
    };
    Ok((canonical, sym))
}

pub(super) struct StagingDirGuard(pub(super) PathBuf);

impl Drop for StagingDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A fresh run folder under `<store>/staging`, removed when the guard drops. The staging
/// folder is made or tightened owner-only (`0700`) and refused when it is a symbolic link or
/// cannot be; the run folder is created `0700` in one step, never with a wider mode first.
fn prepare_staging(store_dir: &Path) -> Result<StagingDirGuard, String> {
    let staging_root = store_dir.join("staging");
    org_platform::fs::create_owner_only_dir_all(&staging_root)
        .and_then(|()| org_platform::fs::set_owner_only_dir(&staging_root))
        .map_err(|e| format!("preparing staging folder ({}): {e}", staging_root.display()))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let run_staging = staging_root.join(format!("install-{}-{nanos}", std::process::id()));
    org_platform::fs::create_owner_only_dir(&run_staging)
        .map_err(|e| format!("preparing staging run ({}): {e}", run_staging.display()))?;
    Ok(StagingDirGuard(run_staging))
}

fn install_archive_into_store(
    store: &mut ReleaseStore,
    store_dir: &Path,
    source: &Path,
) -> Result<ReleasePin, String> {
    let _guard = prepare_staging(store_dir)?;
    let run_staging = &_guard.0;
    let mut bundle = ArchiveBundle::open(source, run_staging)
        .map_err(|e| format!("verifying release archive `{}`: {e}", source.display()))?;
    store
        .install_archive(&mut bundle)
        .map_err(|e| format!("installing release archive `{}`: {e}", source.display()))
}

#[cfg(test)]
mod tests {
    /// P1-4: the staging folders are owner-only or the install stops: a more open staging
    /// folder is tightened, a linked one is refused (nothing is made through it), and the run
    /// folder is `0700` and removed with its guard.
    #[test]
    fn staging_is_owner_only_or_refused() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &std::path::Path| {
            std::fs::symlink_metadata(path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        std::fs::create_dir_all(store.join("staging")).unwrap();
        std::fs::set_permissions(
            store.join("staging"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let guard = super::prepare_staging(&store).unwrap();
        assert_eq!(mode(&store.join("staging")), 0o700);
        assert_eq!(mode(&guard.0), 0o700);
        let run = guard.0.clone();
        drop(guard);
        assert!(!run.exists());

        let linked = dir.path().join("linked");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, linked.join("staging")).unwrap();
        let error = super::prepare_staging(&linked).map(|_| ()).unwrap_err();
        assert!(error.contains("staging folder"), "{error}");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[test]
    fn resolve_bundle_source_rejects_symlink_leaf_and_canonicalizes_parent() {
        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("real");
        std::fs::create_dir_all(&real_dir).unwrap();
        let archive = real_dir.join("bundle.tar.gz");
        std::fs::write(&archive, b"archive").unwrap();
        let parent_link = dir.path().join("alias");
        std::os::unix::fs::symlink(&real_dir, &parent_link).unwrap();

        let (resolved, meta) =
            super::resolve_bundle_source(parent_link.join("bundle.tar.gz").to_str().unwrap())
                .unwrap();
        assert!(meta.is_file());
        assert_eq!(resolved, archive.canonicalize().unwrap());

        let leaf_link = real_dir.join("link.tar.gz");
        std::os::unix::fs::symlink(&archive, &leaf_link).unwrap();
        let err = super::resolve_bundle_source(leaf_link.to_str().unwrap()).unwrap_err();
        assert!(err.contains("symlink"), "{err}");

        // StagingDirGuard always cleans up staging directory on drop (including early error exits).
        let stage = dir.path().join("install-test");
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join("input-1.gz"), b"frozen").unwrap();
        {
            let _guard = super::StagingDirGuard(stage.clone());
            assert!(stage.exists());
        }
        assert!(!stage.exists());
    }
}
