//! `worktree.prepare`: caller authorization, base-branch resolution, and preparing, reusing or
//! persisting an isolated task worktree under the repository lock.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use org_git::{
    choose_unique_task_branch, create_worktree, derive_sandbox_rules, resolve_remote_branch_commit,
    rollback_created_worktree, sanitize_worktree_links, GitError, GitRunner, RemoteSpec,
};
use org_office::git::{bare_cache_path, worktree_path, OfficeGitState, TaskWorkspaceRecord};
use org_office::organization_family::records::Repository;
use org_proto::git::{WorktreePrepareParams, WorktreePrepareResult};
use org_proto::tasks::TaskStatus;
use org_proto::{ApiError, ErrorCode};

use super::cache::ensure_bare_cache_ready;
use super::credentials::resolve_remote_spec;
use super::redaction::{map_git_error, map_office_error};
use super::GitService;

impl GitService {
    pub(super) fn handle_worktree_prepare(
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
            skip_fetch_if_active: false,
        };
        let mut outcome = self
            .locks
            .with_lock(&bare_path, || prepare_worktree_locked(self, &prep))
            .map_err(map_git_error)??;

        self.live.record_mailbox_change();
        self.notify_session_worktree_prepared(member, &mut outcome);
        Ok(outcome.result)
    }

    fn notify_session_worktree_prepared(
        &self,
        member: &str,
        outcome: &mut PreparedWorktreeOutcome,
    ) {
        let Some(sessions) = self.sessions() else {
            return;
        };
        let wt = &outcome.result.worktree;
        let notice = org_office_sessions::PreparedWorktreeNotice {
            employee_id: &wt.employee_id,
            triggered_by_self: member == wt.employee_id,
            recycle_idle_target: true,
            task_id: &wt.task_id,
            task_key: &wt.task_key,
            repository_id: &wt.repository_id,
            admin_dir_name: &outcome.admin_dir_name,
            branch: &wt.branch,
            worktree_path: &wt.worktree_path,
        };
        let refresh = sessions.on_worktree_prepared(&notice);
        outcome.result.sandbox_refresh_required = refresh.sandbox_refresh_required;
        outcome.result.sandbox_refresh_hint = refresh.sandbox_refresh_hint;
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

pub(super) struct PrepareLockedArgs<'a> {
    pub(super) runner: &'a GitRunner,
    pub(super) member: &'a str,
    pub(super) command_id: &'a str,
    pub(super) ctx: &'a org_office::TaskGitContext,
    pub(super) repo: &'a Repository,
    pub(super) base_branch: &'a str,
    pub(super) bare_path: &'a Path,
    pub(super) remote: Result<&'a RemoteSpec, &'a ApiError>,
    /// When `true` (auto-pre-prepare), reuse an already-active worktree on disk immediately
    /// without running a network `git fetch` first (`TASK-135` AC #4).
    pub(super) skip_fetch_if_active: bool,
}

pub(super) struct PreparedWorktreeOutcome {
    pub(super) result: WorktreePrepareResult,
    pub(super) admin_dir_name: String,
    pub(super) created_new: bool,
}

fn check_durable_link_matches_branch(
    durable_link: Option<&org_office::tasks::records::TaskRepositoryRecord>,
    branch: &str,
) -> Result<(), ApiError> {
    if let Some(link) = durable_link {
        if link.branch.as_deref() != Some(branch) {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "workspace branch does not match the recorded durable task branch",
            ));
        }
    }
    Ok(())
}

