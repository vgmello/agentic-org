//! `agentic-org release install|status` (`TASK-121`, ADR-0009): verify and atomically stage
//! a signed release bundle (`.tar.gz` archive or directory) into the account's protected
//! release store, or inspect the active pinned release.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use org_host::release::{ArchiveBundle, DirectoryBundle, ReleasePin, ReleaseStore};

pub(crate) const HELP: &str = "agentic-org release <subcommand>\n\
Use when: installing a publisher-signed release bundle into this host's protected release store or checking which release is currently pinned.\n\n\
Subcommands:\n  \
install <bundle.tar.gz | bundle-dir> [--store <dir>] [--json]\n      \
Verify Ed25519 signature, closed inventory, and binary hashes, then atomically pin the release\n      \
Use when: installing or upgrading a prebuilt signed Agentic Org release package without compiling from source.\n  \
status [--store <dir>] [--json]\n      \
Show the active pinned release ID, sequence, manifest SHA-256, and installed executable paths\n      \
Use when: verifying which release version is currently installed and active on this host.";

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{HELP}");
            Ok(())
        }
        [sub, rest @ ..] if sub == "install" => run_install(rest),
        [sub, rest @ ..] if sub == "status" => run_status(rest),
        _ => Err(HELP.to_owned()),
    }
}

fn strip_json_flag(arguments: &[String]) -> Result<(Vec<String>, bool), String> {
    let mut json = false;
    let mut rest = Vec::with_capacity(arguments.len());
    for arg in arguments {
        if arg == "--json" {
            if json {
                return Err("--json is given more than once".into());
            }
            json = true;
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((rest, json))
}

fn resolve_store_dir(explicit: Option<&str>) -> Result<PathBuf, String> {
    match explicit {
        Some(raw) => {
            let path = PathBuf::from(raw);
            if !path.is_absolute() {
                return Err(format!("--store ({raw}) must be an absolute path"));
            }
            Ok(path)
        }
        None => org_host::default_release_store_dir(),
    }
}

fn resolve_bundle_source(source_raw: &str) -> Result<(PathBuf, fs::Metadata), String> {
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

struct StagingDirGuard(PathBuf);

impl Drop for StagingDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn install_archive_into_store(
    store: &mut ReleaseStore,
    store_dir: &Path,
    source: &Path,
) -> Result<ReleasePin, String> {
    let staging_root = store_dir.join("staging");
    fs::create_dir_all(&staging_root)
        .map_err(|e| format!("preparing staging folder ({}): {e}", staging_root.display()))?;
    let _ = fs::set_permissions(&staging_root, fs::Permissions::from_mode(0o700));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let run_staging = staging_root.join(format!("install-{}-{nanos}", std::process::id()));
    fs::create_dir(&run_staging)
        .map_err(|e| format!("preparing staging run ({}): {e}", run_staging.display()))?;
    let _guard = StagingDirGuard(run_staging.clone());
    fs::set_permissions(&run_staging, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("securing staging run ({}): {e}", run_staging.display()))?;
    let mut bundle = ArchiveBundle::open(source, &run_staging)
        .map_err(|e| format!("verifying release archive `{}`: {e}", source.display()))?;
    store
        .install_archive(&mut bundle)
        .map_err(|e| format!("installing release archive `{}`: {e}", source.display()))
}

fn run_install(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let Some((source_raw, tail)) = arguments.split_first() else {
        return Err(format!("<bundle.tar.gz | bundle-dir> is required\n{HELP}"));
    };
    if source_raw.starts_with("--") {
        return Err(format!("<bundle.tar.gz | bundle-dir> is required\n{HELP}"));
    }
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--store"])?;
    let explicit_store = options.get("--store").map(String::as_str);
    let store_dir = resolve_store_dir(explicit_store)?;
    let (source, meta) = resolve_bundle_source(source_raw)?;

    // Only auto-create and restrict mode 0700 on our own default app directory; never chmod
    // an arbitrary user-supplied `--store` parent (such as `$HOME`).
    if explicit_store.is_none() {
        if let Some(parent) = store_dir.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!("creating release store parent ({}): {e}", parent.display())
            })?;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    let mut store = ReleaseStore::open(&store_dir).map_err(|e| {
        format!(
            "opening release store ({}): {e} (ensure parent directories are not group/world-writable, e.g. `chmod go-w ~/.local ~/.local/share`, and stop any running service first with `agentic-org service stop`)",
            store_dir.display()
        )
    })?;
    let pin = if meta.is_dir() {
        let bundle = DirectoryBundle::open(&source)
            .map_err(|e| format!("verifying release directory `{}`: {e}", source.display()))?;
        store
            .install_directory(&bundle)
            .map_err(|e| format!("installing release directory `{}`: {e}", source.display()))?
    } else if meta.is_file() {
        install_archive_into_store(&mut store, &store_dir, &source)?
    } else {
        return Err(format!(
            "bundle path `{}` must be a `.tar.gz` archive or release directory",
            source.display()
        ));
    };
    print_pin(&store_dir, &pin, "installed", json)
}

fn run_status(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let (rest, json) = strip_json_flag(arguments)?;
    let options = crate::options(&rest, &["--store"])?;
    let store_dir = resolve_store_dir(options.get("--store").map(String::as_str))?;
    if !store_dir.exists() {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "store": store_dir.display().to_string(),
                    "pinned": serde_json::Value::Null,
                })
            );
        } else {
            println!("No release installed in {}.", store_dir.display());
        }
        return Ok(());
    }
    let mut store = ReleaseStore::open(&store_dir).map_err(|e| {
        format!(
            "opening release store ({}): {e} (a running service host holds the store lock while active)",
            store_dir.display()
        )
    })?;
    let recovered = store
        .recover()
        .map_err(|e| format!("verifying release store ({}): {e}", store_dir.display()))?;
    match recovered {
        Some(pin) => print_pin(&store_dir, &pin, "active", json),
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "store": store_dir.display().to_string(),
                        "pinned": serde_json::Value::Null,
                    })
                );
            } else {
                println!("No release pinned in {}.", store_dir.display());
            }
            Ok(())
        }
    }
}

