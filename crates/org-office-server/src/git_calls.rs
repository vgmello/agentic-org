//! Office Git service and RPC dispatcher (`TASK-51`, `C7`, `doc-35`, `FC-103`, `FC-139`, `FC-140`).
//!
//! Composes [`org_office::Office`] with [`org_git::GitRunner`] so `org-office` never depends
//! on `org-git` while the office enforces:
//! - HTTPS / SSH only for remote repos; office-local `origin_path` for `kind: local` repos;
//! - hardened `git ls-remote` verification before `repo.attach`;
//! - per-repository locking around bare caches and isolated `task/<key>-<slug>` worktrees;
//! - office-mediated non-force `git.push` (caller-owned task workspace only, branch from durable
//!   `task_repositories` + runtime state, never targeting integration branches, `main`, or `master`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use org_git::{
    choose_unique_task_branch, create_worktree, derive_sandbox_rules, fetch_cache, ls_remote_heads,
    push_task_branch, resolve_remote_branch_commit, rollback_created_worktree,
    sanitize_worktree_links, validate_local_origin_path, validate_remote_url,
    verify_cache_integrity, AllowedProtocol, GitCredential, GitError, GitRunner, RemoteSpec,
    RepoLocks,
};
use org_office::git::{
    bare_cache_path, git_clean_home_path, worktree_path, OfficeGitState, RepoCacheState,
    TaskWorkspaceRecord,
};
use org_office::organization_family::records::{Repository, RepositoryKind};
use org_office::Office;
use org_proto::git::{
    CacheStateWire, GitPush, GitPushParams, GitPushResult, RepoAttach, RepoAttachParams,
    RepoAttachResult, RepoList, RepoListParams, RepoListResult, RepoRegister, RepoRegisterParams,
    RepoRegisterResult, RepoStatus, RepoStatusParams, RepoStatusResult, RepositoryKindWire,
    RepositoryRecordWire, WorktreeList, WorktreeListParams, WorktreeListResult, WorktreePrepare,
    WorktreePrepareParams, WorktreePrepareResult,
};
use org_proto::tasks::TaskStatus;
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::live::LiveEvents;

/// Whether `request` belongs to the C7 Git/repository/worktree RPC family.
pub(crate) fn is_git_method(request: &Request) -> bool {
    request.is::<RepoRegister>()
        || request.is::<RepoList>()
        || request.is::<RepoAttach>()
        || request.is::<RepoStatus>()
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
}

