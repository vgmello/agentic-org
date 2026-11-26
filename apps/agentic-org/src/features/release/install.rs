//! `release install`: verify a signed bundle and pin it atomically into the release store.

use std::fs;
use std::path::{Path, PathBuf};

use org_cli_shell::context::ExecutionContext;
use org_host::release::{ArchiveBundle, DirectoryBundle, ReleasePin, ReleaseStore};
use serde::Serialize;

use super::{store_dir, PinView};
use crate::error::CliError;

pub(super) fn run(
    source_raw: &str,
    explicit_store: Option<&str>,
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
    if ctx.modes().dry_run {
        return preview(ctx, &store_dir, &source, kind);
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
    let mut store = ReleaseStore::open(&store_dir).map_err(|e| {
        format!(
            "opening release store ({}): {e} (ensure parent directories are not group/world-writable, e.g. `chmod go-w ~/.local ~/.local/share`, and stop any running service first with `agentic-org service stop`)",
            store_dir.display()
        )
    })?;
    let pin = if kind == "directory" {
        let bundle = DirectoryBundle::open(&source)
            .map_err(|e| format!("verifying release directory `{}`: {e}", source.display()))?;
        store
            .install_directory(&bundle)
            .map_err(|e| format!("installing release directory `{}`: {e}", source.display()))?
    } else {
        install_archive_into_store(&mut store, &store_dir, &source)?
    };
    PinView::new(&store_dir, &pin, "installed").render(ctx)
}

/// What `--dry-run` shows: the bundle that would be verified and the store it would be
/// pinned into. Nothing is opened, extracted, locked or written.
#[derive(Serialize)]
struct InstallPlan {
    action: &'static str,
    bundle: String,
    kind: &'static str,
    store: String,
}

fn preview(
    ctx: &mut ExecutionContext,
    store_dir: &Path,
    source: &Path,
    kind: &'static str,
) -> Result<(), CliError> {
    let plan = InstallPlan {
        action: "release_install",
        bundle: source.display().to_string(),
        kind,
        store: store_dir.display().to_string(),
    };
    let summary = format!(
        "would verify the release {kind} {} and pin it into {}",
        plan.bundle, plan.store
    );
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
