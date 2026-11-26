//! Collecting and validating `state/` files through the office's production projectors.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use org_office_launch::LOCAL_ONLY_NOT_RESTORED;
use org_store::DurablePath;

use crate::organization::records::{self};
use crate::store::open::validate_imported_files;

use super::git::{check_git_source, run_git_capture, PrivateTempDir};
use super::tree::{parse_ls_tree_entries, read_git_blobs};
use super::{
    ImportError, ImportedOrganization, MAX_IMPORT_DEPTH, MAX_IMPORT_FILES, MAX_IMPORT_FILE_BYTES,
    MAX_IMPORT_TOTAL_BYTES,
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
