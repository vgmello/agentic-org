//! Turn-scoped `0400` attachment staging and guaranteed cleanup (`TASK-98.3`, AC #3 & AC #6).
//!
//! Files are staged strictly under `<runtime_dir>/attachments/<turn_id>/<att_id>.<ext>` with
//! mode `0400` (owner read-only via `org-platform::fs`) inside a `0700` directory, and purged
//! immediately on turn end, child exit/finalization, and office startup recovery.
//!
//! Zero platform `#[cfg]` live here: all OS permission enforcement flows through `org-platform::fs`.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Stable subdirectory inside `<runtime_dir>` exposed via `SessionSpec.read_only_dirs`.
pub const ATTACHMENTS_DIR_NAME: &str = "attachments";

/// Strips trailing `/` and `/.` so `symlink_metadata` (`lstat`) inspects the final component
/// itself rather than dereferencing a symlinked directory (`.agents/rules` §3.1).
fn normalize_lstat_path(path: &Path) -> PathBuf {
    path.components().collect()
}

/// Returns `true` when `path` (after stripping trailing `/` and `/.`) is a real directory and
/// never a symbolic link.
fn is_real_dir(path: &Path) -> bool {
    let norm = normalize_lstat_path(path);
    fs::symlink_metadata(&norm)
        .is_ok_and(|meta| meta.file_type().is_dir() && !meta.file_type().is_symlink())
}

/// Returns `<runtime_dir>/attachments`, the stable per-agent root added to `read_only_dirs`
/// so `spec_hash()` never changes across turns.
pub fn attachments_root(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(ATTACHMENTS_DIR_NAME)
}

/// Ensures `<runtime_dir>/attachments` exists as a real `0700` directory (never a symlink) and
/// returns its canonical path.
pub fn ensure_attachments_root(runtime_dir: &Path) -> io::Result<PathBuf> {
    let root = normalize_lstat_path(&attachments_root(runtime_dir));
    org_platform::fs::create_owner_only_dir_all(&root)?;
    org_platform::fs::set_owner_only_dir(&root)?;
    root.canonicalize()
}

/// Validates a single path segment used for `turn_id`, `attachment_id`, or file extension.
pub(crate) fn is_safe_stage_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 128
        && token != "."
        && token != ".."
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn is_safe_single_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains('/')
        && !segment.contains('\\')
        && !segment.contains('\0')
}

/// Maps an allowlisted attachment MIME type to a fixed ASCII file extension.
pub(crate) fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type.trim().to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "application/pdf" => "pdf",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "text/markdown" => "md",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "application/json" => "json",
        _ => "bin",
    }
}

/// Atomically stages one immutable `0400` file under
/// `<attachments_root>/<turn_id>/<attachment_id>.<ext>` via verified `OwnerOnlyDir` descriptors
/// (no symlink traversal, no observable `0600` window) and returns its canonical path after
/// verifying canonical containment under `<attachments_root>`.
pub fn stage_turn_file(
    attachments_root: &Path,
    turn_id: &str,
    attachment_id: &str,
    ext: &str,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    if !is_safe_stage_token(turn_id)
        || !is_safe_stage_token(attachment_id)
        || !is_safe_stage_token(ext)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid attachment staging identifier",
        ));
    }
    let norm_root = normalize_lstat_path(attachments_root);
    org_platform::fs::create_owner_only_dir_all(&norm_root)?;
    org_platform::fs::set_owner_only_dir(&norm_root)?;
    let uid = org_platform::fs::effective_uid()?;
    let root_dir = org_platform::fs::OwnerOnlyDir::open(&norm_root, uid)?;
    let turn_dir = root_dir.open_or_create_child(OsStr::new(turn_id), uid)?;
    let file_name = format!("{attachment_id}.{ext}");
    org_platform::fs::write_owner_read_only_file_at(&turn_dir, OsStr::new(&file_name), bytes)?;

    let canon_root = norm_root.canonicalize()?;
    let canon_target = norm_root.join(turn_id).join(&file_name).canonicalize()?;
    if !canon_target.starts_with(&canon_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "staged attachment path escaped attachments root",
        ));
    }
    Ok(canon_target)
}

