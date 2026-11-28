//! `release verify`: offline Ed25519 signature, closed inventory, and SHA-256 verification of a
//! signed release bundle (`.tar.gz` archive or release directory) without mutating the release
//! store or touching any running service.

use std::fs;
use std::path::{Path, PathBuf};

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::release::{ArchiveBundle, DirectoryBundle};
use serde::Serialize;

use super::install::resolve_bundle_source;
use crate::error::CliError;

#[derive(Serialize)]
struct VerifyView {
    verified: bool,
    kind: &'static str,
    bundle: String,
    release_id: String,
    sequence: u64,
    manifest_sha256: String,
    files_verified: usize,
}

/// Isolated `0700` staging folder created under the OS temp directory and unconditionally removed
/// when verification finishes (or errors/panics). Never touches `--store` or the default store.
struct VerifyTempDir(PathBuf);

impl Drop for VerifyTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn prepare_verify_staging() -> Result<VerifyTempDir, String> {
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| format!("resolving temporary verification directory: {e}"))?;
    let mut nonce = [0u8; 16];
    org_platform::confinement::fill_entropy(&mut nonce)
        .map_err(|e| format!("generating verification staging nonce: {e}"))?;
    let hex_nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let dir = temp_root.join(format!("ao-rel-verify-{}-{hex_nonce}", std::process::id()));
    org_platform::fs::create_owner_only_dir(&dir)
        .and_then(|()| org_platform::fs::set_owner_only_dir(&dir))
        .map_err(|e| {
            format!(
                "preparing isolated verification folder ({}): {e}",
                dir.display()
            )
        })?;
    Ok(VerifyTempDir(dir))
}

fn validate_optional_store_flag(explicit_store: Option<&str>) -> Result<(), CliError> {
    if let Some(raw) = explicit_store {
        if !Path::new(raw).is_absolute() {
            return Err(CliError::usage(format!(
                "--store ({raw}) must be an absolute path"
            )));
        }
    }
    Ok(())
}

fn verify_archive_with<F, T>(
    source: &Path,
    explicit_store: Option<&str>,
    verify_fn: F,
) -> Result<T, CliError>
where
    F: FnOnce(&Path, &Path) -> Result<T, String>,
{
    validate_optional_store_flag(explicit_store)?;
    let guard = prepare_verify_staging()?;
    verify_fn(source, &guard.0).map_err(CliError::from)
}

pub(super) fn run(
    source_raw: &str,
    explicit_store: Option<&str>,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    validate_optional_store_flag(explicit_store)?;
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
    let manifest = if kind == "directory" {
        let bundle = DirectoryBundle::open(&source)
            .map_err(|e| format!("verifying release directory `{}`: {e}", source.display()))?;
        bundle.manifest().clone()
    } else {
        verify_archive_with(&source, explicit_store, |src, staging| {
            let bundle = ArchiveBundle::open(src, staging)
                .map_err(|e| format!("verifying release archive `{}`: {e}", src.display()))?;
            Ok(bundle.manifest().clone())
        })?
    };

    let sha256 = manifest
        .manifest_sha256()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let view = VerifyView {
        verified: true,
        kind,
        bundle: source.display().to_string(),
        release_id: manifest.manifest().release_id().to_owned(),
        sequence: manifest.manifest().sequence(),
        manifest_sha256: sha256,
        files_verified: manifest.manifest().files().len(),
    };
    let fields = [
        ("release", Cell::id(&view.release_id)),
        ("verified", Cell::status("verified", Style::Ok)),
        ("kind", Cell::plain(view.kind)),
        ("sequence", Cell::plain(view.sequence.to_string())),
        (
            "files_verified",
            Cell::plain(view.files_verified.to_string()),
        ),
        ("manifest_sha256", Cell::dim(&view.manifest_sha256)),
        ("bundle", Cell::plain(&view.bundle)),
    ];
    let json =
        serde_json::to_value(&view).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    Ok(ctx.render_detail(&view.release_id, &fields, &json)?)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn verify_archive_uses_isolated_0700_temp_dir_and_never_touches_store() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("bundle.tar.gz");
        fs::write(&archive, b"archive").unwrap();

        // 1. Nonexistent `--store` must stay completely uncreated.
        let nonexistent_store = dir.path().join("never-created-store");
        let mut observed_staging = PathBuf::new();
        let ok = verify_archive_with(
            &archive,
            Some(nonexistent_store.to_str().unwrap()),
            |src, staging| {
                assert_eq!(src, archive);
                assert!(!staging.starts_with(&nonexistent_store));
                let mode = fs::symlink_metadata(staging).unwrap().permissions().mode() & 0o7777;
                assert_eq!(mode, 0o700);
                observed_staging = staging.to_path_buf();
                Ok("verified-ok")
            },
        )
        .unwrap();
        assert_eq!(ok, "verified-ok");
        assert!(
            !nonexistent_store.exists(),
            "release verify must never create nonexistent --store"
        );
        assert!(
            !observed_staging.exists(),
            "isolated verification staging dir must be removed on drop"
        );

        // 2. Read-only (`0500`) `--store` succeeds cleanly without touching store.
        let ro_store = dir.path().join("readonly-store");
        fs::create_dir(&ro_store).unwrap();
        fs::set_permissions(&ro_store, fs::Permissions::from_mode(0o500)).unwrap();
        let ro_result =
            verify_archive_with(&archive, Some(ro_store.to_str().unwrap()), |_, staging| {
                assert!(staging.is_dir());
                assert!(!staging.starts_with(&ro_store));
                Ok("verified-readonly")
            });
        fs::set_permissions(&ro_store, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(ro_result.unwrap(), "verified-readonly");
        assert_eq!(fs::read_dir(&ro_store).unwrap().count(), 0);
    }
}
