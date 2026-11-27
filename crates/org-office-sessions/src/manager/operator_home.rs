//! The built-in Chief of Staff's private home as an operator workspace (`OperatorSandboxed`,
//! `TASK-80.1`): she works from it and nowhere else, and before each of her launches the office
//! cleans every registered runtime's operator execpolicy folder so only the office's own rules
//! file remains.

use std::io::ErrorKind;
use std::path::Path;

use org_agent_runtime::OperatorRulesLayout;
use org_office::organization_family::records::{BuiltinKey, Employee};
use org_proto::{ApiError, ErrorCode};

use crate::runtime_registry::RuntimeRegistry;

/// The built-in Chief of Staff works from her empty, repo-less private home and nowhere else:
/// a session of hers that names another workspace (a repository, a worktree) is refused before
/// anything is planned. Every registered runtime's operator rules folder in her home is then
/// cleared of everything but the office's own execpolicy
/// ([`clean_operator_rules_for_runtimes`]), whatever runtime she runs on.
pub(super) fn ensure_operator_home(
    employee: &Employee,
    home: &Path,
    workspace: &Path,
    runtimes: &RuntimeRegistry,
) -> Result<(), ApiError> {
    if employee.builtin_key != Some(BuiltinKey::ChiefOfStaff) {
        return Ok(());
    }
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    if workspace != home {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "the Chief of Staff works only from her private home",
        ));
    }
    clean_operator_rules_for_runtimes(workspace, runtimes)
}

/// Cleans the operator execpolicy folder declared by every registered runtime adapter in
/// `runtimes`. Fails closed on any unreadable or unremovable entry.
pub(crate) fn clean_operator_rules_for_runtimes(
    workspace: &Path,
    runtimes: &RuntimeRegistry,
) -> Result<(), ApiError> {
    for adapter in runtimes.adapters() {
        if let Some(layout) = adapter.operator_rules_layout() {
            clean_operator_rules_layout(workspace, layout)?;
        }
    }
    Ok(())
}

/// Test convenience wrapper cleaning operator rules for all adapters in
/// [`RuntimeRegistry::production`].
#[cfg(test)]
pub(crate) fn clean_operator_rules(workspace: &Path) -> Result<(), ApiError> {
    clean_operator_rules_for_runtimes(workspace, &RuntimeRegistry::production())
}

/// Removes every entry of `workspace`'s `layout.rules_dir` except a regular file named
/// `layout.office_rules_file`, which the launch rewrites: no stale or planted rules file loosens
/// the office's execpolicy. Links are never followed: a linked ancestor or rules folder (or any
/// linked entry) is removed as a link, and the launch makes real folders again. Fails closed: an
/// entry it cannot remove refuses the launch.
fn clean_operator_rules_layout(
    workspace: &Path,
    layout: OperatorRulesLayout,
) -> Result<(), ApiError> {
    let refused = || {
        ApiError::new(
            ErrorCode::StorageError,
            "could not clear the operator's execpolicy folder",
        )
    };
    let rules = workspace.join(layout.rules_dir);
    let folders: Vec<_> = layout
        .ancestor_dirs
        .iter()
        .map(|rel| workspace.join(rel))
        .chain(std::iter::once(rules.clone()))
        .collect();
    for folder in &folders {
        match folder.symlink_metadata() {
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(refused()),
            Ok(meta) if meta.is_dir() => {}
            // A link or a file where a folder belongs: removed, never followed.
            Ok(_) => return std::fs::remove_file(folder).map_err(|_| refused()),
        }
    }
    for entry in std::fs::read_dir(&rules).map_err(|_| refused())? {
        let entry = entry.map_err(|_| refused())?;
        // `DirEntry::file_type` does not follow a link.
        let kind = entry.file_type().map_err(|_| refused())?;
        if kind.is_file() && entry.file_name() == layout.office_rules_file {
            continue;
        }
        let path = entry.path();
        let removed = if kind.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed.map_err(|_| refused())?;
    }
    Ok(())
}
