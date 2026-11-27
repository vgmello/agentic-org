//! The built-in Chief of Staff's private home as an operator workspace (`OperatorSandboxed`,
//! `TASK-80.1`): she works from it and nowhere else, and before each of her launches the office
//! leaves exactly one execpolicy in the folder Codex loads rules from, its own.

use std::io::ErrorKind;
use std::path::Path;

use org_agent_runtime_codex::{OPERATOR_RULES_DIR, OPERATOR_RULES_FILE_NAME};
use org_office::organization_family::records::{BuiltinKey, Employee};
use org_proto::{ApiError, ErrorCode};

/// The built-in Chief of Staff works from her empty, repo-less private home and nowhere else:
/// a session of hers that names another workspace (a repository, a worktree) is refused before
/// anything is planned. Her home's Codex rules folder is then cleared of everything but the
/// office's own execpolicy ([`clean_operator_rules`]), whatever runtime she runs on.
pub(super) fn ensure_operator_home(
    employee: &Employee,
    home: &Path,
    workspace: &Path,
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
    clean_operator_rules(workspace)
}

/// Removes every entry of `workspace`'s Codex rules folder ([`OPERATOR_RULES_DIR`]) except a
/// regular file named [`OPERATOR_RULES_FILE_NAME`], which the launch rewrites: no stale or
/// planted `.rules` file loosens the office's execpolicy. Links are never followed: a linked
/// `.codex` or rules folder (or any linked entry) is removed as a link, and the launch makes
/// real folders again. Fails closed: an entry it cannot remove refuses the launch.
pub(crate) fn clean_operator_rules(workspace: &Path) -> Result<(), ApiError> {
    let refused = || {
        ApiError::new(
            ErrorCode::StorageError,
            "could not clear the operator's execpolicy folder",
        )
    };
    let codex = workspace.join(".codex");
    let rules = workspace.join(OPERATOR_RULES_DIR);
    for folder in [&codex, &rules] {
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
        if kind.is_file() && entry.file_name() == OPERATOR_RULES_FILE_NAME {
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