impl GitService {
    pub(crate) fn new(office: Arc<Office>, live: Arc<LiveEvents>) -> Self {
        let loaded = OfficeGitState::load(office.runtime_root(), &office.info().office_id)
            .unwrap_or_else(|e| {
                eprintln!("agentic-org-office: failed loading git-state.json: {e}");
                OfficeGitState::default()
            });
        Self {
            office,
            live,
            locks: Arc::new(RepoLocks::new()),
            state: Mutex::new(loaded),
            runner: Mutex::new(None),
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

    fn handle_worktree_prepare(
        &self,
        member: &str,
        params: WorktreePrepareParams,
    ) -> Result<WorktreePrepareResult, ApiError> {
        if !org_proto::is_uuid_v7(&params.command_id) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id must be a canonical lowercase UUIDv7",
            ));
        }
        let ctx = self
            .office
            .task_git_context(member, &params.task)
            .map_err(map_office_error)??;
        if matches!(ctx.task.status, TaskStatus::Done | TaskStatus::Cancelled) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "cannot prepare a worktree for a closed or cancelled task",
            ));
        }
        let repo = org_office::projects::features::resolve_repository(
            &ctx.repositories,
            &params.repository,
        )
        .cloned()
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;
        authorize_worktree_prepare(self.office.owner_member_id(), member, &ctx)?;
        let base_branch = resolve_prepare_base_branch(&ctx, &repo, params.base_branch.as_deref())?;

        let remote_res = resolve_remote_spec(&self.office, &repo);
        let runner = self.runner()?;
        let bare_path = bare_cache_path(
            self.office.runtime_root(),
            &self.office.info().office_id,
            &repo.id,
        );
        let prep = PrepareLockedArgs {
            runner: &runner,
            member,
            command_id: &params.command_id,
            ctx: &ctx,
            repo: &repo,
            base_branch: &base_branch,
            bare_path: &bare_path,
            remote: remote_res.as_ref(),
        };
        let result = self
            .locks
            .with_lock(&bare_path, || prepare_worktree_locked(self, &prep))
            .map_err(map_git_error)??;

        self.live.record_mailbox_change();
        Ok(result)
    }

    fn handle_git_push(
        &self,
        member: &str,
        params: GitPushParams,
    ) -> Result<GitPushResult, ApiError> {
        let ctx = self
            .office
            .task_git_context(member, &params.task)
            .map_err(map_office_error)??;
        if matches!(ctx.task.status, TaskStatus::Done | TaskStatus::Cancelled) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "cannot push a branch for a closed or cancelled task",
            ));
        }
        let (ws_key, ws) = select_push_workspace(
            self.office.owner_member_id(),
            &self.state,
            member,
            &ctx,
            params.repository.as_deref(),
        )?;
        verify_durable_push_branch(&ctx.task, &ws)?;
        if ws.admin_dir_name.is_empty() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "workspace is missing recorded Git administrative directory name",
            ));
        }

        let repo = ctx
            .repositories
            .iter()
            .find(|r| r.id.as_str() == ws.repository_id)
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;
        let remote = resolve_remote_spec(&self.office, &repo)?;
        let runner = self.runner()?;
        let bare_path = bare_cache_path(
            self.office.runtime_root(),
            &self.office.info().office_id,
            &repo.id,
        );
        let forbidden: BTreeSet<String> = ctx.forbidden_branches.into_iter().collect();
        let wt_path = PathBuf::from(&ws.worktree_path);
        let pushed = self
            .locks
            .with_lock(&bare_path, || -> Result<org_git::PushResult, GitError> {
                sanitize_worktree_links(&bare_path, &ws.admin_dir_name, &wt_path)?;
                push_task_branch(
                    &runner,
                    &bare_path,
                    &remote,
                    &ws.branch,
                    &ws.base_commit,
                    &forbidden,
                    false,
                )
            })
            .map_err(map_git_error)?;

        {
            let mut state_guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = state_guard.workspaces.get_mut(&ws_key) {
                entry.pushed_head = Some(pushed.commit.clone());
            }
            self.save_state_locked(&state_guard)?;
        }
        self.live.record_mailbox_change();

        Ok(GitPushResult {
            task_id: ws.task_id,
            repository_id: ws.repository_id,
            branch: pushed.branch,
            commit: pushed.commit,
        })
    }
}

fn validate_register_origin(params: &RepoRegisterParams) -> Result<Option<String>, ApiError> {
    match params.kind {
        RepositoryKindWire::Remote => {
            if params.origin_path.is_some() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "remote repository cannot specify origin_path",
                ));
            }
            let url = params.remote_url.as_deref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "remote repository requires remote_url",
                )
            })?;
            validate_remote_url(url).map_err(map_git_error)?;
            Ok(None)
        }
        RepositoryKindWire::Local => {
            if params.remote_url.is_some() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "local repository cannot specify remote_url",
                ));
            }
            let origin = params.origin_path.as_deref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "local repository requires origin_path",
                )
            })?;
            let canon = validate_local_origin_path(Path::new(origin)).map_err(map_git_error)?;
            Ok(Some(canon.to_string_lossy().into_owned()))
        }
    }
}

fn record_or_verify_local_origin(
    office: &Office,
    repo: &Repository,
    origin_path: &str,
) -> Result<(), ApiError> {
    if let Some(existing) = office
        .read_local_repo_origin(&repo.id)
        .map_err(map_office_error)?
    {
        if existing != origin_path {
            return Err(ApiError::new(
                ErrorCode::OperationReused,
                "command_id reused with different local origin_path",
            ));
        }
    } else {
        office
            .write_local_repo_origin(&repo.id, origin_path)
            .map_err(map_office_error)?;
    }
    Ok(())
}

fn resolve_remote_spec(office: &Office, repo: &Repository) -> Result<RemoteSpec, ApiError> {
    match repo.kind {
        RepositoryKind::Remote => {
            let url = repo.remote_url.as_deref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "remote repository has no remote_url",
                )
            })?;
            let protocol = validate_remote_url(url).map_err(map_git_error)?;
            Ok(RemoteSpec {
                url: url.to_owned(),
                protocol,
                credential: GitCredential::None,
            })
        }
        RepositoryKind::Local => {
            let origin = office
                .read_local_repo_origin(&repo.id)
                .map_err(map_office_error)?
                .ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::NotFound,
                        "local repository origin_path is not configured in this office",
                    )
                })?;
            let canon = validate_local_origin_path(Path::new(&origin)).map_err(map_git_error)?;
            Ok(RemoteSpec {
                url: canon.to_string_lossy().into_owned(),
                protocol: AllowedProtocol::File,
                credential: GitCredential::None,
            })
        }
    }
}

