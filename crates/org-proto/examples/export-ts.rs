//! Regenerates the committed TypeScript in `app/src/lib/api/generated/`:
//!
//!     cargo run -p org-proto --example export-ts
//!
//! The directory is fully generated: every file in it is replaced. The new files are written
//! to a staging directory next to it and swapped in only once the export succeeded. Run one
//! at a time.

use std::fs;
use std::path::{Path, PathBuf};

const STAGING_PREFIX: &str = ".generated-";
const NEW_PREFIX: &str = ".generated-new-";
const OLD_PREFIX: &str = ".generated-old-";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/lib/api");
    let dir = api_dir.join("generated");
    recover_interrupted_runs(&api_dir, &dir)?;

    let pid = std::process::id();
    let staging = api_dir.join(format!("{NEW_PREFIX}{pid}"));
    let previous = api_dir.join(format!("{OLD_PREFIX}{pid}"));

    let files = match org_proto::typescript::export_typescript(&staging) {
        Ok(files) => files,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e.into());
        }
    };
    if dir.exists() {
        fs::rename(&dir, &previous)?;
    }
    if let Err(e) = fs::rename(&staging, &dir) {
        if previous.exists() && fs::rename(&previous, &dir).is_err() {
            return Err(format!(
                "couldn't move {} into place: {e}; the old files are in {}",
                staging.display(),
                previous.display()
            )
            .into());
        }
        return Err(format!("couldn't move {} into place: {e}", staging.display()).into());
    }
    if previous.exists() {
        fs::remove_dir_all(&previous)?;
    }
    println!("wrote {} files to {}", files.len(), dir.display());
    Ok(())
}

/// Cleans up after an interrupted run: puts a set-aside `generated` back if it went missing,
/// then removes leftover staging directories.
fn recover_interrupted_runs(api_dir: &Path, dir: &Path) -> std::io::Result<()> {
    let Ok(entries) = fs::read_dir(api_dir) else {
        return Ok(());
    };
    let mut leftovers: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
            && entry.file_type()?.is_dir()
        {
            leftovers.push(entry.path());
        }
    }
    if !dir.exists() {
        let old = leftovers.iter().position(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(OLD_PREFIX))
        });
        if let Some(index) = old {
            fs::rename(leftovers.remove(index), dir)?;
        }
    }
    for path in leftovers {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}
