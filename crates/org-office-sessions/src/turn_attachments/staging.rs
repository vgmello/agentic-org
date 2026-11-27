//! Turn-scoped `0400` attachment staging and guaranteed cleanup (`TASK-98.3`, AC #3 & AC #6).
//!
//! Files are staged strictly under `<runtime_dir>/attachments/<turn_id>/<att_id>.<ext>` with
//! mode `0400` (owner read-only via `org-platform::fs`) inside a `0700` directory, and purged
//! immediately on turn end, child exit/finalization, and office startup recovery.
//!
//! Zero platform `#[cfg]` live here: all OS permission enforcement flows through `org-platform::fs`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Stable subdirectory inside `<runtime_dir>` exposed via `SessionSpec.read_only_dirs`.
pub const ATTACHMENTS_DIR_NAME: &str = "attachments";

/// Returns `<runtime_dir>/attachments`, the stable per-agent root added to `read_only_dirs`
/// so `spec_hash()` never changes across turns.
pub fn attachments_root(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(ATTACHMENTS_DIR_NAME)
}

/// Ensures `<runtime_dir>/attachments` exists as a real `0700` directory (never a symlink) and
/// returns its canonical path.
pub fn ensure_attachments_root(runtime_dir: &Path) -> io::Result<PathBuf> {
    let root = attachments_root(runtime_dir);
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

/// Stages one immutable `0400` file under `<attachments_root>/<turn_id>/<attachment_id>.<ext>`
/// and returns its canonical path.
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
    org_platform::fs::create_owner_only_dir_all(attachments_root)?;
    org_platform::fs::set_owner_only_dir(attachments_root)?;
    let turn_dir = attachments_root.join(turn_id);
    org_platform::fs::create_owner_only_dir_all(&turn_dir)?;
    org_platform::fs::set_owner_only_dir(&turn_dir)?;
    let target = turn_dir.join(format!("{attachment_id}.{ext}"));
    org_platform::fs::write_owner_read_only_file(&target, bytes)?;
    target.canonicalize()
}

/// Safely removes a real directory tree without ever following a symbolic link.
fn remove_real_dir_tree(dir: &Path) {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return;
    };
    if meta.file_type().is_symlink() {
        let _ = fs::remove_file(dir);
        return;
    }
    if !meta.file_type().is_dir() {
        let _ = fs::remove_file(dir);
        return;
    }
    // Restore owner write bit on the directory itself if somehow tightened, never following links.
    let _ = org_platform::fs::set_owner_only_dir(dir);
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
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
    let _ = fs::remove_dir(dir);
}

/// Immediately purges `<attachments_root>/<turn_id>/` and all staged files inside it.
pub fn purge_turn_dir(attachments_root: &Path, turn_id: &str) {
    if !is_safe_stage_token(turn_id) {
        return;
    }
    let Ok(root_meta) = fs::symlink_metadata(attachments_root) else {
        return;
    };
    if !root_meta.file_type().is_dir() || root_meta.file_type().is_symlink() {
        return;
    }
    remove_real_dir_tree(&attachments_root.join(turn_id));
}

/// Purges every turn subdirectory under `<attachments_root>` while preserving the `0700` root
/// directory itself so warm session sandboxes remain valid.
pub fn purge_all_stale_turn_dirs(attachments_root: &Path) {
    let Ok(root_meta) = fs::symlink_metadata(attachments_root) else {
        return;
    };
    if !root_meta.file_type().is_dir() || root_meta.file_type().is_symlink() {
        return;
    }
    let Ok(entries) = fs::read_dir(attachments_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
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
/// startup/recovery and purges every leftover turn directory (`TASK-98.3` AC #3).
pub fn purge_office_stale_attachments(durable_root: &Path, office_id: &str) {
    let agents_root = durable_root
        .join("local")
        .join("offices")
        .join(office_id)
        .join("agents");
    let Ok(root_meta) = fs::symlink_metadata(&agents_root) else {
        return;
    };
    if !root_meta.file_type().is_dir() || root_meta.file_type().is_symlink() {
        return;
    }
    let Ok(entries) = fs::read_dir(&agents_root) else {
        return;
    };
    for entry in entries.flatten() {
        let agent_dir = entry.path();
        let Ok(meta) = fs::symlink_metadata(&agent_dir) else {
            continue;
        };
        if !meta.file_type().is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        let att_root = attachments_root(&agent_dir.join("runtime"));
        purge_all_stale_turn_dirs(&att_root);
    }
}