fn authorize_worktree_prepare(
    owner_member_id: &str,
    member: &str,
    ctx: &org_office::TaskGitContext,
) -> Result<(), ApiError> {
    let is_owner = member == owner_member_id;
    match &ctx.task.assignee_id {
        Some(assignee) if assignee.as_str() != member && !is_owner => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the assigned employee or organization owner may prepare this task's worktree",
        )),
        None if !is_owner && !ctx.caller_in_task_team => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "unassigned task worktrees may only be prepared by the owner or a member of the task's team",
        )),
        _ => Ok(()),
    }
}

fn resolve_prepare_base_branch(
    ctx: &org_office::TaskGitContext,
    repo: &Repository,
    requested_base: Option<&str>,
) -> Result<String, ApiError> {
    match &ctx.project {
        Some(project) => {
            let link = project
                .repositories
                .iter()
                .find(|l| l.repository_id == repo.id)
                .ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::InvalidRequest,
                        "repository is not attached to the task's project",
                    )
                })?;
            if requested_base.is_some_and(|r| r.trim() != link.integration_branch) {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "project tasks must branch from the project's configured integration_branch",
                ));
            }
            Ok(link.integration_branch.clone())
        }
        None => {
            let raw = requested_base.unwrap_or("").trim();
            if raw.is_empty() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "projectless tasks require an explicit base_branch",
                ));
            }
            Ok(raw.to_owned())
        }
    }
}

struct PrepareLockedArgs<'a> {
    runner: &'a GitRunner,
    member: &'a str,
    command_id: &'a str,
    ctx: &'a org_office::TaskGitContext,
    repo: &'a Repository,
    base_branch: &'a str,
    bare_path: &'a Path,
    remote: Result<&'a RemoteSpec, &'a ApiError>,
}

fn prepare_worktree_locked(
    svc: &GitService,
    args: &PrepareLockedArgs<'_>,
) -> Result<Result<WorktreePrepareResult, ApiError>, GitError> {
    let PrepareLockedArgs {
        runner,
        member,
        command_id,
        ctx,
        repo,
        base_branch,
        bare_path,
        remote,
    } = *args;
    if let Err(api_err) = ensure_bare_cache_ready(svc, runner, repo, bare_path, remote) {
        return Ok(Err(api_err));
    }
    let office_id = svc.office.info().office_id.clone();
    let ws_key = OfficeGitState::workspace_key(
        ctx.task.id.as_str(),
        repo.id.as_str(),
        ctx.workspace_owner_id.as_str(),
    );
    let taken_branches =
        match try_reuse_workspace(&svc.state, bare_path, &ws_key, repo.id.as_str())? {
            Ok(reused) => return Ok(Ok(reused)),
            Err(taken) => taken,
        };
    let base_commit = resolve_remote_branch_commit(runner, bare_path, base_branch)?;
    let branch = choose_unique_task_branch(
        runner,
        bare_path,
        &ctx.task.key(),
        &ctx.task.title,
        &taken_branches,
    )?;
    let wt_path = worktree_path(
        svc.office.runtime_root(),
        &office_id,
        &ctx.workspace_owner_team_chain,
        &ctx.workspace_owner_id,
        ctx.task.project_id.as_ref(),
        &ctx.task.id,
        &repo.id,
    );
    let created = create_worktree(runner, bare_path, &wt_path, &branch, &base_commit, false)?;
    if let Err(err) = svc
        .office
        .record_task_repository_branch(
            member,
            command_id,
            &ctx.task.id,
            &repo.id,
            branch.clone(),
            None,
        )
        .map_err(map_office_error)
        .and_then(|inner| inner)
    {
        rollback_created_worktree(runner, bare_path, &created.worktree_path, &branch);
        return Ok(Err(err));
    }
    let ws_record = TaskWorkspaceRecord {
        office_id,
        employee_id: ctx.workspace_owner_id.as_str().to_owned(),
        task_id: ctx.task.id.as_str().to_owned(),
        task_key: ctx.task.key(),
        repository_id: repo.id.as_str().to_owned(),
        repository_key: repo.key.clone(),
        branch,
        worktree_path: created.worktree_path.to_string_lossy().into_owned(),
        admin_dir_name: created.admin_dir_name.clone(),
        base_commit: created.base_commit.clone(),
        pushed_head: None,
        state: "active".into(),
    };
    if let Err(err) = persist_prepared_workspace(
        svc,
        runner,
        bare_path,
        ws_key,
        ws_record.clone(),
        &created.worktree_path,
    ) {
        return Ok(Err(err));
    }
    Ok(Ok(build_prepare_result(
        &ws_record,
        bare_path,
        &created.admin_dir_name,
        &created.worktree_path,
    )))
}

