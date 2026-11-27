//! Wire contracts and typed RPCs for shared repository registration, per-project integration
//! branches, office-managed bare Git caches, on-demand origin fetch, isolated per-task worktrees,
//! and office Git push (`TASK-51`, `TASK-173`, `C7`, `doc-35`, `FC-103`, `FC-139`, `FC-140`).

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

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

/// Most branch names each [`RepoFetchResult`] change list carries; its `*_count` stays exact.
pub const REPO_FETCH_MAX_LISTED_BRANCHES: usize = 50;

/// `repo.fetch` params (`TASK-173`): the office refreshes its bare cache of the repository's
/// origin with office-side credentials, so `refs/remotes/origin/*` and tags in every task
/// worktree of this office see newly pushed remote branches. Never touches `refs/heads/*`,
/// a worktree's working tree, or its checked-out branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoFetchParams {
    /// Repository UUIDv7 or key.
    pub repository: String,
}

/// `repo.fetch` result: the refreshed cache and how `refs/remotes/origin/*` changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoFetchResult {
    pub office_id: String,
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_key: Option<String>,
    pub state: CacheStateWire,
    /// The origin's `HEAD` branch, when the origin advertises one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// Commit OID of `origin/<default_branch>` after the fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_oid: Option<String>,
    /// Number of `origin/*` branches in the cache after the fetch.
    pub branch_count: u64,
    /// New `origin/*` branches (at most [`REPO_FETCH_MAX_LISTED_BRANCHES`] names, sorted).
    pub added: Vec<String>,
    pub added_count: u64,
    /// `origin/*` branches whose commit moved (at most [`REPO_FETCH_MAX_LISTED_BRANCHES`]).
    pub updated: Vec<String>,
    pub updated_count: u64,
    /// `origin/*` branches removed because the origin no longer has them (capped likewise).
    pub pruned: Vec<String>,
    pub pruned_count: u64,
    /// When the fetch completed (`YYYY-MM-DDTHH:MM:SS.mmmZ`).
    pub fetched_at: String,
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

/// Non-secret status view of an office's scoped Git credential for one repository (`TASK-135`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoCredentialView {
    pub office_id: String,
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_key: Option<String>,
    pub configured: bool,
    /// Credential source (`"vault"`, `"host_helper"`, `"host_ssh"`, or `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Canonical repository URL scope the credential is bound to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_url: Option<String>,
    /// Non-secret HTTP username (e.g. `"x-access-token"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// `repo.credential_set` params: owner stores an encrypted per-repository scoped HTTPS token in
/// this office's local credential vault (outside `durable/`, `runtime/`, and every agent home).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoCredentialSetParams {
    /// Repository UUIDv7 or key.
    pub repository: String,
    /// Optional HTTP username (defaults to `"x-access-token"` when omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Scoped repository HTTPS personal access token or fine-grained token. Zeroized on drop.
    pub token: String,
}

impl Drop for RepoCredentialSetParams {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

impl std::fmt::Debug for RepoCredentialSetParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoCredentialSetParams")
            .field("repository", &self.repository)
            .field("username", &self.username)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// `repo.credential_status` params: inspect non-secret repository credential configuration on this office.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoCredentialStatusParams {
    /// Repository UUIDv7 or key.
    pub repository: String,
}

/// `repo.credential_revoke` params: remove a per-repository scoped HTTPS token from this office's vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoCredentialRevokeParams {
    /// Repository UUIDv7 or key.
    pub repository: String,
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

pub struct RepoFetch;
impl Call for RepoFetch {
    const METHOD: &'static str = "repo.fetch";
    type Params = RepoFetchParams;
    type Result = RepoFetchResult;
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

pub struct RepoCredentialSet;
impl Call for RepoCredentialSet {
    const METHOD: &'static str = "repo.credential_set";
    type Params = RepoCredentialSetParams;
    type Result = RepoCredentialView;
}

pub struct RepoCredentialStatus;
impl Call for RepoCredentialStatus {
    const METHOD: &'static str = "repo.credential_status";
    type Params = RepoCredentialStatusParams;
    type Result = RepoCredentialView;
}

pub struct RepoCredentialRevoke;
impl Call for RepoCredentialRevoke {
    const METHOD: &'static str = "repo.credential_revoke";
    type Params = RepoCredentialRevokeParams;
    type Result = RepoCredentialView;
}
