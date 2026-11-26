//! Collecting a local `state/` directory into the files an import validates, within the
//! import's symlink, depth, count and size limits. Validating them through the office's
//! production projectors is the office's part (`crate::office::import`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use org_store::DurablePath;

use super::{
    ImportError, MAX_IMPORT_DEPTH, MAX_IMPORT_FILES, MAX_IMPORT_FILE_BYTES, MAX_IMPORT_TOTAL_BYTES,
};

/// Walks a local `state_root` directory (for unit tests and local directory validation), enforcing
/// symlink, depth, file-count, and size limits, collecting every file under `state/<relative>`.
pub fn collect_state_files(
    state_root: &Path,
) -> Result<BTreeMap<DurablePath, Vec<u8>>, ImportError> {
    let mut files = BTreeMap::new();
    let mut total_bytes: u64 = 0;
    walk_dir(state_root, state_root, 0, &mut files, &mut total_bytes)?;
    Ok(files)
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut BTreeMap<DurablePath, Vec<u8>>,
    total_bytes: &mut u64,
) -> Result<(), ImportError> {
    if depth > MAX_IMPORT_DEPTH {
        return Err(ImportError::InvalidFile {
            path: "state/".into(),
            reason: "directory nesting exceeds maximum allowed depth".into(),
        });
    }
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| ImportError::Storage(format!("reading state directory: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ImportError::Storage(format!("listing state directory: {e}")))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        if dir == root && name == ".git" {
            continue;
        }
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| ImportError::Storage(format!("inspecting file: {e}")))?;
        let rel = path
            .strip_prefix(root)
            .map_err(|_| ImportError::Storage("path outside staging root".into()))?;
        let rel_str = rel.to_str().ok_or_else(|| ImportError::InvalidFile {
            path: format!("state/{}", rel.to_string_lossy()),
            reason: "file path is not valid UTF-8".into(),
        })?;
        let durable_text = format!("state/{rel_str}");
        if meta.file_type().is_symlink() {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "symlinks are not permitted in organization state".into(),
            });
        }
        if meta.is_dir() {
            walk_dir(root, &path, depth + 1, out, total_bytes)?;
            continue;
        }
        if !meta.is_file() {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "only regular files are permitted in organization state".into(),
            });
        }
        if out.len() >= MAX_IMPORT_FILES {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "organization state exceeds maximum file count limit".into(),
            });
        }
        if meta.len() > MAX_IMPORT_FILE_BYTES {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "file exceeds the single-file import size limit".into(),
            });
        }
        *total_bytes = total_bytes.saturating_add(meta.len());
        if *total_bytes > MAX_IMPORT_TOTAL_BYTES {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "organization state exceeds total import size limit".into(),
            });
        }
        let durable = DurablePath::parse(&durable_text).map_err(|_| ImportError::InvalidFile {
            path: durable_text.clone(),
            reason: "invalid organization state file path".into(),
        })?;
        let bytes = fs::read(&path).map_err(|e| ImportError::InvalidFile {
            path: durable_text,
            reason: format!("could not read file: {e}"),
        })?;
        out.insert(durable, bytes);
    }
    Ok(())
}
