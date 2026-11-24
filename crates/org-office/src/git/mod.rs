//! Office-local repository location records, runtime Git cache/workspace state, and
//! canonical `doc-35` filesystem layout helpers (`TASK-51`, `C7`).
//!
//! `org-office` intentionally has zero dependency on `org-git`: this module owns the
//! durable/local records, runtime state persistence, and canonical path conventions, while
//! `org-office-server` composes `Office` with `org-git` to run hardened Git subprocesses.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use org_proto::git::{CacheStateWire, WorktreeRecordWire};
use org_store::Id;
use serde::{Deserialize, Serialize};

use crate::private_fs::{private_dir, tighten_dir};

/// Office-local binding for a `kind: local` repository (`local_repository_locations`,
/// stored under `durable/local/offices/<office-id>/repositories/<repo-id>.json`, never
/// replicated or exported to Git).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalRepositoryLocation {
    pub office_id: String,
    pub repository_id: String,
    pub origin_path: String,
}

/// Persisted runtime state for one office-managed bare cache (`repository_caches`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoCacheState {
    pub repository_id: String,
    pub state: CacheStateWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bare_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Persisted runtime state for one isolated per-task worktree (`task_workspaces`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskWorkspaceRecord {
    pub office_id: String,
    pub employee_id: String,
    pub task_id: String,
    pub task_key: String,
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_key: Option<String>,
    pub branch: String,
    pub worktree_path: String,
    /// Exact Git administrative directory name under `<cache>/worktrees/<admin_dir_name>`
    /// (`doc-35` Open Question #2).
    #[serde(default)]
    pub admin_dir_name: String,
    pub base_commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_head: Option<String>,
    /// `"active"` or `"cleaned"`.
    pub state: String,
}

impl TaskWorkspaceRecord {
    pub fn to_wire(&self) -> WorktreeRecordWire {
        WorktreeRecordWire {
            office_id: self.office_id.clone(),
            employee_id: self.employee_id.clone(),
            task_id: self.task_id.clone(),
            task_key: self.task_key.clone(),
            repository_id: self.repository_id.clone(),
            repository_key: self.repository_key.clone(),
            branch: self.branch.clone(),
            worktree_path: self.worktree_path.clone(),
            base_commit: self.base_commit.clone(),
            pushed_head: self.pushed_head.clone(),
            state: self.state.clone(),
        }
    }
}

/// Persisted office runtime Git state (`<runtime-root>/<office-id>/git-state.json`).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeGitState {
    /// Keyed by `repository_id`.
    #[serde(default)]
    pub caches: BTreeMap<String, RepoCacheState>,
    /// Keyed by `<task_id>:<repository_id>:<employee_id>`.
    #[serde(default)]
    pub workspaces: BTreeMap<String, TaskWorkspaceRecord>,
}

impl OfficeGitState {
    pub fn workspace_key(task_id: &str, repository_id: &str, employee_id: &str) -> String {
        format!("{task_id}:{repository_id}:{employee_id}")
    }

    pub fn load(runtime_root: &Path, office_id: &str) -> io::Result<Self> {
        let path = git_state_path(runtime_root, office_id);
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, runtime_root: &Path, office_id: &str) -> io::Result<()> {
        let office_dir = runtime_root.join(office_id);
        private_dir(&office_dir)?;
        tighten_dir(&office_dir)?;
        let mut bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        bytes.push(b'\n');
        org_store::publish_one(&office_dir, "git-state.json", &bytes, false)
            .map_err(io::Error::other)
    }
}

/// Path to `<runtime-root>/<office-id>/git-state.json`.
pub fn git_state_path(runtime_root: &Path, office_id: &str) -> PathBuf {
    runtime_root.join(office_id).join("git-state.json")
}

/// Office-owned empty `HOME` directory for hardened Git subprocesses (`doc-35`).
pub fn git_clean_home_path(runtime_root: &Path, office_id: &str) -> PathBuf {
    runtime_root.join(office_id).join("git-home")
}

/// Canonical bare repository cache path (`doc-35` §What lives where):
/// `<runtime-root>/<office-id>/repositories/<repository-id>.git`.
pub fn bare_cache_path(runtime_root: &Path, office_id: &str, repository_id: &Id) -> PathBuf {
    runtime_root
        .join(office_id)
        .join("repositories")
        .join(format!("{repository_id}.git"))
}

/// Canonical isolated per-task worktree path (`doc-35` §What lives where):
/// `<runtime-root>/<office-id>/teams/<t1>/.../<tn>/agents/<employee-id>/[projects/<project-id>/]tasks/<task-id>/repositories/<repository-id>`.
/// Every path segment is an immutable UUIDv7 so team/project renames never move worktrees.
pub fn worktree_path(
    runtime_root: &Path,
    office_id: &str,
    team_chain: &[Id],
    employee_id: &Id,
    project_id: Option<&Id>,
    task_id: &Id,
    repository_id: &Id,
) -> PathBuf {
    let mut path = runtime_root.join(office_id);
    for team_id in team_chain {
        path = path.join("teams").join(team_id.as_str());
    }
    path = path.join("agents").join(employee_id.as_str());
    if let Some(proj) = project_id {
        path = path.join("projects").join(proj.as_str());
    }
    path.join("tasks")
        .join(task_id.as_str())
        .join("repositories")
        .join(repository_id.as_str())
}

/// Atomically stores a `local` repository's `origin_path` under
/// `durable/local/offices/<office-id>/repositories/<repository-id>.json` (`0700`/`0600`).
pub fn write_local_repo_origin(
    durable_root: &Path,
    office_id: &Id,
    repository_id: &Id,
    origin_path: &str,
) -> io::Result<()> {
    let local_dir = durable_root.join("local");
    let offices_dir = local_dir.join("offices");
    let office_dir = offices_dir.join(office_id.as_str());
    let repos_dir = office_dir.join("repositories");
    for dir in [&local_dir, &offices_dir, &office_dir, &repos_dir] {
        private_dir(dir)?;
        tighten_dir(dir)?;
    }
    let rec = LocalRepositoryLocation {
        office_id: office_id.as_str().to_owned(),
        repository_id: repository_id.as_str().to_owned(),
        origin_path: origin_path.to_owned(),
    };
    let mut bytes = serde_json::to_vec_pretty(&rec)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    bytes.push(b'\n');
    let file_name = format!("{repository_id}.json");
    org_store::publish_one(&repos_dir, &file_name, &bytes, false).map_err(io::Error::other)
}

/// Reads an office-local `origin_path` for `repository_id`, if present.
pub fn read_local_repo_origin(
    durable_root: &Path,
    office_id: &Id,
    repository_id: &Id,
) -> io::Result<Option<String>> {
    let path = durable_root
        .join("local")
        .join("offices")
        .join(office_id.as_str())
        .join("repositories")
        .join(format!("{repository_id}.json"));
    match fs::read(&path) {
        Ok(bytes) => {
            let rec: LocalRepositoryLocation = serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(rec.origin_path))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