fn try_reuse_workspace(
    state: &Mutex<OfficeGitState>,
    bare_path: &Path,
    ws_key: &str,
    repo_id: &str,
) -> Result<Result<WorktreePrepareResult, BTreeSet<String>>, GitError> {
    let state_guard = state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = state_guard.workspaces.get(ws_key) {
        let existing_path = PathBuf::from(&existing.worktree_path);
        if existing.state == "active" && existing_path.is_dir() {
            if existing.admin_dir_name.is_empty() {
                return Err(GitError::SecurityViolation(
                    "workspace is missing recorded admin_dir_name".into(),
                ));
            }
            sanitize_worktree_links(bare_path, &existing.admin_dir_name, &existing_path)?;
            return Ok(Ok(build_prepare_result(
                existing,
                bare_path,
                &existing.admin_dir_name,
                &existing_path,
            )));
        }
    }
    let taken = state_guard
        .workspaces
        .values()
        .filter(|w| w.repository_id == repo_id)
        .map(|w| w.branch.clone())
        .collect();
    Ok(Err(taken))
}

fn persist_prepared_workspace(
    svc: &GitService,
    runner: &GitRunner,
    bare_path: &Path,
    ws_key: String,
    ws_record: TaskWorkspaceRecord,
    created_path: &Path,
) -> Result<(), ApiError> {
    let mut state_guard = svc.state.lock().unwrap_or_else(|e| e.into_inner());
    let branch = ws_record.branch.clone();
    let prev = state_guard.workspaces.insert(ws_key.clone(), ws_record);
    if let Err(err) = svc.save_state_locked(&state_guard) {
        if let Some(old) = prev {
            state_guard.workspaces.insert(ws_key, old);
        } else {
            state_guard.workspaces.remove(&ws_key);
        }
        rollback_created_worktree(runner, bare_path, created_path, &branch);
        return Err(err);
    }
    Ok(())
}

fn ensure_bare_cache_ready(
    svc: &GitService,
    runner: &GitRunner,
    repo: &Repository,
    bare_path: &Path,
    remote: Result<&RemoteSpec, &ApiError>,
) -> Result<(), ApiError> {
    let already_exists = bare_path.join("HEAD").is_file();
    let remote = match remote {
        Ok(r) => r,
        Err(_) if already_exists => {
            verify_cache_integrity(bare_path, false).map_err(map_git_error)?;
            return Ok(());
        }
        Err(err) => return Err(err.clone()),
    };
    if !already_exists {
        let mut state_guard = svc.state.lock().unwrap_or_else(|e| e.into_inner());
        state_guard.caches.insert(
            repo.id.as_str().to_owned(),
            RepoCacheState {
                repository_id: repo.id.as_str().to_owned(),
                state: CacheStateWire::Cloning,
                bare_path: Some(bare_path.to_string_lossy().into_owned()),
                reason: None,
            },
        );
        let _ = svc.save_state_locked(&state_guard);
    }
    let op_res = fetch_cache(runner, bare_path, remote, false);
    let mut state_guard = svc.state.lock().unwrap_or_else(|e| e.into_inner());
    match op_res {
        Ok(()) => {
            state_guard.caches.insert(
                repo.id.as_str().to_owned(),
                RepoCacheState {
                    repository_id: repo.id.as_str().to_owned(),
                    state: CacheStateWire::Ready,
                    bare_path: Some(bare_path.to_string_lossy().into_owned()),
                    reason: None,
                },
            );
            svc.save_state_locked(&state_guard)?;
            Ok(())
        }
        Err(GitError::RemoteUnavailable(detail)) if already_exists => {
            eprintln!(
                "agentic-org-office: offline fetch fallback for cache {}: {detail}",
                repo.id
            );
            Ok(())
        }
        Err(err) => {
            let sanitized = sanitized_git_reason(&err);
            eprintln!(
                "agentic-org-office: cache fetch failed for {}: {err}",
                repo.id
            );
            state_guard.caches.insert(
                repo.id.as_str().to_owned(),
                RepoCacheState {
                    repository_id: repo.id.as_str().to_owned(),
                    state: CacheStateWire::Failed,
                    bare_path: Some(bare_path.to_string_lossy().into_owned()),
                    reason: Some(sanitized),
                },
            );
            let _ = svc.save_state_locked(&state_guard);
            Err(map_git_error(err))
        }
    }
}

