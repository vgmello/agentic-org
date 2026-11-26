//! Opening an existing organization from Git repository files (`TASK-39` / R1, `doc-91`
//! "Opening an existing organization", `doc-61` D25).
//!
//! Clones into a bare temporary repository with strict protocol/option/timeout bounds, inspects
//! the tree via `git ls-tree -r -l -z HEAD` and reads regular blobs via `git cat-file --batch`
//! without checking out a working tree (eliminating zip-bomb disk DoS and catching Git symlinks
//! `120000` and submodules `160000`), validates every file through the office's real production
//! projectors (`OrganizationFiles` and `ProjectFiles`) without guessing or repairing, reports any
//! invalid or unrecognized file by path and reason, and builds Entry 1 for a new HQ office.

use std::collections::BTreeMap;

use org_store::{DurablePath, Id};
use serde::{Deserialize, Serialize};

/// Re-exported canonical summary of local-only data that never lives in Git (`doc-91`).
pub use org_office_launch::LOCAL_ONLY_NOT_RESTORED;

mod decide;
mod git;
mod tree;
mod validate;

pub use decide::{decide_import, next_office_code, ImportOrganization};
pub use git::check_git_source;
pub(crate) use git::PrivateTempDir;
pub use validate::{clone_and_validate_state, collect_state_files, validate_state_dir};

/// Maximum size of any single imported state file (4 MiB).
const MAX_IMPORT_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum total bytes across all imported state files (12 MiB, leaving safe headroom below
/// `org_store::MAX_ENTRY_BYTES` = 16 MiB for journal envelope, new HQ records, and CA escrow).
const MAX_IMPORT_TOTAL_BYTES: u64 = 12 * 1024 * 1024;
/// Maximum number of state files accepted in a single import.
const MAX_IMPORT_FILES: usize = 4_096;
/// Maximum directory depth inside `state/`.
const MAX_IMPORT_DEPTH: usize = 32;

/// An active person record discovered in an imported organization so the administrator can pick
/// which person record belongs to them as organization owner.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ImportedPerson {
    pub id: String,
    pub name: String,
    pub handle: String,
    pub home_team_id: String,
}

/// Summary extracted by validating an imported organization through the real projectors.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValidatedImport {
    pub organization_id: Id,
    pub name: String,
    pub slug: String,
    pub canonical_remote_url: Option<String>,
    pub hq_office_id: Id,
    pub hq_office_name: String,
    pub root_team_id: Option<Id>,
    pub has_active_root_team: bool,
    pub teams_count: usize,
    pub projects_count: usize,
    pub agents_count: usize,
    pub people: Vec<ImportedPerson>,
    pub reserved_handles: Vec<String>,
    pub reserved_entity_ids: Vec<String>,
}

/// Complete validated import ready for preview or Entry 1 HQ import.
#[derive(Clone, Debug)]
pub struct ImportedOrganization {
    pub summary: ValidatedImport,
    pub repository_remote_url: String,
    pub state_branch: String,
    pub commit_oid: Option<String>,
    pub local_only_not_restored: Vec<String>,
    pub files: BTreeMap<DurablePath, Vec<u8>>,
}

/// Failure while cloning, reading, or validating an existing organization from Git.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ImportError {
    InvalidInput(String),
    GitFailed(String),
    InvalidFile { path: String, reason: String },
    Storage(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::InvalidInput(msg) => f.write_str(msg),
            ImportError::GitFailed(msg) => write!(f, "git repository could not be read: {msg}"),
            ImportError::InvalidFile { path, reason } => write!(f, "{path}: {reason}"),
            ImportError::Storage(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ImportError {}

#[cfg(test)]
mod tests;
