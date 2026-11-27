//! The one validator for repository-relative paths read from a commit (`TASK-92.1`): a diff's
//! path filter and a file whose lines are read. A path is refused, never normalized, and the
//! callers pass Git only what this returns, after `--` and with literal pathspecs.

use crate::GitError;

/// Longest repository-relative path accepted, in bytes.
pub const MAX_REPO_PATH_BYTES: usize = 4096;

/// Accepts a normalized repository-relative path: 1 to [`MAX_REPO_PATH_BYTES`] bytes, no
/// leading `/` or `-`, no control characters (NUL included) or `\`, and `/`-separated
/// components that are never empty, `.`, `..` or `.git` (in any letter case).
pub fn validate_repo_path(raw: &str) -> Result<&str, GitError> {
    if raw.is_empty() || raw.len() > MAX_REPO_PATH_BYTES {
        return Err(GitError::InvalidInput(format!(
            "repository path must be 1 to {MAX_REPO_PATH_BYTES} bytes"
        )));
    }
    if raw.starts_with('/') {
        return Err(GitError::InvalidInput(
            "repository path must be relative (no leading `/`)".into(),
        ));
    }
    if raw.starts_with('-') {
        return Err(GitError::InvalidInput(
            "repository path must not start with `-`".into(),
        ));
    }
    if raw.chars().any(|c| c.is_control() || c == '\\') {
        return Err(GitError::InvalidInput(
            "repository path must not contain control characters or `\\`".into(),
        ));
    }
    for component in raw.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(GitError::InvalidInput(
                "repository path must be normalized: no empty, `.` or `..` components".into(),
            ));
        }
        if component.eq_ignore_ascii_case(".git") {
            return Err(GitError::InvalidInput(
                "repository path must not name a `.git` component".into(),
            ));
        }
    }
    Ok(raw)
}
