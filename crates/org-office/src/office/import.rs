//! Opening an existing organization from Git repository files, the office's part: the files a
//! clone or a local `state/` directory yields are validated through the office store's
//! production projectors (`crate::store::open::validate_imported_files`) before anything is
//! built from them. The organization domain (`crate::organization::import`) owns the limits,
//! the Git reading and the Entry 1 decision; it never reaches into the store.

use std::collections::BTreeMap;
use std::path::Path;

use org_office_launch::LOCAL_ONLY_NOT_RESTORED;
use org_store::DurablePath;

use crate::organization::import::{
    check_git_source, collect_state_files, parse_ls_tree_entries, read_git_blobs, run_git_capture,
    ImportError, ImportedOrganization, PrivateTempDir,
};
use crate::organization::records;
use crate::store::open::validate_imported_files;

fn validate_files_map(
    files: BTreeMap<DurablePath, Vec<u8>>,
    remote_url: &str,
    state_branch: &str,
    commit_oid: Option<String>,
) -> Result<ImportedOrganization, ImportError> {
    let org_path = records::organization_path();
    let org_bytes = files
        .get(&org_path)
        .ok_or_else(|| ImportError::InvalidFile {
            path: org_path.as_str().to_owned(),
            reason: "the organization record is missing".into(),
        })?;
    let org_record =
        records::decode_organization(org_bytes).map_err(|e| ImportError::InvalidFile {
            path: org_path.as_str().to_owned(),
            reason: e.to_string(),
        })?;
    let summary = validate_imported_files(&org_record.id, &files)?;
    Ok(ImportedOrganization {
        summary,
        repository_remote_url: remote_url.to_owned(),
        state_branch: state_branch.to_owned(),
        commit_oid,
        local_only_not_restored: LOCAL_ONLY_NOT_RESTORED
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        files,
    })
}

/// Validates a local directory of organization state files against the full data contract.
pub fn validate_state_dir(
    state_root: &Path,
    remote_url: &str,
    state_branch: &str,
) -> Result<ImportedOrganization, ImportError> {
    let files = collect_state_files(state_root)?;
    validate_files_map(files, remote_url, state_branch, None)
}

/// Clones `state_branch` from `remote_url` into a bare temporary repository (no working-tree
/// checkout), inspects the Git tree for symlinks/submodules/size/count limits, reads regular
/// blobs via `git cat-file --batch`, and validates every file through the production projectors.
pub fn clone_and_validate_state(
    remote_url: &str,
    state_branch: &str,
) -> Result<ImportedOrganization, ImportError> {
    check_git_source(remote_url, state_branch)?;
    let staging = PrivateTempDir::create("org-import")?;
    let repo_dir = staging.path().join("repo.git");
    let repo_arg = repo_dir
        .to_str()
        .ok_or_else(|| ImportError::Storage("staging path is not valid UTF-8".into()))?;
    run_git_capture(
        &[
            "clone",
            "--bare",
            "--depth",
            "1",
            "--single-branch",
            "--branch",
            state_branch,
            "--",
            remote_url,
            repo_arg,
        ],
        None,
        staging.path(),
        "clone failed",
    )?;
    let rev_bytes = run_git_capture(
        &["rev-parse", "--verify", "HEAD"],
        Some(&repo_dir),
        staging.path(),
        "could not resolve branch head",
    )?;
    let commit_oid = String::from_utf8_lossy(&rev_bytes).trim().to_owned();
    let tree_bytes = run_git_capture(
        &["ls-tree", "-r", "-l", "-z", "HEAD"],
        Some(&repo_dir),
        staging.path(),
        "could not list repository files",
    )?;
    let blobs = parse_ls_tree_entries(&tree_bytes)?;
    let files = read_git_blobs(&repo_dir, &blobs, staging.path())?;
    validate_files_map(files, remote_url, state_branch, Some(commit_oid))
}
