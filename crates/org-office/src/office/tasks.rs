//! High-level unified tasks facade on [`Office`] (`TASK-49`, `TASK-77`, `TASK-218`).
//!
//! Single-office task write authority (`TASK-52`, `TASK-192`) is enforced atomically inside each
//! mutation's `UnitOfWork` via [`TaskWriteAuthority`] rather than via a pre-commit snapshot check.

use std::collections::BTreeSet;

use org_proto::attachments::{TASK_ATTACHMENTS_MAX, TASK_COMMENT_ATTACHMENTS_MAX};
use org_proto::tasks::{
    TaskDependenciesResult, TaskInspectResult, TasksAddDependencyParams, TasksAssignParams,
    TasksChecklistAddParams, TasksChecklistCheckParams, TasksChecklistEditParams,
    TasksChecklistMoveParams, TasksChecklistRemoveParams, TasksChecklistResult, TasksCommentParams,
    TasksCommentResult, TasksCommentsResult, TasksCreateParams, TasksFinalSummaryParams,
    TasksHistoryResult, TasksLabelsParams, TasksLabelsResult, TasksListParams, TasksListResult,
    TasksRemoveDependencyParams, TasksTransitionParams, TasksUnwatchParams, TasksUpdateParams,
    TasksWatchParams,
};
use org_proto::ApiError;

use crate::error::OfficeError;
use crate::model::OfficeRole;
use crate::office::{storage, Office};
use crate::organization::records::OfficeRole as RecordOfficeRole;
use crate::store::open::task_ops::{
    TaskAddDependencyOp, TaskAssignOp, TaskChecklistAddOp, TaskChecklistCheckOp,
    TaskChecklistEditOp, TaskChecklistMoveOp, TaskChecklistRemoveOp, TaskCommentOp,
    TaskCommentsRead, TaskCreateOp, TaskDependenciesRead, TaskFinalSummaryOp, TaskHistoryRead,
    TaskInspectRead, TaskLabelsRead, TaskListRead, TaskRemoveDependencyOp, TaskTransitionOp,
    TaskUnwatchOp, TaskUpdateOp, TaskWatchOp,
};
use crate::tasks::features::TaskWriteOutcome;
use crate::tasks::ops::TaskWriteAuthority;

impl Office {
    /// Builds the [`TaskWriteAuthority`] checked atomically inside every task mutation's
    /// `UnitOfWork` against the live transaction view (`TASK-218`).
    pub(super) fn task_write_authority(&self) -> Result<TaskWriteAuthority, OfficeError> {
        let role = match self.info.role {
            OfficeRole::Hq => RecordOfficeRole::Hq,
            OfficeRole::Satellite => RecordOfficeRole::Satellite,
        };
        let removed_offices = if role == RecordOfficeRole::Hq {
            crate::office::invitations::removed_satellite_offices(self)?
        } else {
            BTreeSet::new()
        };
        Ok(TaskWriteAuthority::Local {
            office_id: self.identity.office_id.clone(),
            role,
            removed_offices,
        })
    }

    /// Returns whether this office holds local write authority for a task numbered with
    /// `task_office_code` (`TASK-52`, `TASK-192`).
    pub fn has_local_task_write_authority_for_code(
        &self,
        task_office_code: &str,
    ) -> Result<bool, OfficeError> {
        crate::office::satellite::has_local_task_write_authority(self, task_office_code)
    }