fn select_push_workspace(
    owner_member_id: &str,
    state: &Mutex<OfficeGitState>,
    member: &str,
    ctx: &org_office::TaskGitContext,
    repository: Option<&str>,
) -> Result<(String, TaskWorkspaceRecord), ApiError> {
    let explicit_repo_id = match repository {
        Some(r) => {
            let found = org_office::projects::features::resolve_repository(&ctx.repositories, r)
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;
            Some(found.id.as_str().to_owned())
        }
        None => None,
    };
    let is_owner = member == owner_member_id;
    let state_guard = state.lock().unwrap_or_else(|e| e.into_inner());
    let matching: Vec<(&String, &TaskWorkspaceRecord)> = state_guard
        .workspaces
        .iter()
        .filter(|(_, w)| {
            w.state == "active"
                && w.task_id == ctx.task.id.as_str()
                && (is_owner || w.employee_id == member)
                && explicit_repo_id
                    .as_deref()
                    .is_none_or(|rid| w.repository_id == rid)
        })
        .collect();
    match matching.as_slice() {
        [] => Err(ApiError::new(
            ErrorCode::NotFound,
            "no active task worktree exists for this caller/task/repository in this office",
        )),
        [(k, w)] => Ok(((*k).clone(), (*w).clone())),
        _ => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "task has multiple repositories; specify --repo",
        )),
    }
}

fn build_prepare_result(
    ws: &TaskWorkspaceRecord,
    bare_path: &Path,
    admin_dir_name: &str,
    worktree_dir: &Path,
) -> WorktreePrepareResult {
    let rules = derive_sandbox_rules(bare_path, admin_dir_name, worktree_dir);
    WorktreePrepareResult {
        worktree: ws.to_wire(),
        writable_roots: rules
            .writable_roots
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        fixed_write_denies: rules
            .fixed_write_denies
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
    }
}

fn verify_durable_push_branch(
    task: &org_office::tasks::records::TaskRecord,
    ws: &TaskWorkspaceRecord,
) -> Result<(), ApiError> {
    let durable_link = task
        .repositories
        .iter()
        .find(|r| r.repository_id.as_str() == ws.repository_id)
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::Conflict,
                "workspace repository is not recorded on the durable task record",
            )
        })?;
    if durable_link.branch.as_deref() != Some(ws.branch.as_str()) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "workspace branch does not match the recorded durable task branch",
        ));
    }
    if durable_link.integrated_commit.is_some() {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "task branch is already recorded as integrated",
        ));
    }
    Ok(())
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

fn map_office_error(err: org_office::OfficeError) -> ApiError {
    eprintln!("agentic-org-office: office error in git service: {err}");
    ApiError::new(ErrorCode::StorageError, err.to_string())
}

fn sanitized_git_reason(err: &GitError) -> String {
    match err {
        GitError::InvalidInput(_) => "invalid Git input".into(),
        GitError::SecurityViolation(_) => "Git repository security check failed".into(),
        GitError::Conflict(_) => "Git conflict (non-fast-forward or workspace conflict)".into(),
        GitError::UnsupportedGit(_) => "host Git binary unavailable or unsupported version".into(),
        GitError::RemoteUnavailable(_) => "Git remote is unreachable or refused connection".into(),
        GitError::CommandFailed { .. } => "Git command failed".into(),
        GitError::Io { .. } => "office Git storage error".into(),
    }
}

