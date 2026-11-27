//! Office Git service and RPC dispatcher (`TASK-51`, `C7`, `doc-35`, `FC-103`, `FC-139`, `FC-140`).
//!
//! Composes [`org_office::Office`] with [`org_git::GitRunner`] so `org-office` never depends
//! on `org-git` while the office enforces:
//! - HTTPS / SSH only for remote repos; office-local `origin_path` for `kind: local` repos;
//! - hardened `git ls-remote` verification before `repo.attach`;
//! - on-demand, rate-limited `repo.fetch` of a bare cache with office-side credentials only;
//! - per-repository locking around bare caches and isolated `task/<key>-<slug>` worktrees;
//! - office-mediated non-force `git.push` (caller-owned task workspace only, branch from durable
//!   `task_repositories` + runtime state, never targeting integration branches, `main`, or `master`).

mod cache;
mod credentials;
mod fetch;
mod push;
mod redaction;
mod worktree;

use std::sync::{Arc, Mutex};

use crate::live::LiveEvents;
use org_git::{ls_remote_heads, GitRunner, RepoLocks};
use org_office::git::{git_clean_home_path, OfficeGitState, RepoCacheState, TaskWorkspaceRecord};
use org_office::organization_family::records::{Repository, RepositoryKind};
use org_office::Office;
use org_proto::git::{
    CacheStateWire, GitPush, RepoAttach, RepoAttachParams, RepoAttachResult, RepoFetch, RepoList,
    RepoListParams, RepoListResult, RepoRegister, RepoRegisterParams, RepoRegisterResult,
    RepoStatus, RepoStatusParams, RepoStatusResult, RepositoryKindWire, RepositoryRecordWire,
    WorktreeList, WorktreeListParams, WorktreeListResult, WorktreePrepare,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use self::credentials::{
    record_or_verify_local_origin, resolve_remote_spec, validate_register_origin,
};
use self::fetch::{FetchGate, FETCH_COOLDOWN};
use self::redaction::{map_git_error, map_office_error};

/// Whether `request` belongs to the C7 Git/repository/worktree RPC family.
pub(crate) fn is_git_method(request: &Request) -> bool {
    request.is::<RepoRegister>()
        || request.is::<RepoList>()
        || request.is::<RepoAttach>()
        || request.is::<RepoStatus>()
        || request.is::<RepoFetch>()
        || request.is::<WorktreeList>()
        || request.is::<WorktreePrepare>()
        || request.is::<GitPush>()
}

/// Live Git orchestration service composed over [`Office`], [`GitRunner`], and [`RepoLocks`].
pub(crate) struct GitService {
    office: Arc<Office>,
    live: Arc<LiveEvents>,
    locks: Arc<RepoLocks>,
    state: Mutex<OfficeGitState>,
    runner: Mutex<Option<GitRunner>>,
    fetch_gate: FetchGate,
}

impl GitService {
    #[cfg(test)]
    pub(crate) fn new(office: Arc<Office>, live: Arc<LiveEvents>) -> Self {
        Self::with_locks(office, live, Arc::new(RepoLocks::new()))
    }

    pub(crate) fn with_locks(
        office: Arc<Office>,
        live: Arc<LiveEvents>,
        locks: Arc<RepoLocks>,
    ) -> Self {
        let loaded = OfficeGitState::load(office.runtime_root(), &office.info().office_id)
            .unwrap_or_else(|e| {
                eprintln!("agentic-org-office: failed loading git-state.json: {e}");
                OfficeGitState::default()
            });
        Self {
            office,
            live,
            locks,
            state: Mutex::new(loaded),
            runner: Mutex::new(None),
            fetch_gate: FetchGate::new(FETCH_COOLDOWN),
        }
    }

    pub(crate) fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if !is_git_method(request) {
            return None;
        }
        let Some(member) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            ));
        };
        if request.is::<RepoRegister>() {
            return Some(run_call::<RepoRegister>(request, |p| {
                self.handle_register(member, p)
            }));
        }
        if request.is::<RepoList>() {
            return Some(run_call::<RepoList>(request, |p| self.handle_list(p)));
        }
        if request.is::<RepoAttach>() {
            return Some(run_call::<RepoAttach>(request, |p| {
                self.handle_attach(member, p)
            }));
        }
        if request.is::<RepoStatus>() {
            return Some(run_call::<RepoStatus>(request, |p| {
                self.handle_status(member, p)
            }));
        }
        if request.is::<RepoFetch>() {
            return Some(run_call::<RepoFetch>(request, |p| self.handle_fetch(p)));
        }
        if request.is::<WorktreeList>() {
            return Some(run_call::<WorktreeList>(request, |p| {
                self.handle_worktree_list(member, p)
            }));
        }
        if request.is::<WorktreePrepare>() {
            return Some(run_call::<WorktreePrepare>(request, |p| {
                self.handle_worktree_prepare(member, p)
            }));
        }
        if request.is::<GitPush>() {
            return Some(run_call::<GitPush>(request, |p| {
                self.handle_git_push(member, p)
            }));
        }
        None
    }

    fn runner(&self) -> Result<GitRunner, ApiError> {
        let mut guard = self.runner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.clone());
        }
        let clean_home =
            git_clean_home_path(self.office.runtime_root(), &self.office.info().office_id);
        let created = GitRunner::new(clean_home).map_err(map_git_error)?;
        *guard = Some(created.clone());
        Ok(created)
    }

    fn save_state_locked(&self, state: &OfficeGitState) -> Result<(), ApiError> {
        state
            .save(self.office.runtime_root(), &self.office.info().office_id)
            .map_err(|e| {
                eprintln!("agentic-org-office: save git-state.json failed: {e}");
                ApiError::new(
                    ErrorCode::StorageError,
                    "failed to persist office Git state",
                )
            })
    }

    fn handle_register(
        &self,
        member: &str,
        params: RepoRegisterParams,
    ) -> Result<RepoRegisterResult, ApiError> {
        if member != self.office.owner_member_id() {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only the organization owner may register repositories",
            ));
        }
        let validated_local_origin = validate_register_origin(&params)?;

        let record = self
            .office
            .register_repository(
                member,
                &params.command_id,
                &params.repository_id,
                params.key,
                params.kind,
                params.remote_url,
            )
            .map_err(map_office_error)??;

        if let Some(origin_path) = validated_local_origin {
            record_or_verify_local_origin(&self.office, &record, &origin_path)?;
        }
        self.live.record_mailbox_change();

        let state_guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let cache = state_guard.caches.get(record.id.as_str());
        Ok(RepoRegisterResult {
            repository: to_repo_wire(&record, None, cache),
        })
    }

    fn handle_list(&self, params: RepoListParams) -> Result<RepoListResult, ApiError> {
        let listed = self
            .office
            .list_repositories(params.project.as_deref())
            .map_err(map_office_error)??;
        let state_guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let repositories = listed
            .into_iter()
            .map(|entry| {
                let cache = state_guard.caches.get(entry.record.id.as_str());
                to_repo_wire(&entry.record, entry.integration_branch, cache)
            })
            .collect();
        Ok(RepoListResult { repositories })
    }

    fn handle_attach(
        &self,
        member: &str,
        params: RepoAttachParams,
    ) -> Result<RepoAttachResult, ApiError> {
        if member != self.office.owner_member_id() {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only the organization owner may attach repositories to projects",
            ));
        }
        let branch = params.integration_branch.trim();
        if branch.is_empty() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "integration_branch is required",
            ));
        }
        let repo = self
            .office
            .resolve_repository(&params.repository)
            .map_err(map_office_error)?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;
        let remote = resolve_remote_spec(&self.office, &repo)?;
        let runner = self.runner()?;
        let heads = ls_remote_heads(&runner, &remote).map_err(map_git_error)?;
        if !heads.contains_key(branch) {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("integration branch `{branch}` does not exist on the repository origin"),
            ));
        }
        let link = self
            .office
            .attach_project_repository(
                member,
                &params.command_id,
                &params.project,
                repo.id.as_str(),
                branch.to_owned(),
            )
            .map_err(map_office_error)??;
        self.live.record_mailbox_change();
        Ok(RepoAttachResult { link })
    }

    fn handle_status(
        &self,
        member: &str,
        params: RepoStatusParams,
    ) -> Result<RepoStatusResult, ApiError> {
        let repo = self
            .office
            .resolve_repository(&params.repository)
            .map_err(map_office_error)?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;
        let is_owner = member == self.office.owner_member_id();
        let state_guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let cache = state_guard.caches.get(repo.id.as_str());
        Ok(RepoStatusResult {
            office_id: self.office.info().office_id.clone(),
            repository_id: repo.id.as_str().to_owned(),
            state: cache.map_or(CacheStateWire::Unprepared, |c| c.state),
            bare_path: if is_owner {
                cache.and_then(|c| c.bare_path.clone())
            } else {
                None
            },
            reason: cache.and_then(|c| c.reason.clone()),
        })
    }

    fn handle_worktree_list(
        &self,
        member: &str,
        params: WorktreeListParams,
    ) -> Result<WorktreeListResult, ApiError> {
        let filter_task_id = match params.task.as_deref() {
            Some(task_ref) => {
                let inspected = self
                    .office
                    .task_inspect(task_ref)
                    .map_err(map_office_error)??;
                Some(inspected.task.id)
            }
            None => None,
        };
        let is_owner = member == self.office.owner_member_id();
        let state_guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut worktrees: Vec<_> = state_guard
            .workspaces
            .values()
            .filter(|w| {
                w.state == "active"
                    && (is_owner || w.employee_id == member)
                    && filter_task_id.as_deref().is_none_or(|tid| w.task_id == tid)
            })
            .map(TaskWorkspaceRecord::to_wire)
            .collect();
        worktrees.sort_by(|a, b| {
            a.task_key
                .cmp(&b.task_key)
                .then_with(|| a.repository_id.cmp(&b.repository_id))
        });
        Ok(WorktreeListResult { worktrees })
    }
}

