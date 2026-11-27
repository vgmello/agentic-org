//! Anchor-contained path verification and symlink-safe removal primitives for `uninstall`.
//!
//! Ensures every intermediate directory up to the canonical anchor (`instance_root`,
//! `account_base`, or `<user_temp>/ao`) is a real non-symlink directory strictly contained
//! within its canonical anchor before any unlink or recursive removal occurs.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::paths::{normalize_lstat_path, validate_loaded_abs_path, InstancePaths};
use crate::error::CliError;

const MAX_REMOVE_DEPTH: usize = 64;

fn unsafe_path_err(msg: impl Into<String>) -> CliError {
    CliError::failed("uninstall::unsafe_path", msg.into())
}

/// Pre-flight verifies every anchor and intermediate directory uninstall will touch before any
/// file is deleted so symlink or traversal tampering fails closed with zero partial deletion.
pub(super) fn verify_uninstall_paths(paths: &InstancePaths) -> Result<(), CliError> {
    verify_real_dir_if_present(&paths.account_base, "account base")?;
    verify_dir_within_anchor(&paths.account_base, &paths.setup_dir)?;
    verify_dir_within_anchor(&paths.account_base, &paths.locator_dir)?;
    verify_real_dir_if_present(&paths.instance_root, "instance root")?;
    verify_dir_within_anchor(&paths.instance_root, &paths.office_home)?;
    verify_dir_within_anchor(&paths.instance_root, &paths.offices_dir)?;
    verify_dir_within_anchor(&paths.instance_root, &paths.logs_dir)?;
    verify_dir_within_anchor(&paths.instance_root, &paths.office_home.join("run"))?;
    verify_dir_within_anchor(&paths.instance_root, &paths.instance_root.join("agents"))?;
    if paths.run_dir != paths.office_home.join("run") {
        verify_instance_temp_dir(paths)?;
    }
    Ok(())
}

/// Verifies `dir` is a real non-symlink directory when present. Returns `Ok(Some(canon))` when
/// present, `Ok(None)` when absent (`NotFound`), or `Err` on symlink/non-dir/I/O error.
pub(super) fn verify_real_dir_if_present(
    dir: &Path,
    label: &str,
) -> Result<Option<PathBuf>, CliError> {
    validate_loaded_abs_path(dir, label)?;
    let norm = normalize_lstat_path(dir);
    match fs::symlink_metadata(&norm) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(unsafe_path_err(format!(
            "inspecting {label} `{}`: {e}",
            norm.display()
        ))),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            Err(unsafe_path_err(format!(
                "{label} `{}` must be a real directory (symlinks are refused)",
                norm.display()
            )))
        }
        Ok(_) => norm.canonicalize().map(Some).map_err(|e| {
            unsafe_path_err(format!("canonicalizing {label} `{}`: {e}", norm.display()))
        }),
    }
}

/// Verifies every directory component from `anchor` down to and including `target` is a real
/// non-symlink directory strictly contained within canonical `anchor`. Returns `Ok(true)` if
/// `target` exists and is verified, `Ok(false)` if `anchor` or any component does not exist,
/// or `Err` on any symlink, traversal, escape, or non-directory.
pub(super) fn verify_dir_within_anchor(anchor: &Path, target: &Path) -> Result<bool, CliError> {
    validate_loaded_abs_path(anchor, "anchor")?;
    validate_loaded_abs_path(target, "target")?;
    let norm_anchor = normalize_lstat_path(anchor);
    let norm_target = normalize_lstat_path(target);
    let rel = norm_target.strip_prefix(&norm_anchor).map_err(|_| {
        unsafe_path_err(format!(
            "target `{}` escapes anchor `{}`",
            norm_target.display(),
            norm_anchor.display()
        ))
    })?;
    let Some(canon_anchor) = verify_real_dir_if_present(&norm_anchor, "anchor")? else {
        return Ok(false);
    };
    let mut cur = norm_anchor;
    for comp in rel.components() {
        let Component::Normal(seg) = comp else {
            return Err(unsafe_path_err(format!(
                "target `{}` contains disallowed path component",
                norm_target.display()
            )));
        };
        cur.push(seg);
        let Some(canon_cur) = verify_real_dir_if_present(&cur, "intermediate directory")? else {
            return Ok(false);
        };
        if !canon_cur.starts_with(&canon_anchor) || canon_cur == canon_anchor {
            return Err(unsafe_path_err(format!(
                "directory `{}` escapes canonical anchor `{}`",
                cur.display(),
                canon_anchor.display()
            )));
        }
    }
    Ok(true)
}