fn map_git_error(err: GitError) -> ApiError {
    eprintln!("agentic-org-office: git error: {err}");
    match &err {
        GitError::InvalidInput(msg) => ApiError::new(ErrorCode::InvalidRequest, msg.clone()),
        GitError::SecurityViolation(_) => ApiError::new(
            ErrorCode::InvalidRequest,
            "Git repository integrity or security check failed",
        ),
        GitError::Conflict(_) => ApiError::new(
            ErrorCode::Conflict,
            "Git push or workspace operation conflicted with repository state",
        ),
        GitError::UnsupportedGit(_) => ApiError::new(
            ErrorCode::Unavailable,
            "host Git binary unavailable or older than required version",
        ),
        GitError::RemoteUnavailable(_) => ApiError::new(
            ErrorCode::Unavailable,
            "Git remote is unreachable or missing requested branch",
        ),
        GitError::CommandFailed { .. } => {
            ApiError::new(ErrorCode::Unavailable, "Git operation failed")
        }
        GitError::Io { .. } => ApiError::new(ErrorCode::StorageError, "office Git storage error"),
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
mod tests {
    use super::*;
    use org_office::{EngineChoice, Office, TestOffice};
    use org_proto::org::{MemberAddPersonParams, ProjectCreateParams, TeamCreateParams};
    use org_proto::tasks::{TaskStatus, TasksCreateParams, TasksTransitionParams};
    use std::process::Command;

    fn run_host_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn seed_bare_origin(dir: &Path) -> PathBuf {
        let bare = dir.join("origin.git");
        std::fs::create_dir_all(&bare).unwrap();
        run_host_git(&bare, &["init", "--bare", "--initial-branch=main"]);
        let seed = dir.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        run_host_git(&seed, &["init", "--initial-branch=main"]);
        run_host_git(&seed, &["config", "user.name", "Seed"]);
        run_host_git(&seed, &["config", "user.email", "seed@example.test"]);
        std::fs::write(seed.join("README.md"), "# Seed\n").unwrap();
        run_host_git(&seed, &["add", "README.md"]);
        run_host_git(&seed, &["commit", "-m", "initial"]);
        run_host_git(&seed, &["remote", "add", "origin", bare.to_str().unwrap()]);
        run_host_git(&seed, &["push", "origin", "main"]);
        bare
    }

    #[test]
    fn real_office_register_attach_prepare_commit_and_push_end_to_end() {
        let office = Arc::new(Office::for_test(TestOffice {
            engine: EngineChoice::Sqlite,
        }));
        let owner = office.owner_member_id().to_owned();
        let live = Arc::new(LiveEvents::for_owner(Box::new(|| {}), owner.clone()));
        let svc = GitService::new(Arc::clone(&office), live);

        let origin_tmp = tempfile::tempdir().unwrap();
        let bare_origin = seed_bare_origin(origin_tmp.path());

        let uuid = |n: u64| format!("018bce83-1200-7000-8000-{n:012x}");
        let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
        let project_id = uuid(201);
        let team_id = uuid(202);
        let child_member_id = uuid(205);
        let outsider_team_id = uuid(206);
        let outsider_member_id = uuid(207);

        let _ = office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: uuid(200),
                    team_id: team_id.clone(),
                    name: "Platform".into(),
                    owner_team_id: Some(root_team.clone()),
                }),
            )
            .unwrap()
            .unwrap();
        let _ = office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: uuid(208),
                    team_id: outsider_team_id.clone(),
                    name: "Design".into(),
                    owner_team_id: Some(root_team.clone()),
                }),
            )
            .unwrap()
            .unwrap();
        let _ = office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                    command_id: uuid(209),
                    member_id: child_member_id.clone(),
                    name: "Dev One".into(),
                    handle: Some("dev1".into()),
                    home_team_id: Some(team_id.clone()),
                    contact: None,
                }),
            )
            .unwrap()
            .unwrap();
        let _ = office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                    command_id: uuid(210),
                    member_id: outsider_member_id.clone(),
                    name: "Outsider".into(),
                    handle: Some("out1".into()),
                    home_team_id: Some(outsider_team_id.clone()),
                    contact: None,
                }),
            )
            .unwrap()
            .unwrap();

        let proj_params: ProjectCreateParams = serde_json::from_value(serde_json::json!({
            "command_id": uuid(203),
            "project_id": project_id,
            "name": "Core",
            "slug": "core",
            "owner_team_id": team_id,
            "contributing_team_ids": [],
        }))
        .unwrap();
        let _ = office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::ProjectCreate(proj_params),
            )
            .unwrap()
            .unwrap();

        // Non-owner cannot register a repository (even probing nonexistent local path gets NotAuthorized).
        let non_owner_reg = svc.handle_register(
            &child_member_id,
            RepoRegisterParams {
                command_id: uuid(299),
                repository_id: uuid(298),
                key: Some("probe".into()),
                kind: RepositoryKindWire::Local,
                remote_url: None,
                origin_path: Some("/nonexistent/secret/path".into()),
            },
        );
        assert_eq!(non_owner_reg.unwrap_err().code, ErrorCode::NotAuthorized);

        let repo_id = uuid(301);
        let reg = svc
            .handle_register(
                &owner,
                RepoRegisterParams {
                    command_id: uuid(302),
                    repository_id: repo_id.clone(),
                    key: Some("core-repo".into()),
                    kind: RepositoryKindWire::Local,
                    remote_url: None,
                    origin_path: Some(bare_origin.to_string_lossy().into_owned()),
                },
            )
            .expect("register local repo");
        assert_eq!(reg.repository.key.as_deref(), Some("core-repo"));

        // Verify local origin_path NEVER enters shared durable `state/repositories.yaml`.
        let repos_yaml =
            std::fs::read_to_string(office.durable_root().join("state/repositories.yaml")).unwrap();
        assert!(!repos_yaml.contains(&bare_origin.to_string_lossy().to_string()));

        // Replaying `repo.register` with same command_id but different `origin_path` fails with OperationReused.
        let other_dir = origin_tmp.path().join("other");
        std::fs::create_dir_all(&other_dir).unwrap();
        let reused = svc.handle_register(
            &owner,
            RepoRegisterParams {
                command_id: uuid(302),
                repository_id: repo_id.clone(),
                key: Some("core-repo".into()),
                kind: RepositoryKindWire::Local,
                remote_url: None,
                origin_path: Some(other_dir.to_string_lossy().into_owned()),
            },
        );
        assert_eq!(reused.unwrap_err().code, ErrorCode::OperationReused);

        // Attach with non-existent branch must be refused before commit!
        let bad_attach = svc.handle_attach(
            &owner,
            RepoAttachParams {
                command_id: uuid(303),
                project: "core".into(),
                repository: "core-repo".into(),
                integration_branch: "does-not-exist".into(),
            },
        );
        assert_eq!(bad_attach.unwrap_err().code, ErrorCode::NotFound);

        // Attach with verified `main` branch succeeds.
        let attached = svc
            .handle_attach(
                &owner,
                RepoAttachParams {
                    command_id: uuid(304),
                    project: "core".into(),
                    repository: "core-repo".into(),
                    integration_branch: "main".into(),
                },
            )
            .expect("attach repo");
        assert_eq!(attached.link.integration_branch, "main");

        // Owner creates task assigned to `child_member_id` (in child team `team_id`) and prepares
        // worktree: path MUST use assignee's nested team chain (`root_team` -> `team_id`)!
        let task_out = office
            .task_create(
                &owner,
                TasksCreateParams {
                    command_id: uuid(401),
                    task_id: uuid(411),
                    team_id: Some(team_id.clone()),
                    assigned_team_id: None,
                    project_id: Some(project_id.clone()),
                    task_type: Some("task".into()),
                    work_type: None,
                    title: "Implement C7 Git Worktrees".into(),
                    body_markdown: String::new(),
                    assignee_id: Some(child_member_id.clone()),
                    priority: None,
                    effort: None,
                    severity: None,
                    rank_order: None,
                    labels: vec![],
                    reference_files: vec![],
                    modified_files: vec![],
                    depends_on: vec![],
                    details: None,
                },
            )
            .unwrap()
            .unwrap();

        let prepared = svc
            .handle_worktree_prepare(
                &owner,
                WorktreePrepareParams {
                    command_id: uuid(402),
                    task: task_out.task.key.clone(),
                    repository: "core-repo".into(),
                    base_branch: None,
                },
            )
            .expect("prepare worktree");
        let expected_chain_segment = format!(
            "teams/{root_team}/teams/{team_id}/agents/{child_member_id}/projects/{project_id}"
        );
        assert!(
            prepared
                .worktree
                .worktree_path
                .contains(&expected_chain_segment),
            "worktree path {} must use assignee team chain {expected_chain_segment}",
            prepared.worktree.worktree_path
        );
        assert!(prepared
            .worktree
            .branch
            .starts_with(&format!("task/{}-", task_out.task.key)));
        assert_eq!(prepared.writable_roots.len(), 5);

        // Non-owner outsider cannot see `child_member_id`'s worktree or bare_path.
        let outsider_list = svc
            .handle_worktree_list(&outsider_member_id, WorktreeListParams::default())
            .unwrap();
        assert!(outsider_list.worktrees.is_empty());
        let non_owner_status = svc
            .handle_status(
                &outsider_member_id,
                RepoStatusParams {
                    repository: "core-repo".into(),
                },
            )
            .unwrap();
        assert!(non_owner_status.bare_path.is_none());

        // Outsider cannot prepare an unassigned task owned by `team_id`.
        let unassigned = office
            .task_create(
                &owner,
                TasksCreateParams {
                    command_id: uuid(420),
                    task_id: uuid(421),
                    team_id: Some(team_id.clone()),
                    assigned_team_id: None,
                    project_id: Some(project_id.clone()),
                    task_type: Some("task".into()),
                    work_type: None,
                    title: "Unassigned task".into(),
                    body_markdown: String::new(),
                    assignee_id: None,
                    priority: None,
                    effort: None,
                    severity: None,
                    rank_order: None,
                    labels: vec![],
                    reference_files: vec![],
                    modified_files: vec![],
                    depends_on: vec![],
                    details: None,
                },
            )
            .unwrap()
            .unwrap();
        let outsider_prep = svc.handle_worktree_prepare(
            &outsider_member_id,
            WorktreePrepareParams {
                command_id: uuid(422),
                task: unassigned.task.key.clone(),
                repository: "core-repo".into(),
                base_branch: None,
            },
        );
        assert_eq!(outsider_prep.unwrap_err().code, ErrorCode::NotAuthorized);

        // Commit inside worktree and push as assignee (`child_member_id`).
        let wt = PathBuf::from(&prepared.worktree.worktree_path);
        std::fs::write(wt.join("feature.txt"), "hello c7\n").unwrap();
        run_host_git(&wt, &["add", "feature.txt"]);
        run_host_git(
            &wt,
            &[
                "-c",
                "user.name=Dev One",
                "-c",
                "user.email=dev1@example.test",
                "commit",
                "-m",
                "add feature",
            ],
        );

        let pushed = svc
            .handle_git_push(
                &child_member_id,
                GitPushParams {
                    task: task_out.task.key.clone(),
                    repository: None,
                },
            )
            .expect("push task branch");
        assert_eq!(pushed.branch, prepared.worktree.branch);
        assert_eq!(pushed.commit.len(), 40);

        // Closing the task blocks subsequent push!
        let current_rev = office
            .task_inspect(&task_out.task.key)
            .unwrap()
            .unwrap()
            .task
            .revision;
        let _ = office
            .task_transition(
                &owner,
                TasksTransitionParams {
                    command_id: uuid(430),
                    task: task_out.task.key.clone(),
                    status: TaskStatus::Done,
                    resolution: Some("fixed".into()),
                    blocked_reason: None,
                    note: None,
                    expected_revision: current_rev,
                },
            )
            .unwrap()
            .unwrap();
        let closed_push = svc.handle_git_push(
            &child_member_id,
            GitPushParams {
                task: task_out.task.key.clone(),
                repository: None,
            },
        );
        assert_eq!(closed_push.unwrap_err().code, ErrorCode::InvalidRequest);

        // Verify origin bare repository received `refs/heads/<branch>`.
        let verify_out = Command::new("git")
            .current_dir(&bare_origin)
            .args(["rev-parse", &format!("refs/heads/{}", pushed.branch)])
            .output()
            .unwrap();
        assert!(verify_out.status.success());
        assert_eq!(
            String::from_utf8_lossy(&verify_out.stdout).trim(),
            pushed.commit
        );

        let owner_status = svc
            .handle_status(
                &owner,
                RepoStatusParams {
                    repository: "core-repo".into(),
                },
            )
            .unwrap();
        assert_eq!(owner_status.state, CacheStateWire::Ready);
        assert!(owner_status.bare_path.is_some());

        // Offline fallback: delete origin repo and verify `worktree.prepare` for a new task still
        // succeeds from the cached bare repository without clobbering `Ready` state!
        std::fs::remove_dir_all(&bare_origin).unwrap();
        let offline_task = office
            .task_create(
                &owner,
                TasksCreateParams {
                    command_id: uuid(440),
                    task_id: uuid(441),
                    team_id: Some(team_id),
                    assigned_team_id: None,
                    project_id: Some(project_id),
                    task_type: Some("task".into()),
                    work_type: None,
                    title: "Offline branch".into(),
                    body_markdown: String::new(),
                    assignee_id: Some(child_member_id),
                    priority: None,
                    effort: None,
                    severity: None,
                    rank_order: None,
                    labels: vec![],
                    reference_files: vec![],
                    modified_files: vec![],
                    depends_on: vec![],
                    details: None,
                },
            )
            .unwrap()
            .unwrap();
        let offline_wt = svc
            .handle_worktree_prepare(
                &owner,
                WorktreePrepareParams {
                    command_id: uuid(442),
                    task: offline_task.task.key,
                    repository: "core-repo".into(),
                    base_branch: None,
                },
            )
            .expect("offline worktree prepare should fall back to cached branch");
        assert!(offline_wt.worktree.branch.starts_with("task/"));
        assert_eq!(
            svc.handle_status(
                &owner,
                RepoStatusParams {
                    repository: "core-repo".into(),
                },
            )
            .unwrap()
            .state,
            CacheStateWire::Ready
        );
    }
}