    fn run_task_write<O>(&self, op: O) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError>
    where
        O: crate::store::open::StoreOp<
            Out = Result<Result<TaskWriteOutcome, ApiError>, org_store::StoreError>,
        >,
    {
        let res = self.store.run(op).map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    fn run_checklist_write<O>(
        &self,
        op: O,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError>
    where
        O: crate::store::open::StoreOp<
            Out = Result<Result<(TasksChecklistResult, bool), ApiError>, org_store::StoreError>,
        >,
    {
        let res = self.store.run(op).map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_create(
        &self,
        actor: &str,
        params: TasksCreateParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let prepared_files = match self.prepare_task_files(
            actor,
            None,
            &params.attachment_ids,
            TASK_ATTACHMENTS_MAX,
            "a task",
        )? {
            Ok(files) => files,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let release = prepared_files.clone();
        let res = self
            .store
            .run(TaskCreateOp {
                actor: actor.to_owned(),
                here: self.identity.clone(),
                params,
                numbering: self.store.task_numbering(),
                prepared_files,
            })
            .map_err(storage)?;
        if res.is_ok() {
            self.release_task_files(&release);
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_update(
        &self,
        actor: &str,
        params: TasksUpdateParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        let prepared_files = match self.prepare_task_files(
            actor,
            Some(&params.task),
            &params.add_attachments,
            TASK_ATTACHMENTS_MAX,
            "a task",
        )? {
            Ok(files) => files,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let release = prepared_files.clone();
        let res = self
            .store
            .run(TaskUpdateOp {
                actor: actor.to_owned(),
                params,
                prepared_files,
                authority,
            })
            .map_err(storage)?;
        if res.is_ok() {
            self.release_task_files(&release);
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_transition(
        &self,
        actor: &str,
        params: TasksTransitionParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_task_write(TaskTransitionOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    /// Sets a task's attributed final summary (`TASK-88`).
    pub fn task_final_summary(
        &self,
        actor: &str,
        params: TasksFinalSummaryParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_task_write(TaskFinalSummaryOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_assign(
        &self,
        actor: &str,
        params: TasksAssignParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_task_write(TaskAssignOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_comment(
        &self,
        actor: &str,
        params: TasksCommentParams,
    ) -> Result<Result<(TasksCommentResult, bool), ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        let prepared_files = match self.prepare_task_files(
            actor,
            Some(&params.task),
            &params.attachment_ids,
            TASK_COMMENT_ATTACHMENTS_MAX,
            "a task comment",
        )? {
            Ok(files) => files,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let release = prepared_files.clone();
        let res = self
            .store
            .run(TaskCommentOp {
                actor: actor.to_owned(),
                params,
                prepared_files,
                authority,
            })
            .map_err(storage)?;
        if res.is_ok() {
            self.release_task_files(&release);
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_watch(
        &self,
        actor: &str,
        params: TasksWatchParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_task_write(TaskWatchOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_unwatch(
        &self,
        actor: &str,
        params: TasksUnwatchParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_task_write(TaskUnwatchOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_add_dependency(
        &self,
        actor: &str,
        params: TasksAddDependencyParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_task_write(TaskAddDependencyOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_remove_dependency(
        &self,
        actor: &str,
        params: TasksRemoveDependencyParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_task_write(TaskRemoveDependencyOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_checklist_add(
        &self,
        actor: &str,
        params: TasksChecklistAddParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_checklist_write(TaskChecklistAddOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_checklist_check(
        &self,
        actor: &str,
        params: TasksChecklistCheckParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_checklist_write(TaskChecklistCheckOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_checklist_edit(
        &self,
        actor: &str,
        params: TasksChecklistEditParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_checklist_write(TaskChecklistEditOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_checklist_remove(
        &self,
        actor: &str,
        params: TasksChecklistRemoveParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_checklist_write(TaskChecklistRemoveOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_checklist_move(
        &self,
        actor: &str,
        params: TasksChecklistMoveParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        let authority = self.task_write_authority()?;
        self.run_checklist_write(TaskChecklistMoveOp {
            actor: actor.to_owned(),
            params,
            authority,
        })
    }

    pub fn task_list(
        &self,
        params: TasksListParams,
    ) -> Result<Result<TasksListResult, ApiError>, OfficeError> {
        let (res, _) = self.store.read(TaskListRead { params }).map_err(storage)?;
        Ok(res)
    }

    pub fn task_inspect(
        &self,
        task: &str,
    ) -> Result<Result<TaskInspectResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskInspectRead {
                task: task.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn task_dependencies(
        &self,
        task: &str,
    ) -> Result<Result<TaskDependenciesResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskDependenciesRead {
                task: task.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn task_comments(
        &self,
        task: &str,
    ) -> Result<Result<TasksCommentsResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskCommentsRead {
                task: task.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn task_history(
        &self,
        task: &str,
    ) -> Result<Result<TasksHistoryResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskHistoryRead {
                task: task.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn task_labels(
        &self,
        params: TasksLabelsParams,
    ) -> Result<Result<TasksLabelsResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskLabelsRead { params })
            .map_err(storage)?;
        Ok(res)
    }
}