/// Verifies every intermediate directory from `anchor` down to `target.parent()` is a real
/// non-symlink directory strictly contained within canonical `anchor`.
pub(super) fn verify_parent_within_anchor(anchor: &Path, target: &Path) -> Result<bool, CliError> {
    validate_loaded_abs_path(anchor, "anchor")?;
    validate_loaded_abs_path(target, "target")?;
    let norm_anchor = normalize_lstat_path(anchor);
    let norm_target = normalize_lstat_path(target);
    let rel = norm_target.strip_prefix(&norm_anchor).map_err(|_| {
        unsafe_path_err(format!(
            "target `{}` escapes anchor `{}`",
            norm_target.display(),
            norm_anchor.display()
        ))
    })?;
    let mut comps = rel.components();
    if comps.next().is_none() {
        return Err(unsafe_path_err(format!(
            "target `{}` must be strictly beneath anchor `{}`",
            norm_target.display(),
            norm_anchor.display()
        )));
    }
    let Some(parent) = norm_target.parent() else {
        return Err(unsafe_path_err(format!(
            "target `{}` has no parent directory",
            norm_target.display()
        )));
    };
    if parent == norm_anchor {
        return Ok(verify_real_dir_if_present(&norm_anchor, "anchor")?.is_some());
    }
    verify_dir_within_anchor(&norm_anchor, parent)
}

/// Verifies `paths.run_dir` is strictly one direct child non-symlink directory beneath canonical
/// `<user_temp>/ao`, with no `..`, `.`, or symlink components anywhere along `user_temp` -> `ao`
/// -> `run_dir`. Returns `Ok(Some((canon_ao, canon_run)))` if `run_dir` exists and is safe to
/// remove, `Ok(None)` if absent, or `Err` on any symlink, traversal, or escape outside `ao`.
pub(super) fn verify_instance_temp_dir(
    paths: &InstancePaths,
) -> Result<Option<(PathBuf, PathBuf)>, CliError> {
    validate_loaded_abs_path(&paths.run_dir, "instance temp folder")?;
    let Some(user_temp) = &paths.user_temp else {
        return Err(unsafe_path_err(
            "cannot verify instance temp folder without user_temp base",
        ));
    };
    validate_loaded_abs_path(user_temp, "user temp base")?;
    let norm_run = normalize_lstat_path(&paths.run_dir);
    let (Some(run_parent), Some(leaf)) = (norm_run.parent(), norm_run.file_name()) else {
        return Err(unsafe_path_err(format!(
            "instance temp folder `{}` must name a direct child of `ao`",
            norm_run.display()
        )));
    };
    if Path::new(leaf).components().count() != 1 {
        return Err(unsafe_path_err(format!(
            "instance temp folder `{}` must be one direct child component",
            norm_run.display()
        )));
    }
    let ao_dir = normalize_lstat_path(&user_temp.join("ao"));
    let Some(_canon_temp) = verify_real_dir_if_present(user_temp, "user temp base")? else {
        return ensure_absent_when_ao_missing(&norm_run);
    };
    let Some(canon_ao) = verify_real_dir_if_present(&ao_dir, "instance temp `ao` directory")?
    else {
        return ensure_absent_when_ao_missing(&norm_run);
    };
    let Some(canon_parent) =
        verify_real_dir_if_present(run_parent, "instance temp parent directory")?
    else {
        return Err(unsafe_path_err(format!(
            "instance temp folder `{}` parent does not match `{}`",
            norm_run.display(),
            canon_ao.display()
        )));
    };
    if canon_parent != canon_ao {
        return Err(unsafe_path_err(format!(
            "instance temp folder `{}` is not a direct child of `{}`",
            norm_run.display(),
            canon_ao.display()
        )));
    }
    let Some(canon_run) = verify_real_dir_if_present(&norm_run, "instance temp folder")? else {
        return Ok(None);
    };
    if canon_run.parent() != Some(canon_ao.as_path()) {
        return Err(unsafe_path_err(format!(
            "instance temp folder `{}` escaped canonical `{}`",
            norm_run.display(),
            canon_ao.display()
        )));
    }
    Ok(Some((canon_ao, canon_run)))
}

