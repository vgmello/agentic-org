//! `release verify`: offline Ed25519 signature, closed inventory, and SHA-256 verification of a
//! signed release bundle (`.tar.gz` archive or release directory) without mutating the release
//! store or touching any running service.

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::release::{ArchiveBundle, DirectoryBundle};
use serde::Serialize;

use super::install::{prepare_staging, resolve_bundle_source};
use super::store_dir;
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

pub(super) fn run(
    source_raw: &str,
    explicit_store: Option<&str>,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
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
        let store = store_dir(explicit_store)?;
        if explicit_store.is_none() {
            if let Some(parent) = store.parent() {
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
        let guard = prepare_staging(&store)?;
        let bundle = ArchiveBundle::open(&source, &guard.0)
            .map_err(|e| format!("verifying release archive `{}`: {e}", source.display()))?;
        bundle.manifest().clone()
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