fn print_pin(store_dir: &Path, pin: &ReleasePin, state: &str, json: bool) -> Result<(), String> {
    let release_dir = store_dir.join("releases").join(&pin.release_id);
    if json {
        let payload = serde_json::json!({
            "state": state,
            "store": store_dir.display().to_string(),
            "release_id": pin.release_id,
            "sequence": pin.sequence,
            "manifest_sha256": pin.manifest_sha256,
            "release_dir": release_dir.display().to_string(),
            "agentic_org": release_dir.join("agentic-org").display().to_string(),
            "agent_os": release_dir.join("agent-os").display().to_string(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|_| "could not encode release status".to_owned())?
        );
        return Ok(());
    }
    println!(
        "Release {} ({state})\n  sequence:        {}\n  manifest_sha256: {}\n  release_dir:     {}\n  agentic_org:     {}\n  agent_os:        {}",
        pin.release_id,
        pin.sequence,
        pin.manifest_sha256,
        release_dir.display(),
        release_dir.join("agentic-org").display(),
        release_dir.join("agent-os").display(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{run, HELP};

    fn invoke(args: &[&str]) -> Result<(), String> {
        run(&args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn release_help_and_argument_validation_work_without_store_mutation() {
        assert!(HELP.contains("Use when:"));
        assert!(HELP.contains("install"));
        assert!(HELP.contains("status"));
        assert_eq!(invoke(&["--help"]), Ok(()));
        assert_eq!(invoke(&["install", "--help"]), Ok(()));
        assert_eq!(invoke(&["status", "--help"]), Ok(()));
        assert!(invoke(&["install"]).unwrap_err().contains("<bundle"));
        assert!(invoke(&["status", "--store", "relative/path"])
            .unwrap_err()
            .contains("absolute"));
        assert!(invoke(&["status", "--json", "--json"])
            .unwrap_err()
            .contains("more than once"));
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