/// Tightens `<attachments_root>/<turn_id>` to mode `0500` (owner read + search, no write) via
/// verified directory descriptors after all files for the turn have been staged (`TASK-98.3`).
pub fn tighten_turn_dir(attachments_root: &Path, turn_id: &str) -> io::Result<()> {
    if !is_safe_stage_token(turn_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid attachment staging identifier",
        ));
    }
    let norm_root = normalize_lstat_path(attachments_root);
    let uid = org_platform::fs::effective_uid()?;
    let root_dir = org_platform::fs::OwnerOnlyDir::open(&norm_root, uid)?;
    let turn_dir = root_dir.open_child(OsStr::new(turn_id), uid)?;
    turn_dir.set_read_only()
}

/// Safely removes a real directory tree without ever following a symbolic link.
/// Aborts immediately if `set_owner_only_dir` refuses the directory (uid mismatch or symlink swap).
fn remove_real_dir_tree(dir: &Path) {
    let dir = normalize_lstat_path(dir);
    let Ok(meta) = fs::symlink_metadata(&dir) else {
        return;
    };
    if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
        let _ = fs::remove_file(&dir);
        return;
    }
    // Restore owner write bit (`0700`) through a verified `O_DIRECTORY | O_NOFOLLOW` descriptor;
    // abort immediately on any error so we never delete across a symlink or foreign directory.
    if org_platform::fs::set_owner_only_dir(&dir).is_err() {
        return;
    }
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = normalize_lstat_path(&entry.path());
            let Ok(child_meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if child_meta.file_type().is_dir() && !child_meta.file_type().is_symlink() {
                remove_real_dir_tree(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }
    let _ = fs::remove_dir(&dir);
}

/// Immediately purges `<attachments_root>/<turn_id>/` and all staged files inside it.
pub fn purge_turn_dir(attachments_root: &Path, turn_id: &str) {
    if !is_safe_stage_token(turn_id) {
        return;
    }
    let root = normalize_lstat_path(attachments_root);
    if !is_real_dir(&root) {
        return;
    }
    if org_platform::fs::set_owner_only_dir(&root).is_err() {
        return;
    }
    remove_real_dir_tree(&root.join(turn_id));
}

/// Purges every turn subdirectory under `<attachments_root>` while preserving the `0700` root
/// directory itself so warm session sandboxes remain valid.
pub fn purge_all_stale_turn_dirs(attachments_root: &Path) {
    let root = normalize_lstat_path(attachments_root);
    if !is_real_dir(&root) {
        return;
    }
    if org_platform::fs::set_owner_only_dir(&root).is_err() {
        return;
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = normalize_lstat_path(&entry.path());
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_dir() && !meta.file_type().is_symlink() {
            remove_real_dir_tree(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Scans `<durable_root>/local/offices/<office_id>/agents/*/runtime/attachments` on office
/// startup/recovery and purges every leftover turn directory (`TASK-98.3` AC #3), verifying each
/// traversed directory (including `runtime`) is a real directory and never a symlink.
pub fn purge_office_stale_attachments(durable_root: &Path, office_id: &str) {
    if !is_safe_single_segment(office_id) {
        return;
    }
    let agents_root = normalize_lstat_path(
        &durable_root
            .join("local")
            .join("offices")
            .join(office_id)
            .join("agents"),
    );
    if !is_real_dir(&agents_root) {
        return;
    }
    let Ok(entries) = fs::read_dir(&agents_root) else {
        return;
    };
    for entry in entries.flatten() {
        let agent_dir = normalize_lstat_path(&entry.path());
        if !is_real_dir(&agent_dir) {
            continue;
        }
        let runtime_dir = normalize_lstat_path(&agent_dir.join("runtime"));
        if !is_real_dir(&runtime_dir) {
            continue;
        }
        let att_root = attachments_root(&runtime_dir);
        purge_all_stale_turn_dirs(&att_root);
    }
}
