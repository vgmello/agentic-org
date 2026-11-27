//! Automatic pre-preparation of repository-linked task worktrees before Turn 1
//! (`TASK-135` AC #4).
//!
//! ## Security & failure invariants
//! - Strictly authorized: ONLY prepares worktrees for an **active agent** placed on **this
//!   office** who is the task's **current assignee** (`task.assignee_id == employee_id`), and
//!   ONLY for repositories attached to `task.project_id` using the project's configured
//!   `integration_branch` (never projectless tasks, closed/cancelled tasks, already-integrated
//!   branches, or sibling agents).
//! - Idempotent & non-destructive: reuses existing active worktrees immediately under the
//!   repository lock without triggering redundant origin fetches (`skip_fetch_if_active: true`).
//! - Safe on error: if an origin is unreachable, credentials are missing, or a bare cache is
//!   unprepared, logs a redacted warning through [`super::redaction`] and leaves `git-state.json`
//!   untouched so session launch still succeeds fail-closed (never leaking untargeted repos).

use org_git::GitRunner;
use org_office::git::bare_cache_path;
use org_office::organization_family::records::{EmployeeKind, EmployeeStatus, Repository};
use org_proto::tasks::TaskStatus;

use super::credentials::resolve_remote_spec;
use super::redaction::{redact_git_error_for_log, scrub_git_log_text};
use super::worktree::{prepare_worktree_locked, PrepareLockedArgs, PreparedWorktreeOutcome};
use super::GitService;

/// Summary of an automatic task worktree pre-prepare attempt (`TASK-135` AC #4).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct PreprepareReport {
    pub(crate) prepared: usize,
    pub(crate) reused: usize,
    pub(crate) skipped: usize,
    pub(crate) failed: usize,
}

impl GitService {
    /// Idempotently pre-prepares all non-integrated project-linked repository worktrees for
    /// `task_ref` on behalf of its assigned active agent `employee_id`.
    pub(crate) fn preprepare_task_worktrees_for_assignee(
        &self,
        employee_id: &str,
        task_ref: &str,
    ) -> PreprepareReport {
        let mut report = PreprepareReport::default();
        if !self.is_local_active_agent(employee_id) {
            return report;
        }
        let ctx = match self.office.task_git_context(employee_id, task_ref) {
            Ok(Ok(ctx)) => ctx,
            Ok(Err(_)) => return report,
            Err(err) => {
                eprintln!(
                    "agentic-org-office: worktree pre-prepare context read failed for `{task_ref}`: {}",
                    scrub_git_log_text(&err.to_string())
                );
                report.failed += 1;
                return report;
            }
        };
        if !is_eligible_for_auto_preprepare(employee_id, &ctx) {
            return report;
        }
        if ctx
            .project
            .as_ref()
            .is_none_or(|p| p.repositories.is_empty())
        {
            return report;
        }
        let runner = match self.runner() {
            Ok(r) => r,
            Err(err) => {
                eprintln!(
                    "agentic-org-office: worktree pre-prepare skipped (git unavailable): {}",
                    scrub_git_log_text(&err.message)
                );
                report.failed += 1;
                return report;
            }
        };
        self.preprepare_project_repos(&runner, employee_id, &ctx, &mut report);
        report
    }

    fn preprepare_project_repos(
        &self,
        runner: &GitRunner,
        employee_id: &str,
        ctx: &org_office::TaskGitContext,
        report: &mut PreprepareReport,
    ) {
        let Some(project) = ctx.project.as_ref() else {
            return;
        };
        for link in &project.repositories {
            let Some(repo) = ctx.repositories.iter().find(|r| r.id == link.repository_id) else {
                report.skipped += 1;
                continue;
            };
            let already_integrated = ctx
                .task
                .repositories
                .iter()
                .any(|tr| tr.repository_id == repo.id && tr.integrated_commit.is_some());
            if already_integrated {
                report.skipped += 1;
                continue;
            }
            match self.preprepare_single_repo(
                runner,
                employee_id,
                ctx,
                repo,
                &link.integration_branch,
            ) {
                Some(outcome) if outcome.created_new => {
                    report.prepared += 1;
                    self.live.record_mailbox_change();
                    self.notify_preprepared_worktree(&outcome);
                }
                Some(_) => report.reused += 1,
                None => report.failed += 1,
            }
        }
    }

