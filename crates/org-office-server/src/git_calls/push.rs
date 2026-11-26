//! `git.push`: the caller-owned task workspace, the durable task branch, and the non-force push.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

use org_git::{push_task_branch, sanitize_worktree_links, GitError};
use org_office::git::{bare_cache_path, OfficeGitState, TaskWorkspaceRecord};
use org_proto::git::{GitPushParams, GitPushResult};
use org_proto::tasks::TaskStatus;
use org_proto::{ApiError, ErrorCode};

use super::credentials::resolve_remote_spec;
use super::redaction::{map_git_error, map_office_error};
use super::GitService;

impl GitService {
    pub(super) fn handle_git_push(
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