pub(super) fn prepare_worktree_locked(
    svc: &GitService,
    args: &PrepareLockedArgs<'_>,
) -> Result<Result<PreparedWorktreeOutcome, ApiError>, GitError> {
    let PrepareLockedArgs {
        runner,
        ctx,
        repo,
        base_branch,
        bare_path,
        remote,
        skip_fetch_if_active,
        ..
    } = *args;
    let durable_link = ctx
        .task
        .repositories
        .iter()
        .find(|r| r.repository_id == repo.id);
    if durable_link.is_some_and(|l| l.integrated_commit.is_some()) {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "task branch is already recorded as integrated",
        )));
    }
    let has_local_task_write = match svc
        .office
        .has_local_task_write_authority_for_code(&ctx.task.office_code)
    {
        Ok(allowed) => allowed,
        Err(err) => return Ok(Err(map_office_error(err))),
    };
    let office_id = svc.office.info().office_id.clone();
    let ws_key = OfficeGitState::workspace_key(
        ctx.task.id.as_str(),
        repo.id.as_str(),
        ctx.workspace_owner_id.as_str(),
    );
    if skip_fetch_if_active {
        if let Ok(reused) = try_reuse_workspace(
            &svc.state,
            bare_path,
            &ws_key,
            repo.id.as_str(),
            durable_link,
        )? {
            return Ok(reused);
        }
    }
    if let Err(api_err) = ensure_bare_cache_ready(svc, runner, repo, bare_path, remote) {
        return Ok(Err(api_err));
    }
    let taken_branches = match try_reuse_workspace(
        &svc.state,
        bare_path,
        &ws_key,
        repo.id.as_str(),
        durable_link,
    )? {
        Ok(reused) => return Ok(reused),
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
    if !has_local_task_write {
        if let Err(err) = check_durable_link_matches_branch(durable_link, &branch) {
            return Ok(Err(err));
        }
    }
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
    if has_local_task_write {
        if let Err(err) = record_branch_or_rollback(svc, args, &created.worktree_path, &branch) {
            return Ok(Err(err));
        }
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
    Ok(Ok(PreparedWorktreeOutcome {
        result: build_prepare_result(
            &ws_record,
            bare_path,
            &created.admin_dir_name,
            &created.worktree_path,
        ),
        admin_dir_name: created.admin_dir_name,
        created_new: true,
    }))
}

fn record_branch_or_rollback(
    svc: &GitService,
    args: &PrepareLockedArgs<'_>,
    created_path: &Path,
    branch: &str,
) -> Result<(), ApiError> {
    if let Err(err) = svc
        .office
        .record_task_repository_branch(
            args.member,
            args.command_id,
            &args.ctx.task.id,
            &args.repo.id,
            branch.to_owned(),
            None,
        )
        .map_err(map_office_error)
        .and_then(|inner| inner)
    {
        rollback_created_worktree(args.runner, args.bare_path, created_path, branch);
        return Err(err);
    }
    Ok(())
}

fn try_reuse_workspace(
    state: &Mutex<OfficeGitState>,
    bare_path: &Path,
    ws_key: &str,
    repo_id: &str,
    durable_link: Option<&org_office::tasks::records::TaskRepositoryRecord>,
) -> Result<Result<Result<PreparedWorktreeOutcome, ApiError>, BTreeSet<String>>, GitError> {
    let state_guard = state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = state_guard.workspaces.get(ws_key) {
        let existing_path = PathBuf::from(&existing.worktree_path);
        if existing.state == "active" && existing_path.is_dir() {
            if let Err(err) = check_durable_link_matches_branch(durable_link, &existing.branch) {
                return Ok(Ok(Err(err)));
            }
            if existing.admin_dir_name.is_empty() {
                return Err(GitError::SecurityViolation(
                    "workspace is missing recorded admin_dir_name".into(),
                ));
            }
            sanitize_worktree_links(bare_path, &existing.admin_dir_name, &existing_path)?;
            return Ok(Ok(Ok(PreparedWorktreeOutcome {
                result: build_prepare_result(
                    existing,
                    bare_path,
                    &existing.admin_dir_name,
                    &existing_path,
                ),
                admin_dir_name: existing.admin_dir_name.clone(),
                created_new: false,
            })));
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
        sandbox_refresh_required: false,
        sandbox_refresh_hint: None,
    }
}