fn ensure_absent_when_ao_missing(norm_run: &Path) -> Result<Option<(PathBuf, PathBuf)>, CliError> {
    match fs::symlink_metadata(norm_run) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        _ => Err(unsafe_path_err(format!(
            "instance temp folder `{}` exists outside missing `ao` directory",
            norm_run.display()
        ))),
    }
}

/// Unlinks one non-directory entry (file, socket, or symlink leaf) after verifying every
/// intermediate directory up to `anchor` is a real non-symlink directory inside `anchor`.
pub(super) fn remove_entry_within(
    anchor: &Path,
    target: &Path,
    removed: &mut Vec<String>,
) -> Result<bool, CliError> {
    if !verify_parent_within_anchor(anchor, target)? {
        return Ok(false);
    }
    let norm = normalize_lstat_path(target);
    match fs::symlink_metadata(&norm) {
        Ok(meta) if meta.is_dir() => Ok(false),
        Ok(_) => {
            fs::remove_file(&norm).map_err(|e| {
                CliError::failed(
                    "uninstall::io",
                    format!("removing `{}`: {e}", norm.display()),
                )
            })?;
            removed.push(norm.display().to_string());
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(CliError::failed(
            "uninstall::io",
            format!("inspecting `{}`: {e}", norm.display()),
        )),
    }
}

/// Recursively removes a verified non-symlink directory strictly contained within `anchor`
/// without following any nested symlinks.
pub(super) fn remove_tree_within(
    anchor: &Path,
    target: &Path,
    removed: &mut Vec<String>,
) -> Result<(), CliError> {
    if !verify_dir_within_anchor(anchor, target)? {
        return Ok(());
    }
    remove_tree_nofollow(&normalize_lstat_path(target), 0, removed)
}

fn remove_tree_nofollow(
    norm: &Path,
    depth: usize,
    removed: &mut Vec<String>,
) -> Result<(), CliError> {
    if depth > MAX_REMOVE_DEPTH {
        return Err(unsafe_path_err(format!(
            "directory nesting exceeded {MAX_REMOVE_DEPTH} while removing `{}`",
            norm.display()
        )));
    }
    let meta = match fs::symlink_metadata(norm) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(CliError::failed(
                "uninstall::io",
                format!("inspecting `{}`: {e}", norm.display()),
            ))
        }
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        fs::remove_file(norm).map_err(|e| {
            CliError::failed(
                "uninstall::io",
                format!("removing `{}`: {e}", norm.display()),
            )
        })?;
        removed.push(norm.display().to_string());
        return Ok(());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(norm).map_err(|e| {
        CliError::failed(
            "uninstall::io",
            format!("listing `{}`: {e}", norm.display()),
        )
    })? {
        let entry = entry.map_err(|e| {
            CliError::failed(
                "uninstall::io",
                format!("reading entry in `{}`: {e}", norm.display()),
            )
        })?;
        entries.push(normalize_lstat_path(&entry.path()));
    }
    for child in entries {
        remove_tree_nofollow(&child, depth + 1, removed)?;
    }
    fs::remove_dir(norm).map_err(|e| {
        CliError::failed(
            "uninstall::io",
            format!("removing `{}`: {e}", norm.display()),
        )
    })?;
    removed.push(norm.display().to_string());
    Ok(())
}

/// Removes an empty real directory after verifying containment within `anchor`.
pub(super) fn remove_dir_if_empty_within(
    anchor: &Path,
    target: &Path,
    removed: &mut Vec<String>,
) -> Result<bool, CliError> {
    let norm_anchor = normalize_lstat_path(anchor);
    let norm_target = normalize_lstat_path(target);
    let present = if norm_anchor == norm_target {
        verify_real_dir_if_present(&norm_target, "instance directory")?.is_some()
    } else {
        verify_dir_within_anchor(&norm_anchor, &norm_target)?
    };
    if !present {
        return Ok(false);
    }
    if fs::remove_dir(&norm_target).is_ok() {
        removed.push(norm_target.display().to_string());
        Ok(true)
    } else {
        Ok(false)
    }
}
