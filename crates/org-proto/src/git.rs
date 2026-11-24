//! Wire contracts and typed RPCs for shared repository registration, per-project integration
//! branches, office-managed bare Git caches, isolated per-task worktrees, and office Git push
//! (`TASK-51`, `C7`, `doc-35`, `FC-103`, `FC-139`, `FC-140`).

use serde::{Deserialize, Serialize};

use crate::Call;

/// Registered repository kind (`doc-35`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryKindWire {
    Remote,
    Local,
}

impl RepositoryKindWire {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::Local => "local",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "remote" => Some(Self::Remote),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

/// Office bare-cache readiness state (`doc-35` G1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStateWire {
    Unprepared,
    Cloning,
    Ready,
    Failed,
}

/// One registered organization repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRecordWire {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub kind: RepositoryKindWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    /// Populated when listed for a specific project (`repo list --project`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_branch: Option<String>,
    /// Office-local cache readiness.
    pub cache_state: CacheStateWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_reason: Option<String>,
    pub revision: u64,
}

/// One project-to-repository link with its required `integration_branch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRepositoryLinkWire {
    pub project_id: String,
    pub project_key: String,
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_key: Option<String>,
    pub integration_branch: String,
    pub revision: u64,
}

/// One isolated per-task worktree managed by this office (`worktree list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeRecordWire {
    pub office_id: String,
    pub employee_id: String,
    pub task_id: String,
    pub task_key: String,
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_key: Option<String>,
    pub branch: String,
    pub worktree_path: String,
    pub base_commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_head: Option<String>,
    pub state: String,
}

/// `repo.register` params: owner registers a remote or local repository in the organization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRegisterParams {
    pub command_id: String,
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub kind: RepositoryKindWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    /// Required for `kind: local` (stored office-locally, never in shared `repositories.yaml`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRegisterResult {
    pub repository: RepositoryRecordWire,
}

/// `repo.list` params (`doc-28` §3.6).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RepoListParams {
    /// Optional project UUIDv7, `P-<N>` key, or slug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoListResult {
    pub repositories: Vec<RepositoryRecordWire>,
}

/// `repo.attach` params (`doc-28` §3.6): links a registered repository to a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoAttachParams {
    pub command_id: String,
    /// Project UUIDv7, `P-<N>` key, or slug.
    pub project: String,
    /// Repository UUIDv7 or key.
    pub repository: String,
    /// Required integration branch checked via hardened `git ls-remote` before commit.
    pub integration_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoAttachResult {
    pub link: ProjectRepositoryLinkWire,
}

/// `repo.status` params (`doc-35` G1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatusParams {
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatusResult {
    pub office_id: String,
    pub repository_id: String,
    pub state: CacheStateWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bare_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `worktree.list` params (`doc-28` §3.6).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorktreeListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeListResult {
    pub worktrees: Vec<WorktreeRecordWire>,
}

/// `worktree.prepare` params: office provisions an isolated worktree for `(task, repository)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreePrepareParams {
    pub command_id: String,
    pub task: String,
    pub repository: String,
    /// Optional base branch for projectless tasks (`doc-35` Open Question #6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreePrepareResult {
    pub worktree: WorktreeRecordWire,
    pub writable_roots: Vec<String>,
    pub fixed_write_denies: Vec<String>,
}

/// `git.push` params (`doc-35` G1, `doc-28` §3.6 `repo push`). Branch is never caller-supplied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitPushParams {
    pub task: String,
    /// Optional when the task targets exactly one repository (`doc-35` G1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitPushResult {
    pub task_id: String,
    pub repository_id: String,
    pub branch: String,
    pub commit: String,
}

pub struct RepoRegister;
impl Call for RepoRegister {
    const METHOD: &'static str = "repo.register";
    type Params = RepoRegisterParams;
    type Result = RepoRegisterResult;
}

pub struct RepoList;
impl Call for RepoList {
    const METHOD: &'static str = "repo.list";
    type Params = RepoListParams;
    type Result = RepoListResult;
}

pub struct RepoAttach;
impl Call for RepoAttach {
    const METHOD: &'static str = "repo.attach";
    type Params = RepoAttachParams;
    type Result = RepoAttachResult;
}

pub struct RepoStatus;
impl Call for RepoStatus {
    const METHOD: &'static str = "repo.status";
    type Params = RepoStatusParams;
    type Result = RepoStatusResult;
}

pub struct WorktreeList;
impl Call for WorktreeList {
    const METHOD: &'static str = "worktree.list";
    type Params = WorktreeListParams;
    type Result = WorktreeListResult;
}

pub struct WorktreePrepare;
impl Call for WorktreePrepare {
    const METHOD: &'static str = "worktree.prepare";
    type Params = WorktreePrepareParams;
    type Result = WorktreePrepareResult;
}

pub struct GitPush;
impl Call for GitPush {
    const METHOD: &'static str = "git.push";
    type Params = GitPushParams;
    type Result = GitPushResult;
}