    fn is_local_active_agent(&self, employee_id: &str) -> bool {
        let Ok(Some(emp)) = self.office.load_employee(employee_id) else {
            return false;
        };
        if emp.kind != EmployeeKind::Agent || emp.status != EmployeeStatus::Active {
            return false;
        }
        if self.office.is_multi_office().unwrap_or(false)
            && org_office::resolve_employee_office(&self.office, employee_id)
                .is_ok_and(|oid| oid != self.office.info().office_id)
        {
            return false;
        }
        true
    }

    fn preprepare_single_repo(
        &self,
        runner: &GitRunner,
        employee_id: &str,
        ctx: &org_office::TaskGitContext,
        repo: &Repository,
        integration_branch: &str,
    ) -> Option<PreparedWorktreeOutcome> {
        let bare_path = bare_cache_path(
            self.office.runtime_root(),
            &self.office.info().office_id,
            &repo.id,
        );
        let remote_res = resolve_remote_spec(&self.office, repo);
        let command_id = self.office.next_uuid_v7();
        let args = PrepareLockedArgs {
            runner,
            member: employee_id,
            command_id: &command_id,
            ctx,
            repo,
            base_branch: integration_branch,
            bare_path: &bare_path,
            remote: remote_res.as_ref(),
            skip_fetch_if_active: true,
        };
        match self
            .locks
            .with_lock(&bare_path, || prepare_worktree_locked(self, &args))
        {
            Ok(Ok(outcome)) => Some(outcome),
            Ok(Err(api_err)) => {
                eprintln!(
                    "agentic-org-office: worktree pre-prepare deferred for task {} repo {}: {}",
                    ctx.task.key(),
                    repo.id,
                    scrub_git_log_text(&api_err.message)
                );
                None
            }
            Err(git_err) => {
                eprintln!(
                    "agentic-org-office: worktree pre-prepare deferred for task {} repo {}: {}",
                    ctx.task.key(),
                    repo.id,
                    redact_git_error_for_log(&git_err)
                );
                None
            }
        }
    }

    fn notify_preprepared_worktree(&self, outcome: &PreparedWorktreeOutcome) {
        let Some(sessions) = self.sessions() else {
            return;
        };
        let wt = &outcome.result.worktree;
        let _ = sessions.on_worktree_prepared(&org_office_sessions::PreparedWorktreeNotice {
            employee_id: &wt.employee_id,
            triggered_by_self: false,
            task_key: &wt.task_key,
            repository_id: &wt.repository_id,
            admin_dir_name: &outcome.admin_dir_name,
            branch: &wt.branch,
            worktree_path: &wt.worktree_path,
        });
    }
}

impl org_office_sessions::TaskWorktreePreparer for GitService {
    fn preprepare_for_task(&self, employee_id: &str, task_ref: &str) {
        let _ = self.preprepare_task_worktrees_for_assignee(employee_id, task_ref);
    }
}

fn is_eligible_for_auto_preprepare(employee_id: &str, ctx: &org_office::TaskGitContext) -> bool {
    if matches!(ctx.task.status, TaskStatus::Done | TaskStatus::Cancelled) {
        return false;
    }
    // Fail closed: only auto-prepare when `employee_id` is explicitly the task's assignee and
    // resolved workspace owner (never for unassigned tasks, watchers, or another agent's task).
    ctx.task.assignee_id.as_ref().map(|id| id.as_str()) == Some(employee_id)
        && ctx.workspace_owner_id.as_str() == employee_id
}