fn to_repo_wire(
    record: &Repository,
    integration_branch: Option<String>,
    cache: Option<&RepoCacheState>,
) -> RepositoryRecordWire {
    RepositoryRecordWire {
        id: record.id.as_str().to_owned(),
        key: record.key.clone(),
        kind: match record.kind {
            RepositoryKind::Remote => RepositoryKindWire::Remote,
            RepositoryKind::Local => RepositoryKindWire::Local,
        },
        remote_url: record.remote_url.clone(),
        integration_branch,
        cache_state: cache.map_or(CacheStateWire::Unprepared, |c| c.state),
        cache_reason: cache.and_then(|c| c.reason.clone()),
        revision: record.meta.revision,
    }
}

fn run_call<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let params = match request.params::<C>() {
        Ok(p) => p,
        Err(e) => {
            return Response::error(
                request.id,
                ApiError::new(ErrorCode::InvalidRequest, e.to_string()),
            );
        }
    };
    match handler(params) {
        Ok(res) => match Response::ok::<C>(request.id, &res) {
            Ok(resp) => resp,
            Err(e) => Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, e.to_string()),
            ),
        },
        Err(api_err) => Response::error(request.id, api_err),
    }
}

#[cfg(test)]
mod fetch_tests;
#[cfg(test)]
mod tests;
