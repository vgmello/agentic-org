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
        let local_task_write = self
            .office
            .has_local_task_write_authority_for_code(&ctx.task.office_code)
            .map_err(map_office_error)?;
        verify_durable_push_branch(&ctx.task, &ws, local_task_write)?;
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
    local_task_write: bool,
) -> Result<(), ApiError> {
    if let Some(durable_link) = task
        .repositories
        .iter()
        .find(|r| r.repository_id.as_str() == ws.repository_id)
    {
        if durable_link.integrated_commit.is_some() {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "task branch is already recorded as integrated",
            ));
        }
        if durable_link.branch.as_deref() != Some(ws.branch.as_str()) {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "workspace branch does not match the recorded durable task branch",
            ));
        }
        return Ok(());
    }
    if local_task_write {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "workspace repository is not recorded on the durable task record",
        ));
    }
    // Satellite offices never mutate HQ-owned `task.json` locally (`TASK-192`), so no durable
    // `task_repositories` link exists yet; enforce the canonical `task/<KEY>-` branch prefix
    // against the office-private `TaskWorkspaceRecord`.
    let expected_prefix = format!("task/{}-", task.key());
    if !ws.branch.starts_with(&expected_prefix) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "satellite workspace branch does not match the task branch prefix",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_office::tasks::records::{TaskMeta, TaskRecord, TaskRepositoryRecord};
    use org_proto::tasks::{TaskPriority, TaskStatus};
    use org_store::Id;

    fn sample_task(repos: Vec<TaskRepositoryRecord>) -> TaskRecord {
        TaskRecord {
            id: Id::parse("018bce83-1200-7000-8000-000000000001").unwrap(),
            office_code: "A".into(),
            number: 7,
            team_id: Id::parse("018bce83-1200-7000-8000-000000000002").unwrap(),
            assigned_team_id: None,
            project_id: None,
            task_type: "task".into(),
            work_type: None,
            title: "Test".into(),
            status: TaskStatus::Todo,
            assignee_id: None,
            body_markdown: String::new(),
            feature_id: None,
            milestone_id: None,
            priority: TaskPriority::Normal,
            effort: None,
            severity: None,
            resolution: None,
            blocked_reason: None,
            rank_order: None,
            labels: Vec::new(),
            reference_files: Vec::new(),
            modified_files: Vec::new(),
            repositories: repos,
            details: None,
            meta: TaskMeta::created(
                &Id::parse("018bce83-1200-7000-8000-000000000003").unwrap(),
                "2026-11-26T00:00:00.000Z",
            ),
        }
    }

    fn sample_ws(branch: &str) -> TaskWorkspaceRecord {
        TaskWorkspaceRecord {
            office_id: "018bce83-1200-7000-8000-000000000099".into(),
            employee_id: "018bce83-1200-7000-8000-000000000003".into(),
            task_id: "018bce83-1200-7000-8000-000000000001".into(),
            task_key: "T-A7".into(),
            repository_id: "018bce83-1200-7000-8000-000000000010".into(),
            repository_key: Some("organization".into()),
            branch: branch.into(),
            worktree_path: "/tmp/wt".into(),
            admin_dir_name: "wt".into(),
            base_commit: "0000000000000000000000000000000000000000".into(),
            pushed_head: None,
            state: "active".into(),
        }
    }

    #[test]
    fn verify_durable_push_branch_enforces_local_and_satellite_invariants() {
        let repo_id = Id::parse("018bce83-1200-7000-8000-000000000010").unwrap();
        let ws = sample_ws("task/T-A7-fix-sync");

        // Local office without durable link fails closed.
        let no_link = sample_task(Vec::new());
        assert_eq!(
            verify_durable_push_branch(&no_link, &ws, true)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );

        // Satellite on remote-owned task allows valid `task/<KEY>-` workspace branch.
        assert!(verify_durable_push_branch(&no_link, &ws, false).is_ok());

        // Satellite rejects any tampered or wrong-task branch prefix.
        for bad in ["main", "v1", "task/T-A7", "task/T-A8-other", "feature/T-A7"] {
            assert_eq!(
                verify_durable_push_branch(&no_link, &sample_ws(bad), false)
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict,
                "expected conflict for `{bad}`"
            );
        }

        // Integrated or mismatched durable links are always rejected even on satellites.
        let integrated = sample_task(vec![TaskRepositoryRecord {
            repository_id: repo_id.clone(),
            branch: Some("task/T-A7-fix-sync".into()),
            integrated_commit: Some("deadbeef".into()),
        }]);
        assert_eq!(
            verify_durable_push_branch(&integrated, &ws, false)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );

        let mismatched = sample_task(vec![TaskRepositoryRecord {
            repository_id: repo_id,
            branch: Some("task/T-A7-different".into()),
            integrated_commit: None,
        }]);
        assert_eq!(
            verify_durable_push_branch(&mismatched, &ws, false)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
}
